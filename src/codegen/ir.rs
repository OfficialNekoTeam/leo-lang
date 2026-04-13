use crate::common::{ErrorCode, ErrorKind, LeoError, LeoResult};
use crate::ast::expr::{BinOp, Expr, UnOp};
use crate::ast::stmt::Stmt;
use crate::llvm::context::LlvmContext;
use inkwell::types::BasicTypeEnum;
use inkwell::values::BasicValueEnum;
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

    /// Declare external C runtime functions (puts, printf)
    fn declare_c_runtime(&self, ctx: &mut LlvmContext) {
        let i8_ptr = ctx.module().get_context().i8_type().ptr_type(AddressSpace::default());
        let i64_type = ctx.module().get_context().i64_type();
        let i32_type = ctx.module().get_context().i32_type();
        ctx.module_mut().add_function("puts", i8_ptr.fn_type(&[], false), None);
        ctx.module_mut().add_function("printf", i32_type.fn_type(&[i8_ptr.into(), i64_type.into()], true), None);
    }

    /// Build top-level statement (function definitions, struct declarations)
    fn build_stmt(&self, stmt: &Stmt, ctx: &mut LlvmContext) -> LeoResult<()> {
        match stmt {
            Stmt::Function(name, params, ret, body, _) |
            Stmt::AsyncFunction(name, params, ret, body, _) => {
                self.build_fn(name, params, ret, body, ctx)?;
            }
            Stmt::Struct(name, fields, _) => {
                let struct_type = ctx.module().get_context().opaque_struct_type(name);
                let _ = struct_type;
                let _ = fields;
            }
            _ => {}
        }
        Ok(())
    }

    /// Build LLVM function with params, return type, and body
    fn build_fn(&self, name: &str, params: &[(String, String)], ret: &Option<String>, body: &[Stmt], ctx: &mut LlvmContext) -> LeoResult<()> {
        ctx.clear_variables();
        let context = ctx.module().get_context();
        let is_main = name == "main";

        let param_types: Vec<BasicTypeEnum> = params.iter()
            .map(|(_, ty)| Self::llvm_type(ty, ctx))
            .collect();
        let param_meta: Vec<_> = param_types.iter().map(|t| (*t).into()).collect();

        let fn_type = if is_main {
            context.i32_type().fn_type(&param_meta, false)
        } else {
            match ret.as_deref() {
                Some("i32") => context.i32_type().fn_type(&param_meta, false),
                Some("bool") => context.bool_type().fn_type(&param_meta, false),
                _ => context.i64_type().fn_type(&param_meta, false),
            }
        };

        let function = ctx.module_mut().add_function(name, fn_type, None);
        let entry = context.append_basic_block(function, "entry");
        ctx.builder().position_at_end(entry);
        ctx.register_function(name.to_string(), function);
        ctx.set_current_fn(function);

        // Alloca + store each parameter as a local variable
        for (i, (pname, _pty)) in params.iter().enumerate() {
            let ptr = ctx.builder().build_alloca(param_types[i], pname)
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("alloca param {} failed", pname)))?;
            let param_val = function.get_nth_param(i as u32)
                .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("param {} not found", pname)))?;
            ctx.builder().build_store(ptr, param_val)
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("store param {} failed", pname)))?;
            ctx.register_variable(pname.clone(), ptr);
        }

        for stmt in body {
            self.build_body_stmt(stmt, ctx)?;
        }

        // Default return (fallback if no explicit return)
        if !Self::block_is_terminated(ctx) {
            if is_main {
                let zero = context.i32_type().const_int(0, false);
                ctx.builder().build_return(Some(&zero))
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "return failed".into()))?;
            } else {
                let zero = context.i64_type().const_int(0, false);
                ctx.builder().build_return(Some(&zero))
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "return failed".into()))?;
            }
        }
        Ok(())
    }

    /// Build statement inside function body (let, assign, while, if, expr, return)
    fn build_body_stmt(&self, stmt: &Stmt, ctx: &mut LlvmContext) -> LeoResult<()> {
        match stmt {
            Stmt::Let(name, ty, init) => {
                self.build_let(name, ty, init, ctx)?;
            }
            Stmt::Assign(name, expr) => {
                self.build_assign(name, expr, ctx)?;
            }
            Stmt::While(cond, body, _span) => {
                self.build_while(cond, body, ctx)?;
            }
            Stmt::If(branches, else_body, _span) => {
                self.build_if(branches, else_body, ctx)?;
            }
            Stmt::Return(Some(expr), _) => {
                let val = self.eval_expr_to_value(expr, ctx)?;
                self.build_return_with(val, ctx)?;
            }
            Stmt::Return(None, _) => {
                let context = ctx.module().get_context();
                let zero = context.i32_type().const_int(0, false);
                ctx.builder().build_return(Some(&zero))
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "return failed".into()))?;
            }
            Stmt::Expr(expr) => {
                self.eval_and_emit(expr, ctx)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Build let binding: alloca on stack, store initial value
    fn build_let(&self, name: &str, ty: &Option<String>, init: &Option<Expr>, ctx: &mut LlvmContext) -> LeoResult<()> {
        let type_str = ty.as_deref().unwrap_or("i64");
        let llvm_type = Self::llvm_type(type_str, ctx);
        let ptr = ctx.builder().build_alloca(llvm_type, name)
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("alloca failed for {}", name)))?;
        ctx.register_variable(name.to_string(), ptr);

        if let Some(expr) = init {
            let val = self.eval_expr_to_value(expr, ctx)?;
            ctx.builder().build_store(ptr, val)
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("store failed for {}", name)))?;
        }
        Ok(())
    }

    /// Build assignment: load value, store into existing variable
    fn build_assign(&self, name: &str, expr: &Expr, ctx: &mut LlvmContext) -> LeoResult<()> {
        let ptr = ctx.get_variable(name)
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("undefined variable: {}", name)))?;
        let val = self.eval_expr_to_value(expr, ctx)?;
        ctx.builder().build_store(ptr, val)
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("store failed for {}", name)))?;
        Ok(())
    }

    /// Build while loop: condition block → body block → merge block
    fn build_while(&self, cond: &Expr, body: &[Stmt], ctx: &mut LlvmContext) -> LeoResult<()> {
        let function = ctx.builder().get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "no function".into()))?;
        let context = ctx.module().get_context();

        let cond_block = context.append_basic_block(function, "while.cond");
        let body_block = context.append_basic_block(function, "while.body");
        let merge_block = context.append_basic_block(function, "while.merge");

        ctx.builder().build_unconditional_branch(cond_block)
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "branch failed".into()))?;

        ctx.builder().position_at_end(cond_block);
        let cond_val = self.eval_int(cond, ctx)?;
        let zero = context.i64_type().const_int(0, false);
        let cmp = ctx.builder().build_int_compare(IntPredicate::NE, cond_val, zero, "while.test")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "compare failed".into()))?;
        ctx.builder().build_conditional_branch(cmp, body_block, merge_block)
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "cond branch failed".into()))?;

        ctx.builder().position_at_end(body_block);
        for stmt in body {
            self.build_body_stmt(stmt, ctx)?;
        }
        self.emit_branch(cond_block, ctx)?;

        ctx.builder().position_at_end(merge_block);
        Ok(())
    }

    /// Build if/else-if/else chain using LLVM conditional branches
    fn build_if(&self, branches: &[(Expr, Vec<Stmt>)], else_body: &Option<Vec<Stmt>>, ctx: &mut LlvmContext) -> LeoResult<()> {
        let function = ctx.builder().get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "no function".into()))?;
        let context = ctx.module().get_context();

        let merge_block = context.append_basic_block(function, "if.merge");

        // Pre-create all condition and then blocks to avoid duplicates
        let cond_blocks: Vec<_> = (0..branches.len())
            .map(|i| context.append_basic_block(function, &format!("if.cond.{}", i)))
            .collect();
        let then_blocks: Vec<_> = (0..branches.len())
            .map(|i| context.append_basic_block(function, &format!("if.then.{}", i)))
            .collect();

        let else_block = if else_body.is_some() {
            Some(context.append_basic_block(function, "if.else"))
        } else {
            None
        };

        // Branch from current position to first condition
        ctx.builder().build_unconditional_branch(cond_blocks[0])
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "branch failed".into()))?;

        for (i, (cond, body)) in branches.iter().enumerate() {
            // Evaluate condition in cond_block
            ctx.builder().position_at_end(cond_blocks[i]);
            let cond_val = self.eval_int(cond, ctx)?;
            let zero = context.i64_type().const_int(0, false);
            let cmp = ctx.builder().build_int_compare(IntPredicate::NE, cond_val, zero, "if.test")
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "compare failed".into()))?;

            let false_block = if i + 1 < branches.len() {
                cond_blocks[i + 1]
            } else {
                else_block.unwrap_or(merge_block)
            };
            ctx.builder().build_conditional_branch(cmp, then_blocks[i], false_block)
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "cond branch failed".into()))?;

            // Build then body
            ctx.builder().position_at_end(then_blocks[i]);
            for stmt in body {
                self.build_body_stmt(stmt, ctx)?;
            }
            self.emit_branch(merge_block, ctx)?;
        }

        // Build else body
        if let (Some(else_stmts), Some(eb)) = (else_body, else_block) {
            ctx.builder().position_at_end(eb);
            for stmt in else_stmts {
                self.build_body_stmt(stmt, ctx)?;
            }
            self.emit_branch(merge_block, ctx)?;
        }

        ctx.builder().position_at_end(merge_block);
        Ok(())
    }

    /// Check if the current basic block already has a terminator
    fn block_is_terminated(ctx: &LlvmContext) -> bool {
        ctx.builder().get_insert_block()
            .map_or(true, |bb| bb.get_terminator().is_some())
    }

    /// Emit unconditional branch only if block has no terminator yet
    fn emit_branch(&self, target: inkwell::basic_block::BasicBlock, ctx: &mut LlvmContext) -> LeoResult<()> {
        if Self::block_is_terminated(ctx) { return Ok(()); }
        ctx.builder().build_unconditional_branch(target)
            .map(|_| ())
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "branch failed".into()))
    }
    fn eval_and_emit(&self, expr: &Expr, ctx: &mut LlvmContext) -> LeoResult<()> {
        match expr {
            Expr::String(s, _) => self.emit_puts(s, ctx),
            Expr::Ident(name, _) => {
                let ptr = ctx.get_variable(name)
                    .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("undefined variable: {}", name)))?;
                let val = ctx.builder().build_load(ptr, name)
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("load failed for {}", name)))?;
                self.emit_print_int(val.into_int_value(), ctx);
                Ok(())
            }
            _ => {
                let val = self.eval_int(expr, ctx)?;
                self.emit_print_int(val, ctx);
                Ok(())
            }
        }
    }

    /// Evaluate expression to an LLVM IntValue (handles ident load, literals, binary, unary)
    fn eval_expr_to_value<'a>(&self, expr: &Expr, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        match expr {
            Expr::Ident(name, _) => {
                let ptr = ctx.get_variable(name)
                    .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("undefined variable: {}", name)))?;
                ctx.builder().build_load(ptr, name)
                    .map(|v| v.into_int_value())
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("load failed for {}", name)))
            }
            _ => self.eval_int(expr, ctx),
        }
    }

    /// Evaluate integer expression (number, bool, binary, unary, call)
    fn eval_int<'a>(&self, expr: &Expr, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        match expr {
            Expr::Number(n, _) => Ok(ctx.module().get_context().i64_type().const_int(*n as u64, false)),
            Expr::Bool(b, _) => Ok(ctx.module().get_context().i64_type().const_int(*b as u64, false)),
            Expr::Ident(name, _) => {
                let ptr = ctx.get_variable(name)
                    .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("undefined variable: {}", name)))?;
                ctx.builder().build_load(ptr, name)
                    .map(|v| v.into_int_value())
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("load failed for {}", name)))
            }
            Expr::Binary(op, left, right, _) => {
                let lv = self.eval_int(left, ctx)?;
                let rv = self.eval_int(right, ctx)?;
                self.emit_binop(op, lv, rv, ctx)
            }
            Expr::Unary(op, e, _) => {
                let val = self.eval_int(e, ctx)?;
                self.emit_unop(op, val, ctx)
            }
            Expr::Call(callee, args, _) => self.eval_call(callee, args, ctx),
            _ => Ok(ctx.module().get_context().i64_type().const_int(0, false)),
        }
    }

    /// Evaluate function call: resolve callee, eval args, build_call
    fn eval_call<'a>(&self, callee: &Expr, args: &[Expr], ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        let func_name = match callee {
            Expr::Ident(name, _) => name.clone(),
            _ => return Err(LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "only direct function calls supported".into())),
        };
        let func = ctx.get_function(&func_name)
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("undefined function: {}", func_name)))?;

        let mut arg_values: Vec<_> = Vec::new();
        for arg in args {
            let val = self.eval_int(arg, ctx)?;
            arg_values.push(BasicValueEnum::from(val).into());
        }

        let call_site = ctx.builder().build_call(func, &arg_values, "call")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("call {} failed", func_name)))?;

        let ret = call_site.try_as_basic_value()
            .left()
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("{} returned void", func_name)))?;
        Ok(ret.into_int_value())
    }

    /// Build explicit return with value, respecting function return type
    fn build_return_with<'a>(&self, val: inkwell::values::IntValue<'a>, ctx: &mut LlvmContext<'a>) -> LeoResult<()> {
        let context = ctx.module().get_context();
        let ret_val: BasicValueEnum = if let Some(fv) = ctx.current_fn() {
            let fn_type = fv.get_type();
            match fn_type.get_return_type() {
                Some(BasicTypeEnum::IntType(int_ty)) if int_ty == context.i32_type() => {
                    let trunc = ctx.builder().build_int_truncate(val, context.i32_type(), "ret.trunc")
                        .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "truncate failed".into()))?;
                    trunc.into()
                }
                _ => val.into(),
            }
        } else {
            val.into()
        };
        ctx.builder().build_return(Some(&ret_val))
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "return failed".into()))?;
        Ok(())
    }

    /// Emit binary arithmetic/comparison/logic (z-extends comparison results to i64)
    fn emit_binop<'a>(&self, op: &BinOp, lv: inkwell::values::IntValue<'a>, rv: inkwell::values::IntValue<'a>, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        let i64_type = ctx.module().get_context().i64_type();
        match op {
            BinOp::Add => ctx.builder().build_int_add(lv, rv, "add"),
            BinOp::Sub => ctx.builder().build_int_sub(lv, rv, "sub"),
            BinOp::Mul => ctx.builder().build_int_mul(lv, rv, "mul"),
            BinOp::Div => ctx.builder().build_int_signed_div(lv, rv, "div"),
            BinOp::Mod => ctx.builder().build_int_signed_rem(lv, rv, "rem"),
            BinOp::Eq => ctx.builder().build_int_compare(IntPredicate::EQ, lv, rv, "eq")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "eq.ext")),
            BinOp::Ne => ctx.builder().build_int_compare(IntPredicate::NE, lv, rv, "ne")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "ne.ext")),
            BinOp::Lt => ctx.builder().build_int_compare(IntPredicate::SLT, lv, rv, "lt")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "lt.ext")),
            BinOp::Le => ctx.builder().build_int_compare(IntPredicate::SLE, lv, rv, "le")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "le.ext")),
            BinOp::Gt => ctx.builder().build_int_compare(IntPredicate::SGT, lv, rv, "gt")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "gt.ext")),
            BinOp::Ge => ctx.builder().build_int_compare(IntPredicate::SGE, lv, rv, "ge")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "ge.ext")),
            BinOp::And => ctx.builder().build_and(lv, rv, "and"),
            BinOp::Or => ctx.builder().build_or(lv, rv, "or"),
            _ => return Ok(lv),
        }.map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("{:?} failed", op)))
    }

    /// Emit unary operation (negate, bitwise not)
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

    /// Emit printf("%ld\n", val) to print an i64
    fn emit_print_int<'a>(&self, val: inkwell::values::IntValue<'a>, ctx: &mut LlvmContext<'a>) {
        let context = ctx.module().get_context();
        let fmt = format!("%ld\n\0");
        let gv = ctx.module_mut().add_global(
            context.i8_type().array_type(fmt.len() as u32),
            Some(AddressSpace::default()),
            &format!("__leo_fmt_int_{}", val),
        );
        gv.set_initializer(&context.const_string(fmt.as_bytes(), false));
        gv.set_constant(true);
        let ptr = gv.as_pointer_value()
            .const_cast(context.i8_type().ptr_type(AddressSpace::default()));
        if let Some(printf) = ctx.module().get_function("printf") {
            ctx.builder().build_call(printf, &[ptr.into(), val.into()], "print_int").ok();
        }
    }

    /// Emit puts(string) for string literal
    fn emit_puts(&self, s: &str, ctx: &mut LlvmContext) -> LeoResult<()> {
        let context = ctx.module().get_context();
        let fmt = format!("{}\0", s);
        let gv = ctx.module_mut().add_global(
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
