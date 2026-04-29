use crate::common::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Identifier(String),
    Number(i64),
    TypedNumber(u128, String),
    Float(f64),
    TypedFloat(f64, String),
    String(String),
    /// Single character literal, e.g. 'a'
    Char(char),
    Keyword(Keyword),
    Symbol(Symbol),
    Comment(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenWithSpan {
    pub token: Token,
    pub span: Span,
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
    Match,
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
    As,
    Type,
    Self_,
    True,
    False,
    Break,
    Continue,
    Const,
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
    DoubleAmpersand,
    Pipe,
    DoublePipe,
    Bang,
    BangEqual,
    DoubleColon,
    Dot,
    DoubleDot,
    Question,
    QuestionQuestion,
    LeftShift,
    RightShift,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_with_span() {
        let span = Span::new(
            crate::common::span::Pos::new(1, 1, 0),
            crate::common::span::Pos::new(1, 3, 2),
        );
        let tws = TokenWithSpan {
            token: Token::Identifier("foo".to_string()),
            span,
        };
        assert_eq!(tws.token, Token::Identifier("foo".to_string()));
    }
}
