use crate::common::LeoResult;
use crate::lexer::token::Token;

pub struct Lexer;

impl Lexer {
    pub fn new(_source: &str) -> Self {
        Self
    }

    pub fn tokenize(&self) -> LeoResult<Vec<Token>> {
        Ok(vec![])
    }
}