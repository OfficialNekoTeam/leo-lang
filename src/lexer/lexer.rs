use crate::common::{ErrorCode, ErrorKind, LeoError, LeoResult, Pos, Span};
use crate::lexer::token::{Keyword, Symbol, Token, TokenWithSpan};

pub struct Lexer {
    source: String,
    pos: usize,
    line: u32,
    column: u32,
    tokens: Vec<TokenWithSpan>,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            pos: 0,
            line: 1,
            column: 1,
            tokens: Vec::new(),
        }
    }

    pub fn tokenize(&mut self) -> LeoResult<Vec<TokenWithSpan>> {
        self.tokens.clear();
        while !self.is_eof() {
            self.skip_whitespace();
            if self.is_eof() {
                break;
            }
            let tok = self.next_token()?;
            self.tokens.push(tok);
        }
        Ok(std::mem::take(&mut self.tokens))
    }

    fn next_token(&mut self) -> LeoResult<TokenWithSpan> {
        let ch = self.current_char();
        let start = self.mark_pos();
        
        if ch.is_ascii_alphabetic() || ch == '_' {
            return self.scan_identifier();
        }
        if ch.is_ascii_digit() {
            return self.scan_number();
        }
        if ch == '"' {
            return self.scan_string();
        }
        if ch == '/' {
            return self.scan_comment();
        }
        
        let symbol = self.scan_symbol();
        if let Some(s) = symbol {
            let end = self.mark_pos();
            return Ok(TokenWithSpan {
                token: Token::Symbol(s),
                span: Span::new(start, end),
            });
        }
        
        let end = self.mark_pos();
        let msg = format!("unexpected character: {}", ch);
        Err(LeoError::new(ErrorKind::Syntax, ErrorCode::LexerUnexpectedChar, msg)
            .with_span(Span::new(start, end)))
    }

    fn scan_identifier(&mut self) -> LeoResult<TokenWithSpan> {
        let start = self.mark_pos();
        let mut buf = String::new();
        while !self.is_eof() {
            let ch = self.current_char();
            if ch.is_ascii_alphanumeric() || ch == '_' {
                buf.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        let end = self.mark_pos();
        
        let token = if let Some(kw) = self.keyword(&buf) {
            Token::Keyword(kw)
        } else {
            Token::Identifier(buf)
        };
        
        Ok(TokenWithSpan {
            token,
            span: Span::new(start, end),
        })
    }

    fn scan_number(&mut self) -> LeoResult<TokenWithSpan> {
        let start = self.mark_pos();
        let mut buf = String::new();
        let mut is_float = false;
        
        while !self.is_eof() {
            let ch = self.current_char();
            if ch.is_ascii_digit() {
                buf.push(ch);
                self.advance();
            } else if ch == '.' && !is_float {
                let next = self.peek_char();
                if next.is_ascii_digit() {
                    is_float = true;
                    buf.push(ch);
                    self.advance();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        let end = self.mark_pos();
        
        let token = if is_float {
            Token::Float(buf.parse().unwrap_or(0.0))
        } else {
            Token::Number(buf.parse().unwrap_or(0))
        };
        
        Ok(TokenWithSpan {
            token,
            span: Span::new(start, end),
        })
    }

    fn scan_string(&mut self) -> LeoResult<TokenWithSpan> {
        let start = self.mark_pos();
        self.advance(); // skip opening "
        let mut buf = String::new();
        
        while !self.is_eof() {
            let ch = self.current_char();
            if ch == '"' {
                self.advance();
                let end = self.mark_pos();
                return Ok(TokenWithSpan {
                    token: Token::String(buf),
                    span: Span::new(start, end),
                });
            }
            if ch == '\\' {
                self.advance();
                if !self.is_eof() {
                    let ch = self.current_char();
                    match ch {
                        'n' => buf.push('\n'),
                        't' => buf.push('\t'),
                        'r' => buf.push('\r'),
                        '\\' => buf.push('\\'),
                        '"' => buf.push('"'),
                        _ => buf.push(ch),
                    }
                    self.advance();
                }
            } else {
                buf.push(ch);
                self.advance();
            }
        }
        
        let end = self.mark_pos();
        let msg = "unterminated string".to_string();
        Err(LeoError::new(ErrorKind::Syntax, ErrorCode::LexerUnterminatedString, msg)
            .with_span(Span::new(start, end)))
    }

    fn scan_comment(&mut self) -> LeoResult<TokenWithSpan> {
        let start = self.mark_pos();
        self.advance(); // skip /
        
        if self.current_char() == '/' {
            self.advance();
            let mut buf = String::new();
            while !self.is_eof() && self.current_char() != '\n' {
                buf.push(self.current_char());
                self.advance();
            }
            let end = self.mark_pos();
            return Ok(TokenWithSpan {
                token: Token::Comment(buf),
                span: Span::new(start, end),
            });
        }
        
        if self.current_char() == '*' {
            self.advance();
            let mut buf = String::new();
            loop {
                if self.is_eof() {
                    let end = self.mark_pos();
                    let msg = "unterminated block comment".to_string();
                    return Err(LeoError::new(ErrorKind::Syntax, ErrorCode::LexerUnterminatedString, msg)
                        .with_span(Span::new(start, end)));
                }
                if self.current_char() == '*' && self.peek_char() == '/' {
                    self.advance();
                    self.advance();
                    let end = self.mark_pos();
                    return Ok(TokenWithSpan {
                        token: Token::Comment(buf),
                        span: Span::new(start, end),
                    });
                }
                buf.push(self.current_char());
                self.advance();
            }
        }
        
        let end = self.mark_pos();
        Ok(TokenWithSpan {
            token: Token::Symbol(Symbol::Slash),
            span: Span::new(start, end),
        })
    }

    fn scan_symbol(&mut self) -> Option<Symbol> {
        let ch = self.current_char();
        let next = self.peek_char();
        
        match (ch, next) {
            ('{', _) => { self.advance(); Some(Symbol::LeftBrace) },
            ('}', _) => { self.advance(); Some(Symbol::RightBrace) },
            ('(', _) => { self.advance(); Some(Symbol::LeftParen) },
            (')', _) => { self.advance(); Some(Symbol::RightParen) },
            ('[', _) => { self.advance(); Some(Symbol::LeftBracket) },
            (']', _) => { self.advance(); Some(Symbol::RightBracket) },
            (',', _) => { self.advance(); Some(Symbol::Comma) },
            (':', ':') => { self.advance(); self.advance(); Some(Symbol::DoubleColon) },
            (':', _) => { self.advance(); Some(Symbol::Colon) },
            (';', _) => { self.advance(); Some(Symbol::Semicolon) },
            ('+', '=') => { self.advance(); self.advance(); Some(Symbol::PlusEqual) },
            ('+', _) => { self.advance(); Some(Symbol::Plus) },
            ('-', '=') => { self.advance(); self.advance(); Some(Symbol::MinusEqual) },
            ('-', '>') => { self.advance(); self.advance(); Some(Symbol::Arrow) },
            ('-', _) => { self.advance(); Some(Symbol::Minus) },
            ('*', '=') => { self.advance(); self.advance(); Some(Symbol::StarEqual) },
            ('*', _) => { self.advance(); Some(Symbol::Star) },
            ('/', '=') => { self.advance(); self.advance(); Some(Symbol::SlashEqual) },
            ('/', _) => { self.advance(); Some(Symbol::Slash) },
            ('%', _) => { self.advance(); Some(Symbol::Percent) },
            ('=', '=') => { self.advance(); self.advance(); Some(Symbol::DoubleEqual) },
            ('=', '>') => { self.advance(); self.advance(); Some(Symbol::FatArrow) },
            ('=', _) => { self.advance(); Some(Symbol::Equal) },
            ('!', '=') => { self.advance(); self.advance(); Some(Symbol::BangEqual) },
            ('!', _) => { self.advance(); Some(Symbol::Bang) },
            ('>', '=') => { self.advance(); self.advance(); Some(Symbol::GreaterEqual) },
            ('>', _) => { self.advance(); Some(Symbol::Greater) },
            ('<', '=') => { self.advance(); self.advance(); Some(Symbol::LessEqual) },
            ('<', _) => { self.advance(); Some(Symbol::Less) },
            ('&', _) => { self.advance(); Some(Symbol::Ampersand) },
            ('|', _) => { self.advance(); Some(Symbol::Pipe) },
            ('.', '.') => { self.advance(); self.advance(); Some(Symbol::DoubleDot) },
            ('.', _) => { self.advance(); Some(Symbol::Dot) },
            ('?', '?') => { self.advance(); self.advance(); Some(Symbol::QuestionQuestion) },
            ('?', _) => { self.advance(); Some(Symbol::Question) },
            _ => None,
        }
    }

    fn keyword(&self, s: &str) -> Option<Keyword> {
        match s {
            "fn" => Some(Keyword::Fn),
            "let" => Some(Keyword::Let),
            "mut" => Some(Keyword::Mut),
            "if" => Some(Keyword::If),
            "else" => Some(Keyword::Else),
            "for" => Some(Keyword::For),
            "while" => Some(Keyword::While),
            "match" => Some(Keyword::Match),
            "return" => Some(Keyword::Return),
            "import" => Some(Keyword::Import),
            "from" => Some(Keyword::From),
            "async" => Some(Keyword::Async),
            "await" => Some(Keyword::Await),
            "spawn" => Some(Keyword::Spawn),
            "pub" => Some(Keyword::Pub),
            "struct" => Some(Keyword::Struct),
            "enum" => Some(Keyword::Enum),
            "trait" => Some(Keyword::Trait),
            "impl" => Some(Keyword::Impl),
            "module" => Some(Keyword::Module),
            "as" => Some(Keyword::As),
            "type" => Some(Keyword::Type),
            "self" => Some(Keyword::Self_),
            "true" => Some(Keyword::True),
            "false" => Some(Keyword::False),
            "break" => Some(Keyword::Break),
            "continue" => Some(Keyword::Continue),
            _ => None,
        }
    }

    fn skip_whitespace(&mut self) {
        while !self.is_eof() {
            let ch = self.current_char();
            if ch.is_whitespace() {
                if ch == '\n' {
                    self.line += 1;
                    self.column = 1;
                }
                self.advance();
            } else {
                break;
            }
        }
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.source.len()
    }

    fn current_char(&self) -> char {
        self.source.chars().nth(self.pos).unwrap_or('\0')
    }

    fn peek_char(&self) -> char {
        self.source.chars().nth(self.pos + 1).unwrap_or('\0')
    }

    fn advance(&mut self) {
        if !self.is_eof() {
            self.pos += 1;
            self.column += 1;
        }
    }

    fn mark_pos(&self) -> Pos {
        Pos::new(self.line, self.column, self.pos as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lexer_identifier() {
        let mut lexer = Lexer::new("foo");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].token, Token::Identifier(_)));
    }

    #[test]
    fn test_lexer_number() {
        let mut lexer = Lexer::new("42");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].token, Token::Number(42)));
    }

    #[test]
    fn test_lexer_float() {
        let mut lexer = Lexer::new("3.14");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].token, Token::Float(_)));
    }

    #[test]
    fn test_lexer_keyword() {
        let mut lexer = Lexer::new("let x = 1");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(tokens[0].token, Token::Keyword(Keyword::Let)));
    }

    #[test]
    fn test_lexer_string() {
        let mut lexer = Lexer::new("\"hello\"");
        let tokens = lexer.tokenize().unwrap();
        assert!(matches!(&tokens[0].token, Token::String(s) if s == "hello"));
    }

    #[test]
    fn test_lexer_symbols() {
        let mut lexer = Lexer::new("+=");
        let tokens = lexer.tokenize().unwrap();
        let first = &tokens[0].token;
        assert!(matches!(first, Token::Symbol(Symbol::PlusEqual)));
    }
}