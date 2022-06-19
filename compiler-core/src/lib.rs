use ariadne::{Color, Fmt, Label, Report, ReportKind, Source};
use chumsky::Parser;
use cranelift::{codegen::ir::Function, prelude::*};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataContext, Linkage, Module, ModuleError};
use miette::{Diagnostic, Result};
use std::collections::BTreeMap;
use thiserror::Error;
use trippy::{parser, Instruction};

#[derive(Error, Diagnostic, Debug)]
pub enum TrippyError {
	#[error(transparent)]
	#[diagnostic(code(trippy_compiler::io_error))]
	IoError(#[from] std::io::Error),

	#[error("Failed to initialize jit environment: {0}")]
	#[diagnostic(code(trippy_compiler::jit_initialization))]
	JitInitialization(#[from] ModuleError),

	#[error("Failed to compile: {0}")]
	#[diagnostic(code(trippy_compiler::compile))]
	Compiler(String),
}

pub fn initialize<'a>(
	func: &'a mut Function,
	builder_context: &'a mut FunctionBuilderContext,
) -> FunctionBuilder<'a> {
	FunctionBuilder::new(func, builder_context)
}

pub struct JIT {
	/// The data context, which is to data objects what `ctx` is to functions.
	pub data_ctx: DataContext,

	/// The module, with the jit backend, which manages the JIT'd
	/// functions.
	pub module: JITModule,
	pub variables: BTreeMap<String, Variable>,
	pub objects: BTreeMap<String, BTreeMap<String, Variable>>,
	pub num: Type,
}

impl JIT {
	pub fn get_module(&mut self) -> &JITModule {
		&self.module
	}

	pub fn new() -> Result<Self, TrippyError> {
		let builder =
			JITBuilder::new(cranelift_module::default_libcall_names())
				.map_err(TrippyError::JitInitialization)?;
		let module = JITModule::new(builder);
		let num = module.target_config().pointer_type();

		Ok(Self {
			data_ctx: DataContext::new(),
			module,
			variables: BTreeMap::new(),
			objects: BTreeMap::new(),
			num,
		})
	}
}

/// Compile a string in the toy language into machine code.
pub fn compile(jit: &mut JIT, ast: Vec<Instruction>) -> Result<*const u8> {
	let mut ctx = jit.module.make_context();
	let mut builder_context = FunctionBuilderContext::new();

	ctx.func.signature.returns.push(AbiParam::new(jit.num));

	let mut func_builder =
		FunctionBuilder::new(&mut ctx.func, &mut builder_context);

	//for _p in &params {
	//jit.ctx.func.signature.params.push(AbiParam::new(jit.num));
	//}

	// Create the entry block, to start emitting code in.
	let entry_block = func_builder.create_block();

	// Since this is the entry block, add block parameters corresponding to
	// the function's parameters.
	func_builder.append_block_params_for_function_params(entry_block);

	// Tell the builder to emit code in this block.
	func_builder.switch_to_block(entry_block);

	// And, tell the builder that this block will have no further
	// predecessors. Since it's the entry block, it won't have any
	// predecessors.
	func_builder.seal_block(entry_block);

	// The toy language allows variables to be declared implicitly.
	// Walk the AST and declare all implicitly-declared variables.
	// Now translate the statements of the function body.
	for expr in ast {
		translate_expr(jit, &mut func_builder, expr);
	}

	// Set up the return variable of the function. Above, we declared a
	// variable to hold the return value. Here, we just do a use of that
	// variable.
	//let return_variable = jit.variables.get_mut("main").unwrap();
	//let return_value = func_builder.use_var(*return_variable);

	let ret = func_builder.ins().iconst(cranelift::prelude::types::I64, 0);

	// Emit the return instruction.
	func_builder.ins().return_(&[ret]);

	// Tell the builder we're done with this function.
	func_builder.finalize();

	let id = jit
		.module
		.declare_function("main", Linkage::Export, &ctx.func.signature)
		.map_err(TrippyError::JitInitialization)?;

	jit.module
		.define_function(id, &mut ctx)
		.map_err(TrippyError::JitInitialization)?;

	//println!("{}", ctx.func);
	jit.module.clear_context(&mut ctx);

	jit.module.finalize_definitions();

	let code = jit.module.get_finalized_function(id);

	Ok(code)
}

