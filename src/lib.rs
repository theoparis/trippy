use std::collections::BTreeMap;

use chumsky::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub enum Instruction {
	StringLiteral(String),
	NumericLiteral(f32),
	FunctionCall {
		name: String,
		args: Vec<Instruction>,
	},
	Array(Vec<Instruction>),
	Object(BTreeMap<String, Instruction>),
}

pub fn value() -> impl Parser<char, Instruction, Error = Simple<char>> {
	recursive(|value| {
		let quote = choice((just('"'), just('\'')));
		let escape = just('\\').ignore_then(
			just('\\')
				.or(just('/'))
				.or(just('"'))
				.or(just('b').to('\x08'))
				.or(just('f').to('\x0C'))
				.or(just('n').to('\n'))
				.or(just('r').to('\r'))
				.or(just('t').to('\t'))
				.or(just('u').ignore_then(
					filter(|c: &char| c.is_digit(16))
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

		let string = quote
			.ignore_then(
				filter(|c| *c != '\\' && *c != '"' && *c != '\'')
					.or(escape)
					.repeated(),
			)
			.then_ignore(quote)
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
		let fn_call = text::ident()
			.separated_by(just('.'))
			.map(|v| v.join("."))
			.padded()
			.then_ignore(just('('))
			.padded()
			.then(value.separated_by(just(',')))
			.padded()
			.then_ignore(just(')'))
			.labelled("fn_call")
			.map(|(ident, args)| Instruction::FunctionCall {
				name: ident,
				args,
			});

		choice((string_literal, num_literal, fn_call, object))
	})
}

pub fn parser() -> impl Parser<char, Vec<Instruction>, Error = Simple<char>> {
	value()
		.then_ignore(just(';').or_not())
		.padded()
		.repeated()
		.then_ignore(end())
}
