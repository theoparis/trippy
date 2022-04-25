use chumsky::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Instruction {
    StringLiteral(String),
    FunctionCall {
        name: String,
        args: Vec<Instruction>,
    },
}

pub fn str_parser() -> impl Parser<char, Instruction, Error = Simple<char>> {
    just('"')
        .ignore_then(filter(|c| *c != '"').repeated())
        .then_ignore(just('"'))
        .collect::<String>()
        .map(Instruction::StringLiteral)
}

pub fn fn_call() -> impl Parser<char, Instruction, Error = Simple<char>> {
    text::ident()
        .separated_by(just('.'))
        .map(|v| v.join("."))
        .then_ignore(just('('))
        .then(str_parser().separated_by(just(',')))
        .then_ignore(just(')'))
        .map(|(ident, args)| Instruction::FunctionCall { name: ident, args })
}

pub fn parser() -> impl Parser<char, Vec<Instruction>, Error = Simple<char>> {
    recursive(|_parser| {
        choice((str_parser(), fn_call()))
            .then_ignore(just(';'))
            .repeated()
    })
    .then_ignore(end())
}
