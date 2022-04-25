use chumsky::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub enum Instruction {
    StringLiteral(String),
    NumericLiteral(f32),
    FunctionCall {
        name: String,
        args: Vec<Instruction>,
    },
}

pub fn str_literal() -> impl Parser<char, Instruction, Error = Simple<char>> {
    just('"')
        .ignore_then(filter(|c| *c != '"').repeated())
        .then_ignore(just('"'))
        .collect::<String>()
        .map(Instruction::StringLiteral)
}

pub fn num_literal() -> impl Parser<char, Instruction, Error = Simple<char>> {
    text::int(10)
        .chain::<char, _, _>(
            just('.').chain(text::digits(10)).or_not().flatten(),
        )
        .collect::<String>()
        .map(|n| Instruction::NumericLiteral(n.parse().unwrap()))
}

pub fn fn_call() -> impl Parser<char, Instruction, Error = Simple<char>> {
    recursive(|fn_call_parser| {
        text::ident()
            .separated_by(just('.'))
            .map(|v| v.join("."))
            .then_ignore(just('('))
            .then(
                choice((str_literal(), num_literal(), fn_call_parser))
                    .separated_by(just(',')),
            )
            .then_ignore(just(')'))
            .map(|(ident, args)| Instruction::FunctionCall {
                name: ident,
                args,
            })
    })
}

pub fn parser() -> impl Parser<char, Vec<Instruction>, Error = Simple<char>> {
    recursive(|_parser| {
        choice((str_literal(), num_literal(), fn_call()))
            .then_ignore(just(';').or_not())
            .repeated()
    })
    .then_ignore(end())
}
