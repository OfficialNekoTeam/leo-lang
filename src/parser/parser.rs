use crate::common::LeoResult;
use crate::lexer::token::Token;
use crate::ast::Stmt;

pub struct Parser;

impl Parser {
    pub fn new() -> Self {
        Self
    }

    pub fn parse(&self, _tokens: &[Token]) -> LeoResult<Vec<Stmt>> {
        Ok(vec![])
    }
}