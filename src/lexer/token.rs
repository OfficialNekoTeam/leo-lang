use crate::common::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Identifier(String),
    Number(i64),
    Float(f64),
    String(String),
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
    Pipe,
    Bang,
    BangEqual,
    DoubleColon,
    Dot,
    DoubleDot,
    Question,
    QuestionQuestion,
}

impl Token {
    pub fn is_keyword(&self) -> bool {
        matches!(self, Token::Keyword(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_keyword() {
        let token = Token::Keyword(Keyword::Fn);
        assert!(token.is_keyword());
    }

    #[test]
    fn test_token_not_keyword() {
        let token = Token::Identifier("foo".to_string());
        assert!(!token.is_keyword());
    }

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