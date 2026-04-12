use crate::common::LeoResult;
use crate::ast::Stmt;

pub struct Checker;

impl Checker {
    pub fn new() -> Self {
        Self
    }

    pub fn check(&self, _stmts: &[Stmt]) -> LeoResult<()> {
        Ok(())
    }
}