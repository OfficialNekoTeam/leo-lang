use super::*;

impl IrBuilder {
    /// Builtin println(x): prints any basic type followed by newline
    pub(super) fn builtin_println<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.is_empty() {
            self.emit_puts("", ctx)?;
        } else {
            self.builtin_print_value(&args[0], ctx, true)?;
        }
        Ok(ctx.module().get_context().i64_type().const_int(0, false))
    }

    /// Builtin print(x): prints without newline
    pub(super) fn builtin_print<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if !args.is_empty() {
            self.builtin_print_value(&args[0], ctx, false)?;
        }
        Ok(ctx.module().get_context().i64_type().const_int(0, false))
    }

    /// Print a single value (integer or string), with or without newline
    pub(super) fn builtin_print_value(
        &mut self,
        expr: &Expr,
        ctx: &mut LlvmContext,
        newline: bool,
    ) -> LeoResult<()> {
        match expr {
            Expr::String(s, _) => {
                if newline {
                    self.emit_puts(s, ctx)?
                } else {
                    self.emit_print_str(s, ctx)?
                }
            }
            _ => {
                let val = self.eval_int(expr, ctx)?;
                if newline {
                    self.emit_print_int(val, ctx)
                } else {
                    self.emit_print_int_no_newline(val, ctx)
                }
            }
        }
        Ok(())
    }

    /// Builtin panic(msg): print error and call abort()
    pub(super) fn builtin_panic<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let msg = match args.first() {
            Some(Expr::String(s, _)) => s.clone(),
            _ => "panic".to_string(),
        };
        self.emit_puts(&format!("PANIC: {}", msg), ctx)?;
        self.emit_abort(ctx);
        Ok(ctx.module().get_context().i64_type().const_int(1, false))
    }

    /// Builtin assert(cond, msg): panic if condition is false
    pub(super) fn builtin_assert<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.is_empty() {
            return Ok(ctx.module().get_context().i64_type().const_int(0, false));
        }
        let cond_val = self.eval_int(&args[0], ctx)?;

        let function = ctx
            .builder()
            .get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "no function".into(),
                )
            })?;
        let context = ctx.module().get_context();

        let pass_block = context.append_basic_block(function, "assert.pass");
        let fail_block = context.append_basic_block(function, "assert.fail");
        let zero = context.i64_type().const_int(0, false);
        let cmp = ctx
            .builder()
            .build_int_compare(IntPredicate::EQ, cond_val, zero, "assert.check")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "assert compare failed".into(),
                )
            })?;
        ctx.builder()
            .build_conditional_branch(cmp, fail_block, pass_block)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "assert branch failed".into(),
                )
            })?;

        ctx.builder().position_at_end(fail_block);
        let msg = match args.get(1) {
            Some(Expr::String(s, _)) => format!("Assertion failed: {}", s),
            _ => "Assertion failed".to_string(),
        };
        self.emit_puts(&msg, ctx)?;
        self.emit_abort(ctx);

        ctx.builder().position_at_end(pass_block);
        Ok(context.i64_type().const_int(0, false))
    }

    /// Emit abort() call (for panic/assert)
    pub(super) fn emit_abort(&mut self, ctx: &mut LlvmContext) {
        let abort_fn = ctx.module().get_function("abort").unwrap_or_else(|| {
            let void_type = ctx.module().get_context().void_type();
            ctx.module_mut()
                .add_function("abort", void_type.fn_type(&[], false), None)
        });
        ctx.builder().build_call(abort_fn, &[], "abort").ok();
    }

    /// Evaluate a string expression to an i8* LLVM pointer value.
    /// Handles string literals (emit global) and string variables (load i64 → int_to_ptr).
    pub(super) fn eval_string_arg<'a>(
        &mut self,
        expr: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::PointerValue<'a>> {
        let i8_ptr_type = ctx
            .module()
            .get_context()
            .i8_type()
            .ptr_type(AddressSpace::default());
        match expr {
            Expr::String(s, _) => {
                let gv = self.emit_string_global(s, ctx);
                let ptr = gv.as_pointer_value().const_cast(i8_ptr_type);
                Ok(ptr)
            }
            Expr::Ident(name, _) if self.string_vars.contains(name) => {
                let i64_val = self.load_ident(name, ctx)?;
                let ptr = ctx
                    .builder()
                    .build_int_to_ptr(i64_val, i8_ptr_type, "str_var_ptr")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "int_to_ptr for string var failed".into(),
                        )
                    })?;
                Ok(ptr)
            }
            _ => Err(LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "expected string argument".into(),
            )),
        }
    }

    /// Create or reuse a global string constant and return the GlobalValue
    pub(super) fn emit_string_global<'a>(
        &mut self,
        s: &str,
        ctx: &mut LlvmContext<'a>,
    ) -> inkwell::values::GlobalValue<'a> {
        let context = ctx.module().get_context();
        let null_terminated = format!("{}\0", s);
        let gv = ctx.module_mut().add_global(
            context.i8_type().array_type(null_terminated.len() as u32),
            Some(AddressSpace::default()),
            &format!("__leo_str_{}_{}", s.len(), s.len() % 1000),
        );
        gv.set_initializer(&context.const_string(null_terminated.as_bytes(), false));
        gv.set_constant(true);
        gv
    }

    /// Emit printf for integer without newline
    pub(super) fn emit_print_int_no_newline<'a>(
        &mut self,
        val: inkwell::values::IntValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) {
        let context = ctx.module().get_context();
        let fmt = "%ld\0".to_string();
        let gv = ctx.module_mut().add_global(
            context.i8_type().array_type(fmt.len() as u32),
            Some(AddressSpace::default()),
            &format!("__leo_fmt_int_nn_{}", val),
        );
        gv.set_initializer(&context.const_string(fmt.as_bytes(), false));
        gv.set_constant(true);
        let ptr = gv
            .as_pointer_value()
            .const_cast(context.i8_type().ptr_type(AddressSpace::default()));
        if let Some(printf) = ctx.module().get_function("printf") {
            ctx.builder()
                .build_call(printf, &[ptr.into(), val.into()], "print_int_nn")
                .ok();
        }
    }

    /// Emit printf for string literal without newline
    pub(super) fn emit_print_str(&mut self, s: &str, ctx: &mut LlvmContext) -> LeoResult<()> {
        let context = ctx.module().get_context();
        let fmt = format!("%s\0");
        let str_lit = format!("{}\0", s);
        let fmt_gv = ctx.module_mut().add_global(
            context.i8_type().array_type(fmt.len() as u32),
            Some(AddressSpace::default()),
            &format!("__leo_fmt_str"),
        );
        fmt_gv.set_initializer(&context.const_string(fmt.as_bytes(), false));
        fmt_gv.set_constant(true);
        let str_gv = ctx.module_mut().add_global(
            context.i8_type().array_type(str_lit.len() as u32),
            Some(AddressSpace::default()),
            &format!("__leo_str_print_{}", s.len()),
        );
        str_gv.set_initializer(&context.const_string(str_lit.as_bytes(), false));
        str_gv.set_constant(true);
        let fmt_ptr = fmt_gv
            .as_pointer_value()
            .const_cast(context.i8_type().ptr_type(AddressSpace::default()));
        let str_ptr = str_gv
            .as_pointer_value()
            .const_cast(context.i8_type().ptr_type(AddressSpace::default()));
        if let Some(printf) = ctx.module().get_function("printf") {
            ctx.builder()
                .build_call(printf, &[fmt_ptr.into(), str_ptr.into()], "print_str")
                .ok();
        }
        Ok(())
    }

    /// Emit printf("%ld\n", val) to print an i64
    pub(super) fn emit_print_int<'a>(
        &mut self,
        val: inkwell::values::IntValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) {
        let context = ctx.module().get_context();
        let fmt = format!("%ld\n\0");
        let gv = ctx.module_mut().add_global(
            context.i8_type().array_type(fmt.len() as u32),
            Some(AddressSpace::default()),
            &format!("__leo_fmt_int_{}", val),
        );
        gv.set_initializer(&context.const_string(fmt.as_bytes(), false));
        gv.set_constant(true);
        let ptr = gv
            .as_pointer_value()
            .const_cast(context.i8_type().ptr_type(AddressSpace::default()));
        if let Some(printf) = ctx.module().get_function("printf") {
            ctx.builder()
                .build_call(printf, &[ptr.into(), val.into()], "print_int")
                .ok();
        }
    }

    /// Emit puts(string) for string literal
    pub(super) fn emit_puts(&mut self, s: &str, ctx: &mut LlvmContext) -> LeoResult<()> {
        let context = ctx.module().get_context();
        let fmt = format!("{}\0", s);
        let gv = ctx.module_mut().add_global(
            context.i8_type().array_type(fmt.len() as u32),
            Some(AddressSpace::default()),
            &format!("__leo_str_{}", s.len()),
        );
        gv.set_initializer(&context.const_string(fmt.as_bytes(), false));
        gv.set_constant(true);
        let ptr = gv
            .as_pointer_value()
            .const_cast(context.i8_type().ptr_type(AddressSpace::default()));
        if let Some(puts) = ctx.module().get_function("puts") {
            ctx.builder()
                .build_call(puts, &[ptr.into()], "puts_call")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "puts failed".into(),
                    )
                })?;
        }
        Ok(())
    }
}
