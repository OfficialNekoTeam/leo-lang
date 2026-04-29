use crate::ast::expr::{BinOp, Expr};
use crate::ast::stmt::Stmt;
use crate::common::types::LeoType;
use crate::common::{ErrorCode, ErrorKind, LeoError, LeoResult};
use crate::llvm::context::LlvmContext;
use inkwell::types::BasicTypeEnum;
use inkwell::values::BasicValueEnum;
use inkwell::AddressSpace;
use inkwell::IntPredicate;
use std::collections::{HashMap, HashSet};

pub(super) const MAX_FILE_READ_BYTES: u64 = 16 * 1024 * 1024;
pub(super) const ERR_CANNOT_OPEN_FILE: &str = "runtime error: cannot open file\n";
pub(super) const ERR_FILE_READ_SIZE: &str = "runtime error: file_read size invalid\n";
pub(super) const ERR_FILE_READ_TOO_LARGE: &str = "runtime error: file_read too large\n";
pub(super) const ERR_OUT_OF_MEMORY: &str = "runtime error: out of memory\n";
pub(super) const ERR_PATH_TRAVERSAL: &str = "runtime error: path traversal blocked\n";

mod builtins;
mod control;
mod core;
mod enum_;
mod expr;
mod file_io;
mod func;
mod mono;
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
    pub(super) generic_fns: HashMap<String, mono::GenericFnDef>,
    pub(super) generic_structs: HashMap<String, mono::GenericStructDef>,
    /// H2/M8: tracks in-progress instantiations for cycle detection
    pub(super) instantiation_stack: HashSet<String>,
    /// H2/M8: current instantiation nesting depth
    pub(super) instantiation_depth: usize,
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
            generic_fns: HashMap::new(),
            generic_structs: HashMap::new(),
            instantiation_stack: HashSet::new(),
            instantiation_depth: 0,
        }
    }

    pub fn build(&mut self, stmts: &[Stmt], ctx: &mut LlvmContext) -> LeoResult<()> {
        self.array_sizes.clear();
        self.struct_fields.clear();
        self.struct_field_types.clear();
        self.var_types.clear();
        self.methods.clear();
        self.enum_payload_types.clear();
        self.generic_fns.clear();
        self.generic_structs.clear();
        self.instantiation_stack.clear();
        self.instantiation_depth = 0;
        self.declare_c_runtime(ctx);
        for stmt in stmts {
            self.build_stmt(stmt, ctx)?;
        }
        Ok(())
    }
}
