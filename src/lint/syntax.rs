use crate::common::{ErrorCode, ErrorKind, LeoError, LeoResult};
use crate::lexer::token::{Token, TokenWithSpan};

/// Syntax-level linter for token stream validation
pub struct SyntaxLinter;

impl SyntaxLinter {
    pub fn new() -> Self { Self }

    /// Validate balanced delimiters in token stream
    pub fn lint(tokens: &[TokenWithSpan]) -> LeoResult<Vec<LeoError>> {
        let mut errors = Vec::new();
        let mut brace = 0i32;
        let mut paren = 0i32;
        let mut bracket = 0i32;
        for tws in tokens {
            match &tws.token {
                Token::Symbol(crate::lexer::token::Symbol::LeftBrace) => brace += 1,
                Token::Symbol(crate::lexer::token::Symbol::RightBrace) => brace -= 1,
                Token::Symbol(crate::lexer::token::Symbol::LeftParen) => paren += 1,
                Token::Symbol(crate::lexer::token::Symbol::RightParen) => paren -= 1,
                Token::Symbol(crate::lexer::token::Symbol::LeftBracket) => bracket += 1,
                Token::Symbol(crate::lexer::token::Symbol::RightBracket) => bracket -= 1,
                _ => {}
            }
        }
        if brace != 0 {
            errors.push(LeoError::new(ErrorKind::Syntax, ErrorCode::ParserMissingToken,
                "unbalanced braces".into()));
        }
        if paren != 0 {
            errors.push(LeoError::new(ErrorKind::Syntax, ErrorCode::ParserMissingToken,
                "unbalanced parentheses".into()));
        }
        if bracket != 0 {
            errors.push(LeoError::new(ErrorKind::Syntax, ErrorCode::ParserMissingToken,
                "unbalanced brackets".into()));
        }
        Ok(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::span::Span;
    use crate::lexer::token::Symbol;

    #[test]
    fn test_balanced_tokens() {
        let tokens = vec![
            TokenWithSpan { token: Token::Symbol(Symbol::LeftBrace), span: Span::dummy() },
            TokenWithSpan { token: Token::Symbol(Symbol::RightBrace), span: Span::dummy() },
        ];
        let errors = SyntaxLinter::lint(&tokens).unwrap();
        assert!(errors.is_empty());
    }

    #[test]
    fn test_unbalanced_tokens() {
        let tokens = vec![
            TokenWithSpan { token: Token::Symbol(Symbol::LeftBrace), span: Span::dummy() },
        ];
        let errors = SyntaxLinter::lint(&tokens).unwrap();
        assert!(!errors.is_empty());
    }
}
