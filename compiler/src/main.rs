use ariadne::{Color, Fmt, Label, Report, ReportKind, Source};
use chumsky::Parser;
use inkwell::{
	builder::Builder,
	context::Context as InkContext,
	execution_engine::JitFunction,
	module::Linkage,
	types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum},
	values::{BasicMetadataValueEnum, CallableValue, PointerValue},
	AddressSpace, OptimizationLevel,
};
use std::collections::BTreeMap;
use trippy::{parser, Instruction};

#[allow(clippy::needless_lifetimes)]
pub fn create_llvm_type<'a>(
	ctx: &'a InkContext,
	type_name: &str,
) -> BasicTypeEnum<'a> {
	let i32_type = ctx.i32_type();
	let bool_type = ctx.bool_type();
	let f64_type = ctx.f64_type();
	let i8_type = ctx.i8_type();
	let str_type = ctx.i8_type().ptr_type(AddressSpace::Generic);

	match type_name {
		"i8" => i8_type.into(),
		"i32" => i32_type.into(),
		"f64" => f64_type.into(),
		"string" => str_type.into(),
		"boolean" => bool_type.into(),
		_ => unimplemented!(),
	}
}

#[allow(clippy::needless_lifetimes)]
pub fn create_llvm_arg_types<'a>(
	ctx: &'a InkContext,
	args: impl Iterator<Item = Instruction>,
) -> Vec<BasicMetadataTypeEnum<'a>> {
	args.map(|arg| match arg {
		Instruction::StringLiteral(type_name) => {
			create_llvm_type(ctx, &type_name).into()
		}
		_ => unimplemented!(),
	})
	.collect()
}

pub fn create_llvm_args<'a>(
	builder: &Builder<'a>,
	variables: &BTreeMap<String, PointerValue<'a>>,
	args: Vec<Instruction>,
) -> Vec<BasicMetadataValueEnum<'a>> {
	let mut llvm_args = vec![];

	for arg in args {
		match arg {
			Instruction::StringLiteral(value) => {
				llvm_args.push(BasicMetadataValueEnum::PointerValue(
					builder
						.build_global_string_ptr(value.as_str(), "temp")
						.as_pointer_value(),
				));
			}
			Instruction::VariableReference(name) => {
				let variable = variables.get(&name);

				if variable.is_none() {
					panic!("Unknown variable {}", name);
				}

				let var_arg = builder.build_load(*variable.unwrap(), &name);

				if var_arg.is_pointer_value() {
					llvm_args.push(BasicMetadataValueEnum::PointerValue(
						var_arg.into_pointer_value(),
					));
				} else if var_arg.is_float_value() {
					llvm_args.push(BasicMetadataValueEnum::FloatValue(
						var_arg.into_float_value(),
					));
				} else if var_arg.is_int_value() {
					llvm_args.push(BasicMetadataValueEnum::IntValue(
						var_arg.into_int_value(),
					));
				} else {
				}
			}
			_ => unimplemented!(),
		}
	}

	llvm_args
}

