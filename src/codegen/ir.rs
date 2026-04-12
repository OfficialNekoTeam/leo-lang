use crate::common::LeoResult;
use crate::ast::stmt::Stmt;
use crate::llvm::context::LlvmContext;
use inkwell::types::BasicTypeEnum;

/// IR builder that walks AST and builds LLVM IR
pub struct IrBuilder;

impl IrBuilder {
    pub fn new() -> Self { Self }

    /// Build LLVM IR from statements
    pub fn build(&self, stmts: &[Stmt], ctx: &mut LlvmContext) -> LeoResult<()> {
        for stmt in stmts {
            self.build_stmt(stmt, ctx)?;
        }
        Ok(())
    }

    /// Build IR for a single statement
    fn build_stmt(&self, stmt: &Stmt, ctx: &mut LlvmContext) -> LeoResult<()> {
        match stmt {
            Stmt::Function(name, params, ret, body, _) |
            Stmt::AsyncFunction(name, params, ret, body, _) => {
                self.build_fn(name, params, ret, body, ctx)?;
            }
            Stmt::Struct(name, fields, _) => {
                self.build_struct(name, fields, ctx);
            }
            _ => {}
        }
        Ok(())
    }

    /// Build LLVM function from AST
    fn build_fn(&self, name: &str, _params: &[(String, String)], _ret: &Option<String>, _body: &[Stmt], ctx: &mut LlvmContext) -> LeoResult<()> {
        let fn_type = ctx.module().get_context().f64_type().fn_type(&[], false);
        let function = ctx.module().add_function(name, fn_type, None);
        ctx.register_function(name.to_string(), function);
        Ok(())
    }

    /// Build LLVM struct type
    fn build_struct(&self, name: &str, fields: &[(String, String)], ctx: &mut LlvmContext) {
        let _struct_type = ctx.module().get_context().opaque_struct_type(name);
        let _ = fields;
    }

    /// Map Leo type string to LLVM type
    pub fn llvm_type<'ctx>(ty: &str, ctx: &LlvmContext<'ctx>) -> BasicTypeEnum<'ctx> {
        let context = ctx.module().get_context();
        match ty {
            "i32" => context.i32_type().into(),
            "i64" => context.i64_type().into(),
            "f64" => context.f64_type().into(),
            "bool" => context.bool_type().into(),
            _ => context.i64_type().into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::context::Context;

    #[test]
    fn test_ir_builder_new() {
        let builder = IrBuilder::new();
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test");
        let result = builder.build(&[], &mut ctx);
        assert!(result.is_ok());
    }
}
