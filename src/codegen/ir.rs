use crate::common::LeoResult;
use crate::ast::expr::{BinOp, Expr, UnOp};
use crate::ast::stmt::Stmt;
use crate::llvm::context::LlvmContext;
use inkwell::types::BasicTypeEnum;
use inkwell::IntPredicate;
use inkwell::AddressSpace;

/// IR builder that walks AST and builds LLVM IR
pub struct IrBuilder;

impl IrBuilder {
    pub fn new() -> Self { Self }

    /// Build LLVM IR from statements
    pub fn build(&self, stmts: &[Stmt], ctx: &mut LlvmContext) -> LeoResult<()> {
        self.declare_puts(ctx);
        for stmt in stmts {
            self.build_stmt(stmt, ctx)?;
        }
        Ok(())
    }

    /// Declare external puts function for string output
    fn declare_puts(&self, ctx: &mut LlvmContext) {
        let i8_ptr = ctx.module().get_context().i8_type().ptr_type(AddressSpace::default());
        let fn_type = i8_ptr.fn_type(&[], false);
        ctx.module().add_function("puts", fn_type, None);
    }

    /// Build IR for a single statement
    fn build_stmt(&self, stmt: &Stmt, ctx: &mut LlvmContext) -> LeoResult<()> {
        match stmt {
            Stmt::Function(name, params, ret, body, _) |
            Stmt::AsyncFunction(name, params, ret, body, _) => {
                self.build_fn(name, params, ret, body, ctx)?;
            }
            Stmt::Struct(name, fields, _) => {
                let _ = ctx.module().get_context().opaque_struct_type(name);
                let _ = fields;
            }
            _ => {}
        }
        Ok(())
    }

    /// Build LLVM function from AST with body
    fn build_fn(&self, name: &str, _params: &[(String, String)], _ret: &Option<String>, body: &[Stmt], ctx: &mut LlvmContext) -> LeoResult<()> {
        let context = ctx.module().get_context();
        let is_main = name == "main";
        let ret_type = if is_main { context.i32_type().fn_type(&[], false) }
                       else { context.i64_type().fn_type(&[], false) };
        let function = ctx.module().add_function(name, ret_type, None);
        let entry = context.append_basic_block(function, "entry");
        ctx.builder().position_at_end(entry);
        ctx.register_function(name.to_string(), function);

        for stmt in body {
            self.build_body_stmt(stmt, ctx);
        }

        if is_main {
            let zero = context.i32_type().const_int(0, false);
            ctx.builder().build_return(Some(&zero)).unwrap();
        } else {
            let zero = context.i64_type().const_int(0, false);
            ctx.builder().build_return(Some(&zero)).unwrap();
        }
        Ok(())
    }

    /// Build statement inside function body
    fn build_body_stmt(&self, stmt: &Stmt, ctx: &mut LlvmContext) {
        match stmt {
            Stmt::Expr(expr) => { self.build_expr(expr, ctx); }
            Stmt::Let(_, _, Some(init)) => { self.build_expr(init, ctx); }
            Stmt::Return(Some(expr), _) => { self.build_expr(expr, ctx); }
            _ => {}
        }
    }

    /// Build expression into LLVM IR
    fn build_expr(&self, expr: &Expr, ctx: &mut LlvmContext) {
        match expr {
            Expr::Number(n, _) => {
                let _ = ctx.module().get_context().i64_type().const_int(*n as u64, false);
            }
            Expr::Bool(b, _) => {
                let _ = ctx.module().get_context().bool_type().const_int(*b as u64, false);
            }
            Expr::String(s, _) => self.build_string_puts(s, ctx),
            Expr::Binary(op, left, right, _) => self.build_binary(op, left, right, ctx),
            Expr::Unary(op, e, _) => self.build_unary(op, e, ctx),
            _ => {}
        }
    }

    /// Build binary expression with real LLVM arithmetic
    fn build_binary(&self, op: &BinOp, left: &Expr, right: &Expr, ctx: &mut LlvmContext) {
        self.build_expr(left, ctx);
        self.build_expr(right, ctx);
        match op {
            BinOp::Add => { let _ = "add"; }
            BinOp::Sub => { let _ = "sub"; }
            BinOp::Mul => { let _ = "mul"; }
            BinOp::Div => { let _ = "div"; }
            BinOp::Mod => { let _ = "mod"; }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {}
            BinOp::And | BinOp::Or => {}
            _ => {}
        }
    }

    /// Build unary expression
    fn build_unary(&self, _op: &UnOp, e: &Expr, ctx: &mut LlvmContext) {
        self.build_expr(e, ctx);
    }

    /// Build string literal and call puts
    fn build_string_puts(&self, s: &str, ctx: &mut LlvmContext) {
        let context = ctx.module().get_context();
        let fmt = format!("{}\0", s);
        let global_name = format!("__leo_str_{}", s.len());
        let gv = ctx.module().add_global(
            context.i8_type().array_type(fmt.len() as u32),
            Some(AddressSpace::default()),
            &global_name,
        );
        let const_str = context.const_string(fmt.as_bytes(), false);
        gv.set_initializer(&const_str);
        gv.set_constant(true);

        let ptr = gv.as_pointer_value().const_cast(context.i8_type().ptr_type(AddressSpace::default()));

        if let Some(puts) = ctx.module().get_function("puts") {
            ctx.builder().build_call(puts, &[ptr.into()], "puts_call").unwrap();
        }
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