/// Create a zero-initialized data section.
pub fn create_data(
	jit: &mut JIT,
	name: &str,
	contents: Vec<u8>,
) -> Result<Vec<u8>, String> {
	// The steps here are analogous to `compile`, except that data is much
	// simpler than functions.
	jit.data_ctx.define(contents.into_boxed_slice());
	let id = jit
		.module
		.declare_data(name, Linkage::Export, true, false)
		.map_err(|e| e.to_string())?;

	jit.module
		.define_data(id, &jit.data_ctx)
		.map_err(|e| e.to_string())?;
	jit.data_ctx.clear();
	jit.module.finalize_definitions();
	let buffer = jit.module.get_finalized_data(id);
	// TODO: Can we move the unsafe into cranelift?
	Ok(unsafe { std::slice::from_raw_parts(buffer.0, buffer.1).to_vec() })
}

fn create_anonymous_string(
	jit: &mut JIT,
	func_builder: &mut FunctionBuilder,
	string_content: &str,
) -> Value {
	jit.data_ctx
		.define(string_content.as_bytes().to_vec().into_boxed_slice());

	let sym = jit
		.module
		.declare_anonymous_data(true, false)
		.expect("problem declaring data object");

	let _result = jit
		.module
		.define_data(sym, &jit.data_ctx)
		.map_err(|e| e.to_string());

	let local_id = jit.module.declare_data_in_func(sym, func_builder.func);
	jit.data_ctx.clear();

	let pointer = jit.module.target_config().pointer_type();
	func_builder.ins().symbol_value(pointer, local_id)
}

/// When you write out instructions in Cranelift, you get back `Value`s. You
pub fn translate_expr(
	jit: &mut JIT,
	func_builder: &mut FunctionBuilder,
	expr: Instruction,
) -> Value {
	match expr {
		Instruction::NumericLiteral(literal) => {
			if literal.trunc() == literal {
				func_builder
					.ins()
					.iconst(cranelift::prelude::types::I64, literal as i64)
			} else {
				func_builder.ins().f64const(literal)
			}
		}
		Instruction::StringLiteral(literal) => {
			let mut string_content_with_terminator = literal;
			string_content_with_terminator.push('\0');
			create_anonymous_string(
				jit,
				func_builder,
				&string_content_with_terminator,
			)
		}
		Instruction::BooleanLiteral(b) => {
			func_builder.ins().iconst(jit.num, i64::from(b))
		}
		Instruction::FunctionCall { name, args } => {
			let is_extern = name.contains("_ext");
			translate_call(jit, func_builder, name, args, is_extern)
		}
		Instruction::VariableReference(name) => {
			// `use_var` is used to read the value of a variable.
			let variable =
				jit.variables.get(&name).expect("variable not defined");
			func_builder.use_var(*variable)
		}
		Instruction::Array(value) => panic!("not implemented: {:#?}", value),
		Instruction::Object(value) => panic!("not implemented: {:#?}", value),
		Instruction::Variable {
			scope: _,
			name,
			value,
		} => {
			//panic!("not implemented: variable; {}", name);
			let var = declare_variable(
				jit.num,
				func_builder,
				&mut jit.variables,
				&mut 0,
				&name,
			);
			let val = translate_expr(jit, func_builder, *value);
			func_builder.def_var(var, val);
			func_builder.use_var(var)
		}
		Instruction::WhileBlock { condition, body } => {
			translate_while_loop(jit, func_builder, *condition, body)
		}
	}
}

pub fn translate_assign(
	jit: &mut JIT,
	func_builder: &mut FunctionBuilder,
	name: String,
	expr: Instruction,
) -> Value {
	// `def_var` is used to write the value of a variable. Note that
	// variables can have multiple definitions. Cranelift will
	// convert them into SSA form for itjit automatically.
	let new_value = translate_expr(jit, func_builder, expr);
	let variable = jit.variables.get(&name).unwrap();
	func_builder.def_var(*variable, new_value);
	new_value
}

pub fn translate_icmp(
	jit: &mut JIT,
	func_builder: &mut FunctionBuilder,
	cmp: IntCC,
	lhs: Instruction,
	rhs: Instruction,
) -> Value {
	let lhs = translate_expr(jit, func_builder, lhs);
	let rhs = translate_expr(jit, func_builder, rhs);
	let c = func_builder.ins().icmp(cmp, lhs, rhs);
	func_builder.ins().bint(jit.num, c)
}

