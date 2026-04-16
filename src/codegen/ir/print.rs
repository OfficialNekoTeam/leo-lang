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
            Expr::Ident(name, _) if self.is_string_var(name, ctx) => {
                let ptr = self.eval_string_arg(expr, ctx)?;
                if newline {
                    self.emit_print_str_ptr(ptr, ctx)?;
                } else {
                    let context = ctx.module().get_context();
                    let i8_ptr_type = context.i8_type().ptr_type(AddressSpace::default());
                    let fmt = "%s\0";
                    let gv_name = format!("__leo_fmt_str_nn_{}", self.tmp_counter);
                    self.tmp_counter += 1;
                    let gv = ctx.module_mut().add_global(
                        context.i8_type().array_type(fmt.len() as u32),
                        Some(AddressSpace::default()),
                        &gv_name,
                    );
                    gv.set_initializer(&context.const_string(fmt.as_bytes(), false));
                    gv.set_constant(true);
                    let fmt_ptr = gv.as_pointer_value().const_cast(i8_ptr_type);
                    if let Some(printf) = ctx.module().get_function("printf") {
                        ctx.builder()
                            .build_call(printf, &[fmt_ptr.into(), ptr.into()], "print_str_nn")
                            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "printf fail".into()))?;
                    }
                }
            }
            _ => {
                let val = self.eval_int(expr, ctx)?;
                if newline {
                    self.emit_print_int(val, ctx)?
                } else {
                    self.emit_print_int_no_newline(val, ctx)?
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
        self.emit_abort(ctx)?;
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
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();

        ctx.builder().position_at_end(pass_block);
        Ok(context.i64_type().const_int(0, false))
    }

    /// Emit abort() call (for panic/assert)
    pub(super) fn emit_abort(&mut self, ctx: &mut LlvmContext) -> LeoResult<()> {
        let abort_fn = ctx.module().get_function("abort").unwrap_or_else(|| {
            let void_type = ctx.module().get_context().void_type();
            ctx.module_mut()
                .add_function("abort", void_type.fn_type(&[], false), None)
        });
        ctx.builder().build_call(abort_fn, &[], "abort").map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "abort".into()))?;
        Ok(())
    }

    /// Runtime NULL check after malloc/realloc/fopen.
    /// Compares ptr (as i64) to zero. If null, prints msg and aborts.
    /// Builder is positioned at the ok_block after return.
    pub(super) fn emit_null_check<'a>(
        &mut self,
        ptr_val: inkwell::values::IntValue<'a>,
        msg: &str,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<()> {
        let function = ctx
            .builder()
            .get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "no function for null check".into(),
                )
            })?;
        let context = ctx.module().get_context();
        let fail_block = context.append_basic_block(function, "null_fail");
        let ok_block = context.append_basic_block(function, "null_ok");
        let zero = context.i64_type().const_int(0, false);
        let is_null = ctx
            .builder()
            .build_int_compare(IntPredicate::EQ, ptr_val, zero, "is_null")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "null compare failed".into(),
                )
            })?;
        ctx.builder()
            .build_conditional_branch(is_null, fail_block, ok_block)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "null branch failed".into(),
                )
            })?;
        ctx.builder().position_at_end(fail_block);
        self.emit_puts(msg, ctx)?;
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();
        ctx.builder().position_at_end(ok_block);
        Ok(())
    }

    /// Runtime non-negative check for index values.
    /// If val < 0, prints msg and aborts. Builder positioned at ok_block after return.
    pub(super) fn emit_nonneg_check<'a>(
        &mut self,
        val: inkwell::values::IntValue<'a>,
        msg: &str,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<()> {
        let function = ctx
            .builder()
            .get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "no function for nonneg check".into(),
                )
            })?;
        let context = ctx.module().get_context();
        let fail_block = context.append_basic_block(function, "neg_fail");
        let ok_block = context.append_basic_block(function, "neg_ok");
        let zero = context.i64_type().const_int(0, false);
        let is_neg = ctx
            .builder()
            .build_int_compare(IntPredicate::SLT, val, zero, "is_neg")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "nonneg compare failed".into(),
                )
            })?;
        ctx.builder()
            .build_conditional_branch(is_neg, fail_block, ok_block)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "nonneg branch failed".into(),
                )
            })?;
        ctx.builder().position_at_end(fail_block);
        self.emit_puts(msg, ctx)?;
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();
        ctx.builder().position_at_end(ok_block);
        Ok(())
    }

    /// Runtime bounds check: val must be < len.
    /// If val >= len, prints msg and aborts. Builder positioned at ok_block after return.
    pub(super) fn emit_bounds_check<'a>(
        &mut self,
        val: inkwell::values::IntValue<'a>,
        len: inkwell::values::IntValue<'a>,
        msg: &str,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<()> {
        let function = ctx
            .builder()
            .get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "no function for bounds check".into(),
                )
            })?;
        let context = ctx.module().get_context();
        let fail_block = context.append_basic_block(function, "oob_fail");
        let ok_block = context.append_basic_block(function, "oob_ok");
        let oob = ctx
            .builder()
            .build_int_compare(IntPredicate::SGE, val, len, "is_oob")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "bounds compare failed".into(),
                )
            })?;
        ctx.builder()
            .build_conditional_branch(oob, fail_block, ok_block)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "bounds branch failed".into(),
                )
            })?;
        ctx.builder().position_at_end(fail_block);
        self.emit_puts(msg, ctx)?;
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();
        ctx.builder().position_at_end(ok_block);
        Ok(())
    }

    /// Runtime division-by-zero check.
    /// If divisor == 0, prints msg and aborts. Builder positioned at ok_block after return.
    pub(super) fn emit_div_zero_check<'a>(
        &mut self,
        divisor: inkwell::values::IntValue<'a>,
        msg: &str,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<()> {
        let function = ctx
            .builder()
            .get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "no function for div-zero check".into(),
                )
            })?;
        let context = ctx.module().get_context();
        let fail_block = context.append_basic_block(function, "divz_fail");
        let ok_block = context.append_basic_block(function, "divz_ok");
        let zero = context.i64_type().const_int(0, false);
        let is_zero = ctx
            .builder()
            .build_int_compare(IntPredicate::EQ, divisor, zero, "is_zero_div")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "div-zero compare failed".into(),
                )
            })?;
        ctx.builder()
            .build_conditional_branch(is_zero, fail_block, ok_block)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "div-zero branch failed".into(),
                )
            })?;
        ctx.builder().position_at_end(fail_block);
        self.emit_puts(msg, ctx)?;
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();
        ctx.builder().position_at_end(ok_block);
        Ok(())
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
            Expr::Ident(name, _) if self.is_string_var(name, ctx) => {
                let tv = self.load_ident(name, ctx)?;
                let ptr = if tv.value.is_pointer_value() {
                    tv.value.into_pointer_value()
                } else {
                    ctx.builder().build_int_to_ptr(tv.value.into_int_value(), i8_ptr_type, "str_var_ptr").map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "int_to_ptr str_var failed".into()))?
                };
                Ok(ptr)
            }
            Expr::Ident(name, _) => Err(LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("'{}' is not a string variable", name),
            )),
            Expr::Select(_, _, _) | Expr::Call(_, _, _) | Expr::Binary(_, _, _, _) => {
                let val = self.eval_int(expr, ctx)?;
                let ptr = ctx
                    .builder()
                    .build_int_to_ptr(val, i8_ptr_type, "str_expr_ptr")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "int_to_ptr for string expr failed".into(),
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
        let name = format!("__leo_str_{}_{}", self.tmp_counter, s.len());
        self.tmp_counter += 1;
        let gv = ctx.module_mut().add_global(
            context.i8_type().array_type(null_terminated.len() as u32),
            Some(AddressSpace::default()),
            &name,
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
    ) -> LeoResult<()> {
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
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "printf fail".into()))?;
        }
        Ok(())
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
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "printf fail".into()))?;
        }
        Ok(())
    }

    /// Emit printf("%ld\n", val) to print an i64
    pub(super) fn emit_print_int<'a>(
        &mut self,
        val: inkwell::values::IntValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<()> {
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
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "printf fail".into()))?;
        }
        Ok(())
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

    /// Emit printf("%s\n", ptr) for a runtime string pointer (i8*)
    pub(super) fn emit_print_str_ptr<'a>(
        &mut self,
        str_ptr: inkwell::values::PointerValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<()> {
        let context = ctx.module().get_context();
        let i8_ptr_type = context.i8_type().ptr_type(AddressSpace::default());
        let fmt_str = "%s\n\0";
        let name = format!("__leo_fmt_str_ptr_{}", self.tmp_counter);
        self.tmp_counter += 1;
        let gv = ctx.module_mut().add_global(
            context.i8_type().array_type(fmt_str.len() as u32),
            Some(AddressSpace::default()),
            &name,
        );
        gv.set_initializer(&context.const_string(fmt_str.as_bytes(), false));
        gv.set_constant(true);
        let fmt_ptr = gv.as_pointer_value().const_cast(i8_ptr_type);
        if let Some(printf) = ctx.module().get_function("printf") {
            ctx.builder()
                .build_call(printf, &[fmt_ptr.into(), str_ptr.into()], "print_str_ptr")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "printf str failed".into(),
                    )
                })?;
        }
        Ok(())
    }
}
