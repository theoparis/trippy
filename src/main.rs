use ariadne::{Color, Fmt, Label, Report, ReportKind, Source};
use chumsky::Parser;
use std::{collections::BTreeMap, io::BufRead};
use trippy::{parser, Instruction};

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
	let src = std::fs::read_to_string(
		std::env::args().nth(1).expect("Expected file argument"),
	)
	.expect("Failed to read file");

	let ast = parse(&src);

	let mut variables: BTreeMap<String, Instruction> = Default::default();

	dbg!(ast.clone());

	let mut i = 0;

	loop {
		let mut line = String::new();
		let stdin = std::io::stdin();
		stdin.lock().read_line(&mut line).unwrap();

		if i >= ast.len() {
			break;
		}

		let value = ast.get(i);
		i += 1;

		if let Some(value) = value {
			match value {
				Instruction::FunctionCall { name, args } => {
					match name.as_str() {
						"console.log" => {
							for arg in args {
								match arg {
									Instruction::VariableReference(
										variable_reference,
									) => {
										let variable =
											variables.get(variable_reference);

										if let Some(variable) = variable {
											println!("{}", variable);
										} else {
											eprintln!(
												"Variable does not exist: {}",
												variable_reference
											);
										}
									}
									_ => println!("{}", arg),
								}
							}
						}
						_ => eprintln!("Unknown function: {}", name),
					}
				}
				// TODO: implement scopes and function definitions
				Instruction::Variable { name, value, .. } => {
					variables.insert(name.to_string(), *value.to_owned());
				}
				_ => {}
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use trippy::Instruction;

	use crate::parse;

	#[test]
	fn test_variables() {
		let source = r#"
		let x = 123.456;
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