pub fn translate_if_else(
	jit: &mut JIT,
	func_builder: &mut FunctionBuilder,
	condition: Instruction,
	then_body: Vec<Instruction>,
	else_body: Vec<Instruction>,
) -> Value {
	let condition_value = translate_expr(jit, func_builder, condition);

	let then_block = func_builder.create_block();
	let else_block = func_builder.create_block();
	let merge_block = func_builder.create_block();

	// If-else constructs in the toy language have a return value.
	// In traditional SSA form, this would produce a PHI between
	// the then and else bodies. Cranelift uses block parameters,
	// so set up a parameter in the merge block, and we'll pass
	// the return values to it from the branches.
	func_builder.append_block_param(merge_block, jit.num);

	// Test the if condition and conditionally branch.
	func_builder.ins().brz(condition_value, else_block, &[]);
	// Fall through to then block.
	func_builder.ins().jump(then_block, &[]);

	func_builder.switch_to_block(then_block);
	func_builder.seal_block(then_block);
	let mut then_return = func_builder.ins().iconst(jit.num, 0);
	for expr in then_body {
		then_return = translate_expr(jit, func_builder, expr);
	}

	// Jump to the merge block, passing it the block return value.
	func_builder.ins().jump(merge_block, &[then_return]);

	func_builder.switch_to_block(else_block);
	func_builder.seal_block(else_block);
	let mut else_return = func_builder.ins().iconst(jit.num, 0);
	for expr in else_body {
		else_return = translate_expr(jit, func_builder, expr);
	}

	// Jump to the merge block, passing it the block return value.
	func_builder.ins().jump(merge_block, &[else_return]);

	// Switch to the merge block for subsequent statements.
	func_builder.switch_to_block(merge_block);

	// We've now seen all the predecessors of the merge block.
	func_builder.seal_block(merge_block);

	// Read the value of the if-else by reading the merge block
	// parameter.
	let phi = func_builder.block_params(merge_block)[0];

	phi
}

pub fn translate_while_loop(
	jit: &mut JIT,
	func_builder: &mut FunctionBuilder,
	condition: Instruction,
	loop_body: Vec<Instruction>,
) -> Value {
	let header_block = func_builder.create_block();
	let body_block = func_builder.create_block();
	let exit_block = func_builder.create_block();

	func_builder.ins().jump(header_block, &[]);
	func_builder.switch_to_block(header_block);

	let condition_value = translate_expr(jit, func_builder, condition);
	func_builder.ins().brz(condition_value, exit_block, &[]);
	func_builder.ins().jump(body_block, &[]);

	func_builder.switch_to_block(body_block);
	func_builder.seal_block(body_block);

	for expr in loop_body {
		translate_expr(jit, func_builder, expr);
	}
	func_builder.ins().jump(header_block, &[]);

	func_builder.switch_to_block(exit_block);

	// We've reached the bottom of the loop, so there will be no
	// more backedges to the header to exits to the bottom.
	func_builder.seal_block(header_block);
	func_builder.seal_block(exit_block);

	// Just return 0 for now.
	func_builder.ins().iconst(jit.num, 0)
}

pub fn translate_call(
	jit: &mut JIT,
	func_builder: &mut FunctionBuilder,
	name: String,
	args: Vec<Instruction>,
	is_extern: bool,
) -> Value {
	let mut sig = jit.module.make_signature();

	// Add a parameter for each argument.
	for _arg in &args {
		sig.params.push(AbiParam::new(jit.num));
	}

	// For simplicity for now, just make all calls return a single I64.
	sig.returns.push(AbiParam::new(jit.num));

	let callee = if is_extern {
		jit.module
			.declare_function(&name.replace("_ext", ""), Linkage::Import, &sig)
			.expect("problem declaring function")
	} else {
		jit.module
			.declare_function(&name, Linkage::Local, &sig)
			.expect("problem declaring function")
	};
	let local_callee =
		jit.module.declare_func_in_func(callee, func_builder.func);

	let mut arg_values = Vec::new();
	for arg in args {
		arg_values.push(translate_expr(jit, func_builder, arg))
	}
	let call = func_builder.ins().call(local_callee, &arg_values);
	func_builder.inst_results(call)[0]
}

pub fn translate_global_data_addr(
	jit: &mut JIT,
	func_builder: &mut FunctionBuilder,
	name: String,
) -> Value {
	let sym = jit
		.module
		.declare_data(&name, Linkage::Export, true, false)
		.expect("problem declaring data object");
	let local_id = jit.module.declare_data_in_func(sym, func_builder.func);

	let pointer = jit.module.target_config().pointer_type();
	func_builder.ins().symbol_value(pointer, local_id)
}

pub fn declare_variables(
	int: types::Type,
	func_builder: &mut FunctionBuilder,
	params: &[String],
	the_return: &str,
	stmts: &[Instruction],
	entry_block: Block,
) -> BTreeMap<String, Variable> {
	let mut variables = BTreeMap::new();
	let mut index = 0;

	for (i, name) in params.iter().enumerate() {
		// TODO: cranelift_frontend should really have an API to make it easy to set
		// up param variables.
		let val = func_builder.block_params(entry_block)[i];
		let var = declare_variable(
			int,
			func_builder,
			&mut variables,
			&mut index,
			name,
		);
		func_builder.def_var(var, val);
	}
	let zero = func_builder.ins().iconst(int, 0);
	let return_variable = declare_variable(
		int,
		func_builder,
		&mut variables,
		&mut index,
		the_return,
	);
	func_builder.def_var(return_variable, zero);
	for expr in stmts {
		declare_variables_in_stmt(
			int,
			func_builder,
			&mut variables,
			&mut index,
			expr,
		);
	}

	variables
}

