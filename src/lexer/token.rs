use crate::common::span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Identifier(String, Span),
    Number(i64, Span),
    String(String, Span),
    Keyword(Keyword, Span),
    Symbol(Symbol, Span),
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keyword {
    Fn,
    Let,
    Mut,
    If,
    Else,
    For,
    While,
    Return,
    Import,
    From,
    Async,
    Await,
    Spawn,
    Pub,
    Struct,
    Enum,
    Trait,
    Impl,
    Module,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Symbol {
    LeftBrace,
    RightBrace,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Comma,
    Colon,
    Semicolon,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Equal,
    DoubleEqual,
    NotEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    PlusEqual,
    MinusEqual,
    StarEqual,
    SlashEqual,
    Arrow,
    FatArrow,
    Ampersand,
    Pipe,
}