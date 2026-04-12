use crate::common::{ErrorCode, ErrorKind, LeoError, LeoResult};
use crate::ast::expr::{BinOp, Expr, UnOp};
use crate::ast::stmt::Stmt;
use crate::llvm::context::LlvmContext;
use inkwell::types::BasicTypeEnum;
use inkwell::IntPredicate;
use inkwell::AddressSpace;

/// IR builder that walks AST and emits LLVM IR
pub struct IrBuilder;

impl IrBuilder {
    pub fn new() -> Self { Self }

    /// Build LLVM IR from statements
    pub fn build(&self, stmts: &[Stmt], ctx: &mut LlvmContext) -> LeoResult<()> {
        self.declare_c_runtime(ctx);
        for stmt in stmts {
            self.build_stmt(stmt, ctx)?;
        }
        Ok(())
    }

    /// Declare external C runtime functions
    fn declare_c_runtime(&self, ctx: &mut LlvmContext) {
        let i8_ptr = ctx.module().get_context().i8_type().ptr_type(AddressSpace::default());
        let i64_type = ctx.module().get_context().i64_type();
        ctx.module().add_function("puts", i8_ptr.fn_type(&[], false), None);
        ctx.module().add_function("printf", ctx.module().get_context().i32_type().fn_type(&[i8_ptr.into(), i64_type.into()], true), None);
    }

    /// Build top-level statement
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

    /// Build LLVM function with body
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
            self.build_body_stmt(stmt, ctx)?;
        }

        if is_main {
            let zero = context.i32_type().const_int(0, false);
            ctx.builder().build_return(Some(&zero))
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "return failed".into()))?;
        } else {
            let zero = context.i64_type().const_int(0, false);
            ctx.builder().build_return(Some(&zero))
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "return failed".into()))?;
        }
        Ok(())
    }

    /// Build statement inside function body
    fn build_body_stmt(&self, stmt: &Stmt, ctx: &mut LlvmContext) -> LeoResult<()> {
        match stmt {
            Stmt::Expr(expr) => { let _ = self.eval_and_emit(expr, ctx)?; }
            Stmt::Let(_name, _ty, Some(init)) => { let _ = self.eval_and_emit(init, ctx)?; }
            Stmt::Return(Some(expr), _) => { let _ = self.eval_and_emit(expr, ctx)?; }
            _ => {}
        }
        Ok(())
    }

    /// Evaluate expression and emit output code (print)
    fn eval_and_emit(&self, expr: &Expr, ctx: &mut LlvmContext) -> LeoResult<()> {
        match expr {
            Expr::String(s, _) => self.emit_puts(s, ctx),
            Expr::Number(_, _) | Expr::Bool(_, _) | Expr::Binary(_, _, _, _) | Expr::Unary(_, _, _) => {
                let val = self.eval_int(expr, ctx)?;
                self.emit_print_int(val, ctx);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Evaluate integer expression
    fn eval_int<'a>(&self, expr: &Expr, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        match expr {
            Expr::Number(n, _) => Ok(ctx.module().get_context().i64_type().const_int(*n as u64, false)),
            Expr::Bool(b, _) => Ok(ctx.module().get_context().i64_type().const_int(*b as u64, false)),
            Expr::Binary(op, left, right, _) => {
                let lv = self.eval_int(left, ctx)?;
                let rv = self.eval_int(right, ctx)?;
                self.emit_binop(op, lv, rv, ctx)
            }
            Expr::Unary(op, e, _) => {
                let val = self.eval_int(e, ctx)?;
                self.emit_unop(op, val, ctx)
            }
            _ => Ok(ctx.module().get_context().i64_type().const_int(0, false)),
        }
    }

    /// Emit binary arithmetic/comparison
    fn emit_binop<'a>(&self, op: &BinOp, lv: inkwell::values::IntValue<'a>, rv: inkwell::values::IntValue<'a>, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        match op {
            BinOp::Add => ctx.builder().build_int_add(lv, rv, "add"),
            BinOp::Sub => ctx.builder().build_int_sub(lv, rv, "sub"),
            BinOp::Mul => ctx.builder().build_int_mul(lv, rv, "mul"),
            BinOp::Div => ctx.builder().build_int_signed_div(lv, rv, "div"),
            BinOp::Mod => ctx.builder().build_int_signed_rem(lv, rv, "rem"),
            BinOp::Eq => ctx.builder().build_int_compare(IntPredicate::EQ, lv, rv, "eq"),
            BinOp::Ne => ctx.builder().build_int_compare(IntPredicate::NE, lv, rv, "ne"),
            BinOp::Lt => ctx.builder().build_int_compare(IntPredicate::SLT, lv, rv, "lt"),
            BinOp::Le => ctx.builder().build_int_compare(IntPredicate::SLE, lv, rv, "le"),
            BinOp::Gt => ctx.builder().build_int_compare(IntPredicate::SGT, lv, rv, "gt"),
            BinOp::Ge => ctx.builder().build_int_compare(IntPredicate::SGE, lv, rv, "ge"),
            BinOp::And => ctx.builder().build_and(lv, rv, "and"),
            BinOp::Or => ctx.builder().build_or(lv, rv, "or"),
            _ => return Ok(lv),
        }.map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("{:?} failed", op)))
    }

    /// Emit unary operation
    fn emit_unop<'a>(&self, op: &UnOp, val: inkwell::values::IntValue<'a>, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        match op {
            UnOp::Neg | UnOp::Minus => {
                let zero = ctx.module().get_context().i64_type().const_int(0, false);
                ctx.builder().build_int_sub(zero, val, "neg")
            }
            UnOp::Not => {
                let ones = ctx.module().get_context().i64_type().const_int(u64::MAX, true);
                ctx.builder().build_xor(val, ones, "not")
            }
            _ => return Ok(val),
        }.map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("{:?} failed", op)))
    }

    /// Emit printf to print an i64
    fn emit_print_int<'a>(&self, val: inkwell::values::IntValue<'a>, ctx: &mut LlvmContext<'a>) {
        let context = ctx.module().get_context();
        let fmt = format!("%ld\n\0");
        let gv = ctx.module().add_global(
            context.i8_type().array_type(fmt.len() as u32),
            Some(AddressSpace::default()),
            "__leo_fmt_int",
        );
        gv.set_initializer(&context.const_string(fmt.as_bytes(), false));
        gv.set_constant(true);
        let ptr = gv.as_pointer_value()
            .const_cast(context.i8_type().ptr_type(AddressSpace::default()));
        if let Some(printf) = ctx.module().get_function("printf") {
            ctx.builder().build_call(printf, &[ptr.into(), val.into()], "print_int").ok();
        }
    }

    /// Emit puts for string
    fn emit_puts(&self, s: &str, ctx: &mut LlvmContext) -> LeoResult<()> {
        let context = ctx.module().get_context();
        let fmt = format!("{}\0", s);
        let gv = ctx.module().add_global(
            context.i8_type().array_type(fmt.len() as u32),
            Some(AddressSpace::default()),
            &format!("__leo_str_{}", s.len()),
        );
        gv.set_initializer(&context.const_string(fmt.as_bytes(), false));
        gv.set_constant(true);
        let ptr = gv.as_pointer_value()
            .const_cast(context.i8_type().ptr_type(AddressSpace::default()));
        if let Some(puts) = ctx.module().get_function("puts") {
            ctx.builder().build_call(puts, &[ptr.into()], "puts_call")
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "puts failed".into()))?;
        }
        Ok(())
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
        assert!(builder.build(&[], &mut ctx).is_ok());
    }
}
