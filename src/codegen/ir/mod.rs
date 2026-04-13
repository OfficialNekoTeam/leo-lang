use crate::ast::expr::{BinOp, Expr, UnOp};
use crate::ast::stmt::Stmt;
use crate::common::{ErrorCode, ErrorKind, LeoError, LeoResult};
use crate::llvm::context::LlvmContext;
use inkwell::attributes::AttributeLoc;
use inkwell::types::BasicTypeEnum;
use inkwell::values::BasicValueEnum;
use inkwell::AddressSpace;
use inkwell::IntPredicate;
use std::collections::{HashMap, HashSet};

mod control;
mod core;
mod expr;
mod print;
mod string;
mod tests;
mod types;
mod vec_;

/// IR builder that walks AST and emits LLVM IR
pub struct IrBuilder {
    pub(super) array_sizes: HashMap<String, u32>,
    pub(super) string_vars: HashSet<String>,
    pub(super) tmp_counter: u64,
}

impl IrBuilder {
    pub fn new() -> Self {
        Self {
            array_sizes: HashMap::new(),
            string_vars: HashSet::new(),
            tmp_counter: 0,
        }
    }

    /// Build LLVM IR from statements
    pub fn build(&mut self, stmts: &[Stmt], ctx: &mut LlvmContext) -> LeoResult<()> {
        self.array_sizes.clear();
        self.string_vars.clear();
        self.declare_c_runtime(ctx);
        for stmt in stmts {
            self.build_stmt(stmt, ctx)?;
        }
        Ok(())
    }
}
