use crate::ast::expr::{BinOp, Expr};
use crate::ast::stmt::Stmt;
use crate::common::types::LeoType;
use crate::common::{ErrorCode, ErrorKind, LeoError, LeoResult};
use crate::llvm::context::LlvmContext;
use inkwell::types::BasicTypeEnum;
use inkwell::values::BasicValueEnum;
use inkwell::AddressSpace;
use inkwell::IntPredicate;
use std::collections::HashMap;

mod builtins;
mod control;
mod core;
mod enum_;
mod expr;
mod file_io;
mod func;
mod ops;
mod print;
mod str_util;
mod string;
mod tests;
mod types;
mod vec_;

pub struct IrBuilder {
    pub(super) array_sizes: HashMap<String, u32>,
    pub(super) tmp_counter: u64,
    pub(super) struct_fields: HashMap<String, Vec<String>>,
    pub(super) struct_field_types: HashMap<String, Vec<String>>,
    pub(super) var_types: HashMap<String, String>,
    pub(super) methods: HashMap<(String, String), String>,
    pub(super) enum_payload_types: HashMap<String, Vec<String>>,
}

impl IrBuilder {
    pub fn new() -> Self {
        Self {
            array_sizes: HashMap::new(),
            tmp_counter: 0,
            struct_fields: HashMap::new(),
            struct_field_types: HashMap::new(),
            var_types: HashMap::new(),
            methods: HashMap::new(),
            enum_payload_types: HashMap::new(),
        }
    }

    pub fn build(&mut self, stmts: &[Stmt], ctx: &mut LlvmContext) -> LeoResult<()> {
        self.array_sizes.clear();
        self.struct_fields.clear();
        self.struct_field_types.clear();
        self.var_types.clear();
        self.methods.clear();
        self.enum_payload_types.clear();
        self.declare_c_runtime(ctx);
        for stmt in stmts {
            self.build_stmt(stmt, ctx)?;
        }
        Ok(())
    }
}