/// Recursively descend through the AST, translating all implicit
/// variable declarations.
fn declare_variables_in_stmt(
	int: types::Type,
	builder: &mut FunctionBuilder,
	variables: &mut BTreeMap<String, Variable>,
	index: &mut usize,
	expr: &Instruction,
) {
	if let Instruction::Variable {
		ref name,
		value: _,
		scope: _,
	} = expr
	{
		declare_variable(int, builder, variables, index, name);
	}
	//Instruction::IfElse(ref _condition, ref then_body, ref else_body) => {
	//for stmt in then_body {
	//declare_variables_in_stmt(int, builder, variables, index, stmt);
	//}
	//for stmt in else_body {
	//declare_variables_in_stmt(int, builder, variables, index, stmt);
	//}
	//}
	//Instruction::WhileLoop(ref _condition, ref loop_body) => {
	//for stmt in loop_body {
	//declare_variables_in_stmt(int, builder, variables, index, stmt);
	//}
	//}
}

/// Declare a single variable declaration.
fn declare_variable(
	int: types::Type,
	func_builder: &mut FunctionBuilder,
	variables: &mut BTreeMap<String, Variable>,
	index: &mut usize,
	name: &str,
) -> Variable {
	let var = Variable::new(*index);
	if !variables.contains_key(name) {
		variables.insert(name.into(), var);
		func_builder.declare_var(var, int);
		*index += 1;
	}
	var
}

pub fn parse(src: &str) -> Vec<Instruction> {
	let (ast, errs) = parser().parse_recovery(src.trim());
	errs.into_iter().for_each(|e| {
		let msg = if let chumsky::error::SimpleReason::Custom(msg) = e.reason()
		{
			msg.clone()
		} else {
			format!(
				"{}{}, expected {}",
				if e.found().is_some() {
					"Unexpected token"
				} else {
					"Unexpected end of input"
				},
				if let Some(label) = e.label() {
					format!(" while parsing {}", label)
				} else {
					String::new()
				},
				if e.expected().len() == 0 {
					"something else".to_string()
				} else {
					e.expected()
						.map(|expected| match expected {
							Some(expected) => expected.to_string(),
							None => "end of input".to_string(),
						})
						.collect::<Vec<_>>()
						.join(", ")
				},
			)
		};

		let report = Report::build(ReportKind::Error, (), e.span().start)
			.with_code(3)
			.with_message(msg)
			.with_label(
				Label::new(e.span())
					.with_message(match e.reason() {
						chumsky::error::SimpleReason::Custom(msg) => {
							msg.clone()
						}
						_ => format!(
							"Unexpected {}",
							e.found()
								.map(|c| format!("token {}", c.fg(Color::Red)))
								.unwrap_or_else(|| "end of input".to_string())
						),
					})
					.with_color(Color::Red),
			);

		let report = match e.reason() {
			chumsky::error::SimpleReason::Unclosed { span, delimiter } => {
				report.with_label(
					Label::new(span.clone())
						.with_message(format!(
							"Unclosed delimiter {}",
							delimiter.fg(Color::Yellow)
						))
						.with_color(Color::Yellow),
				)
			}
			chumsky::error::SimpleReason::Unexpected => report,
			chumsky::error::SimpleReason::Custom(_) => report,
		};

		report.finish().print(Source::from(&src)).unwrap();
	});

	if ast.is_none() {
		std::process::exit(1);
	}

	ast.unwrap()
}

#[cfg(test)]
mod tests {
	use crate::parse;
	use trippy::Instruction;

	#[test]
	fn test_console_log() {
		let source = r#"console.log("Hello world")"#;

		let ast = parse(source);

		assert_eq!(
			ast[0],
			Instruction::FunctionCall {
				name: "console.log".to_string(),
				args: vec![Instruction::StringLiteral(
					"Hello world".to_string()
				)]
			}
		);
	}

	#[test]
	fn test_variables() {
		let source = r#"
		let x = 123.456;

		console.log("%f", x);
		"#;

		let ast = parse(source);

		assert_eq!(
			ast[0],
			Instruction::Variable {
				scope: trippy::VariableScope::Let,
				name: "x".to_string(),
				value: Box::new(Instruction::NumericLiteral(123.456))
			}
		);
	}
}
