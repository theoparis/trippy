use std::collections::BTreeMap;

use chumsky::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VariableScope {
	Let,
	Const,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Instruction {
	StringLiteral(String),
	NumericLiteral(f64),
	BooleanLiteral(bool),
	FunctionCall {
		name: String,
		args: Vec<Instruction>,
	},
	Array(Vec<Instruction>),
	Object(BTreeMap<String, Instruction>),
	Variable {
		scope: VariableScope,
		name: String,
		value: Box<Instruction>,
	},
	VariableReference(String),
	WhileBlock {
		condition: Box<Instruction>,
		body: Vec<Instruction>,
	},
}

impl std::fmt::Display for Instruction {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Instruction::StringLiteral(value) => {
				write!(f, "{}", value)?;
			}
			Instruction::NumericLiteral(value) => {
				write!(f, "{}", value)?;
			}
			Instruction::BooleanLiteral(value) => {
				write!(f, "{}", value)?;
			}
			Instruction::FunctionCall { name, args } => {
				write!(f, "{}(", name)?;

				for arg in args {
					write!(f, "{}", arg)?;
				}

				write!(f, ")")?;
			}
			Instruction::Array(values) => {
				for value in values {
					write!(f, "{}", value)?;
				}
			}
			Instruction::VariableReference(_name) => unimplemented!(),
			Instruction::Object(_map) => {
				unimplemented!()
			}
			Instruction::Variable { .. } => unimplemented!(),
			Instruction::WhileBlock { .. } => unimplemented!(),
		}

		Ok(())
	}
}

pub fn value() -> impl Parser<char, Instruction, Error = Simple<char>> {
	recursive(|value| {
		let quote = choice((just('"'), just('\'')));
		let escape =
			just('\\').ignore_then(
				just('\\')
					.or(just('/'))
					.or(just('"'))
					.or(just('b').to('\x08'))
					.or(just('f').to('\x0C'))
					.or(just('n').to('\n'))
					.or(just('r').to('\r'))
					.or(just('t').to('\t'))
					.or(just('u').ignore_then(
						filter(|c: &char| c.is_ascii_hexdigit())
							.repeated()
							.exactly(4)
							.collect::<String>()
							.validate(|digits, span, emit| {
								char::from_u32(
									u32::from_str_radix(&digits, 16).unwrap(),
								)
								.unwrap_or_else(|| {
									emit(Simple::custom(
										span,
										"invalid unicode character",
									));
									'\u{FFFD}' // unicode replacement character
								})
							}),
					)),
			);

		let string = filter(|c| *c != '\\' && *c != '"' && *c != '\'')
			.or(escape)
			.repeated()
			.delimited_by(quote, quote)
			.collect::<String>();

		let string_literal = string
			.labelled("string_literal")
			.map(Instruction::StringLiteral);

		let num_literal = text::int(10)
			.chain::<char, _, _>(
				just('.').chain(text::digits(10)).or_not().flatten(),
			)
			.collect::<String>()
			.labelled("numeric_literal")
			.map(|n| Instruction::NumericLiteral(n.parse().unwrap()));

		let bool_literal = just("true")
			.or(just("false"))
			.labelled("boolean_literal")
			.map(|b| Instruction::BooleanLiteral(b.parse().unwrap()));

		let member = choice((string, text::ident()))
			.labelled("identifier")
			.padded()
			.then_ignore(just(':').padded())
			.padded()
			.then(value.clone());

		let object = member
			.clone()
			.chain(just(',').padded().ignore_then(member).repeated())
			.or_not()
			.flatten()
			.padded()
			.delimited_by(just('{'), just('}'))
			.collect::<BTreeMap<String, Instruction>>()
			.map(Instruction::Object)
			.labelled("object");

		//let args = text::ident()
		//.separated_by(just(','))
		//.allow_trailing()
		//.delimited_by(just('('), just(')'))
		//.labelled("function args");

		let fn_call = text::ident()
			.separated_by(just('.'))
			.padded()
			.map(|v| v.join("."))
			.then(
				value
					.clone()
					.padded()
					.separated_by(just(','))
					.allow_trailing()
					.delimited_by(just('('), just(')')),
			)
			.labelled("function call")
			.map(|(ident, args)| Instruction::FunctionCall {
				name: ident,
				args,
			});

		let variable = choice((just("let"), just("const")))
			.padded()
			.then(text::ident().labelled("variable_name").padded())
			.then_ignore(just('=').padded())
			.then(value.padded())
			.labelled("variable")
			.map(|((scope, name), value)| Instruction::Variable {
				scope: match scope {
					"let" => VariableScope::Let,
					"const" => VariableScope::Const,
					_ => panic!("Invalid variable scope: {}", scope),
				},
				name,
				value: Box::new(value),
			});

		let variable_reference = text::ident()
			.padded()
			.labelled("variable_reference")
			.map(Instruction::VariableReference);

		choice((
			string_literal,
			num_literal,
			bool_literal,
			fn_call,
			object,
			variable,
			variable_reference,
		))
	})
}

pub fn parser() -> impl Parser<char, Vec<Instruction>, Error = Simple<char>> {
	let block = value()
		.then_ignore(just(';').or_not())
		.padded()
		.repeated()
		.padded()
		.or_not()
		.delimited_by(just('{'), just('}'))
		.labelled("block");

	let while_block = just("while")
		.padded()
		.ignore_then(value().padded().delimited_by(just('('), just(')')))
		.padded()
		.then(block)
		.padded()
		.labelled("while_block")
		.map(|cond| Instruction::WhileBlock {
			condition: Box::new(cond.0),
			body: cond.1.unwrap_or_default(),
		});

	while_block
		//.or(value().then_ignore(just(';').or_not()))
		.padded()
		.repeated()
		.then_ignore(end())
}