pub fn build(ast: Vec<Instruction>, module_name: &str) {
	let ctx = InkContext::create();
	let module = ctx.create_module(module_name);

	let builder = ctx.create_builder();

	let i32_type = ctx.i32_type();
	let f64_type = ctx.f64_type();
	let str_type = ctx.i8_type().ptr_type(AddressSpace::Generic);

	let main_fn_type = i32_type.fn_type(&[], false);
	let main_fn = module.add_function("main", main_fn_type, None);
	let entry_basic_block = ctx.append_basic_block(main_fn, "entry");

	builder.position_at_end(entry_basic_block);

	let mut variables = BTreeMap::new();

	for instruction in ast {
		match instruction {
			Instruction::Variable {
				scope: _scope,
				name,
				value,
			} => {
				let value = *value;
				let variable_type = match value.clone() {
					Instruction::StringLiteral(_) => {
						BasicTypeEnum::PointerType(str_type)
					}
					Instruction::NumericLiteral(_) => {
						BasicTypeEnum::FloatType(f64_type)
					}
					Instruction::FunctionCall { name, args } => {
						match name.as_str() {
							"loadExternalFunction" => match &args[1] {
								Instruction::StringLiteral(return_type) => {
									match &args[0] {
										Instruction::StringLiteral(_) => {
											let return_type = create_llvm_type(
												&ctx,
												return_type,
											);
											let fun_type = return_type.fn_type(
												create_llvm_arg_types(
													&ctx,
													args.clone()
														.into_iter()
														.skip(2),
												)
												.as_slice(),
												true,
											);

											BasicTypeEnum::PointerType(
												fun_type.ptr_type(
													AddressSpace::Generic,
												),
											)
										}
										_ => unimplemented!(),
									}
								}
								_ => unimplemented!(),
							},
							_ => unimplemented!(),
						}
					}
					_ => unimplemented!(),
				};

				let mut alloca = {
					match entry_basic_block.get_first_instruction() {
						Some(first_instr) => {
							builder.position_before(&first_instr)
						}
						None => builder.position_at_end(entry_basic_block),
					}

					builder.build_alloca(variable_type, &name)
				};

				match value {
					Instruction::StringLiteral(value) => {
						builder.build_store(
							alloca,
							ctx.const_string(value.as_bytes(), true),
						);
					}
					Instruction::NumericLiteral(value) => {
						builder
							.build_store(alloca, f64_type.const_float(value));
					}
					Instruction::FunctionCall { name, args } => {
						match name.as_str() {
							"loadExternalFunction" => match &args[0] {
								Instruction::StringLiteral(fun_name) => {
									match &args[1] {
										Instruction::StringLiteral(
											return_type,
										) => {
											let return_type = create_llvm_type(
												&ctx,
												return_type,
											);
											let fun_type = return_type.fn_type(
												create_llvm_arg_types(
													&ctx,
													args.clone()
														.into_iter()
														.skip(2),
												)
												.as_slice(),
												true,
											);

											let fun = module.add_function(
												fun_name,
												fun_type,
												Some(Linkage::External),
											);

											let fun_ptr = fun
												.as_global_value()
												.as_pointer_value();

											alloca = builder.build_alloca(
												fun_ptr.get_type(),
												&format!("alloca_{}", fun_name),
											);
											builder
												.build_store(alloca, fun_ptr);
										}
										_ => unimplemented!(),
									}
								}
								_ => unimplemented!(),
							},
							_ => unimplemented!(),
						}
					}
					_ => unimplemented!(),
				}

				variables.insert(name, alloca);
			}
			Instruction::FunctionCall { name, args } => {
				let variable = variables.get(&name);

				if let Some(variable) = variable {
					let function = builder.build_load(*variable, "load");
					let function = function.into_pointer_value();
					let args = create_llvm_args(&builder, &variables, args);

					builder.build_call(
						CallableValue::try_from(function).unwrap(),
						&args,
						&name,
					);
				} else {
					panic!("Invalid function {}", name);
				}
			}
			_ => unimplemented!(),
		}
	}

	builder.build_return(Some(&i32_type.const_zero()));

	//println!(
	//"Generated LLVM IR: {}",
	//main_fn.print_to_string().to_string()
	//);

	if let Err(err) = module.verify() {
		eprintln!("{}", err);
		std::process::exit(1);
	}

	let execution_engine = module
		.create_jit_execution_engine(OptimizationLevel::Default)
		.unwrap();

	unsafe {
		let jit_function: JitFunction<unsafe extern "C" fn() -> i32> =
			execution_engine.get_function("main").unwrap();

		jit_function.call();
	}
}

fn parse(src: &str) -> Vec<Instruction> {
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

fn main() {
	let src_name = std::env::args().nth(1).expect("Expected file argument");
	let src =
		std::fs::read_to_string(src_name.clone()).expect("Failed to read file");

	let ast = parse(&src);

	//dbg!(ast.clone());

	build(ast, &src_name);
}

#[cfg(test)]
mod tests {
	use crate::{build, parse};
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

		build(ast, "test");
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

		build(ast, "test");
	}
}
