use super::*;

impl IrBuilder {
    pub(super) fn eval_and_emit(&mut self, expr: &Expr, ctx: &mut LlvmContext) -> LeoResult<()> {
        match expr {
            Expr::String(s, _) => self.emit_puts(s, ctx),
            Expr::Call(_, _, _) | Expr::Match(_, _, _) => {
                let _ = self.eval_int(expr, ctx)?;
                Ok(())
            }
            Expr::Ident(name, _) => {
                if self.is_string_var(name, ctx) {
                    let ptr = self.eval_string_arg(
                        &Expr::Ident(name.clone(), crate::common::span::Span::dummy()),
                        ctx,
                    )?;
                    self.emit_print_str_ptr(ptr, ctx)?;
                } else {
                    let val = self.load_ident(name, ctx)?;
                    self.emit_print_int(val, ctx);
                }
                Ok(())
            }
            _ => {
                let val = self.eval_int(expr, ctx)?;
                self.emit_print_int(val, ctx);
                Ok(())
            }
        }
    }

    /// Load identifier value: try local variable, then global constant
    pub(super) fn load_ident<'a>(
        &mut self,
        name: &str,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if let Some(ptr) = ctx.get_variable(name) {
            ctx.builder()
                .build_load(ptr, name)
                .map(|v| v.into_int_value())
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        format!("load failed for {}", name),
                    )
                })
        } else if let Some(gv) = ctx.module().get_global(name) {
            let val = ctx
                .builder()
                .build_load(gv.as_pointer_value(), name)
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        format!("load const {} failed", name),
                    )
                })?;
            Ok(val.into_int_value())
        } else {
            Err(LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("undefined variable: {}", name),
            ))
        }
    }

    /// Evaluate expression to an LLVM IntValue (handles ident load, literals, binary, unary)
    pub(super) fn eval_expr_to_value<'a>(
        &mut self,
        expr: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        match expr {
            Expr::Ident(name, _) => self.load_ident(name, ctx),
            _ => self.eval_int(expr, ctx),
        }
    }

    /// Evaluate integer expression (number, bool, binary, unary, call)
    pub(super) fn eval_int<'a>(
        &mut self,
        expr: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        match expr {
            Expr::Number(n, _) => Ok(ctx
                .module()
                .get_context()
                .i64_type()
                .const_int(*n as u64, false)),
            Expr::Bool(b, _) => Ok(ctx
                .module()
                .get_context()
                .i64_type()
                .const_int(*b as u64, false)),
            Expr::Char(c, _) => Ok(ctx
                .module()
                .get_context()
                .i64_type()
                .const_int(*c as u64, false)),
            Expr::Ident(name, _) => self.load_ident(name, ctx),
            Expr::Binary(op, left, right, _) => {
                if let (Expr::Number(l, _), Expr::Number(r, _)) = (left.as_ref(), right.as_ref()) {
                    if let Some(folded) = Self::fold_constants(op, *l, *r) {
                        return Ok(ctx
                            .module()
                            .get_context()
                            .i64_type()
                            .const_int(folded as u64, false));
                    }
                }
                if let Some(result) = self.try_string_compare(op, left, right, ctx)? {
                    return Ok(result);
                }
                if let Some(result) = self.try_string_concat(op, left, right, ctx)? {
                    return Ok(result);
                }
                let lv = self.eval_int(left, ctx)?;
                let rv = self.eval_int(right, ctx)?;
                self.emit_binop(op, lv, rv, ctx)
            }
            Expr::Unary(op, e, _) => {
                let val = self.eval_int(e, ctx)?;
                self.emit_unop(op, val, ctx)
            }
            Expr::Call(callee, args, _) => self.eval_call(callee, args, ctx),
            Expr::Index(obj, idx, _) => self.eval_index(obj, idx, ctx),
            Expr::Select(obj, field, _) => self.eval_select(obj, field, ctx),
            Expr::Array(_, _) | Expr::ArrayRepeat(_, _, _) => self.eval_array_alloc(expr, ctx),
            Expr::StructInit(_, _, _) => self.eval_struct_init(expr, ctx),
            Expr::Match(scrutinee, arms, _) => self.eval_match(scrutinee, arms, ctx),
            Expr::String(s, _) => {
                let gv = self.emit_string_global(s, ctx);
                let i8_ptr = ctx
                    .module()
                    .get_context()
                    .i8_type()
                    .ptr_type(AddressSpace::default());
                let ptr = gv.as_pointer_value().const_cast(i8_ptr);
                ctx.builder()
                    .build_ptr_to_int(ptr, ctx.module().get_context().i64_type(), "str_as_i64")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "ptr_to_int for string failed".into(),
                        )
                    })
            }
            Expr::Float(_, _)
            | Expr::Lambda(_, _, _)
            | Expr::If(_, _, _, _)
            | Expr::Block(_, _)
            | Expr::Await(_, _)
            | Expr::Unit(_) => Err(LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("cannot evaluate {:?} as integer", expr),
            )),
        }
    }

    /// Evaluate function call: check builtins first, then user functions
    pub(super) fn eval_call<'a>(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        match callee {
            Expr::Ident(name, _) => {
                let func_name = name.clone();
                if func_name.contains("::") {
                    return self.eval_enum_constructor(&func_name, args, ctx);
                }
                match func_name.as_str() {
                    "println" => return self.builtin_println(args, ctx),
                    "print" => return self.builtin_print(args, ctx),
                    "panic" => return self.builtin_panic(args, ctx),
                    "assert" => return self.builtin_assert(args, ctx),
                    "str_len" => return self.builtin_str_len(args, ctx),
                    "str_char_at" => return self.builtin_str_char_at(args, ctx),
                    "str_slice" => return self.builtin_str_slice(args, ctx),
                    "str_concat" => return self.builtin_str_concat(args, ctx),
                    "vec_new" => return self.builtin_vec_new(args, ctx),
                    "vec_push" => return self.builtin_vec_push(args, ctx),
                    "vec_get" => return self.builtin_vec_get(args, ctx),
                    "vec_len" => return self.builtin_vec_len(args, ctx),
                    "file_read" => return self.builtin_file_read(args, ctx),
                    "file_write" => return self.builtin_file_write(args, ctx),
                    "char_to_str" => return self.builtin_char_to_str(args, ctx),
                    "is_digit" => return self.builtin_is_digit(args, ctx),
                    "is_alpha" => return self.builtin_is_alpha(args, ctx),
                    "is_alnum" => return self.builtin_is_alnum(args, ctx),
                    "to_string" => return self.builtin_to_string(args, ctx),
                    _ => {}
                }
                let func = ctx.get_function(&func_name).ok_or_else(|| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        format!("undefined function: {}", func_name),
                    )
                })?;
                let mut arg_values: Vec<_> = Vec::new();
                for arg in args {
                    let val = self.eval_int(arg, ctx)?;
                    arg_values.push(BasicValueEnum::from(val).into());
                }
                let call_site = ctx
                    .builder()
                    .build_call(func, &arg_values, "call")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            format!("call {} failed", func_name),
                        )
                    })?;
                let ret = call_site.try_as_basic_value().left().ok_or_else(|| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        format!("{} returned void", func_name),
                    )
                })?;
                Ok(ret.into_int_value())
            }
            Expr::Select(obj, method, _) => self.eval_method_call(obj, method, args, ctx),
            _ => Err(LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "only direct function calls supported".into(),
            )),
        }
    }

    pub(super) fn eval_method_call<'a>(
        &mut self,
        obj: &Expr,
        method: &str,
        _args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        match method {
            "len" => match obj {
                Expr::Ident(name, _) => {
                    if let Some(size) = self.array_sizes.get(name).copied() {
                        if self.is_string_var(name, ctx) {
                            return self.runtime_strlen(name, ctx);
                        }
                        return Ok(i64_type.const_int(size as u64, false));
                    }
                    if self.is_string_var(name, ctx) {
                        return self.runtime_strlen(name, ctx);
                    }
                    Err(LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        format!("{} has no known length", name),
                    ))
                }
                Expr::Select(inner_obj, field, _) => {
                    if self.select_is_string(inner_obj, field, ctx) {
                        let ptr = self.eval_string_arg(obj, ctx)?;
                        let strlen_fn = ctx.module().get_function("strlen").ok_or_else(|| {
                            LeoError::new(
                                ErrorKind::Syntax,
                                ErrorCode::CodegenLLVMError,
                                "strlen not declared".into(),
                            )
                        })?;
                        let result = ctx
                            .builder()
                            .build_call(strlen_fn, &[ptr.into()], "sel_strlen")
                            .map_err(|_| {
                                LeoError::new(
                                    ErrorKind::Syntax,
                                    ErrorCode::CodegenLLVMError,
                                    "strlen call failed".into(),
                                )
                            })?;
                        return Ok(result
                            .try_as_basic_value()
                            .left()
                            .ok_or_else(|| {
                                LeoError::new(
                                    ErrorKind::Syntax,
                                    ErrorCode::CodegenLLVMError,
                                    "strlen void".into(),
                                )
                            })?
                            .into_int_value());
                    }
                    Err(LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        ".len() on select: not a string field".into(),
                    ))
                }
                Expr::String(s, _) => Ok(i64_type.const_int(s.len() as u64, false)),
                Expr::Array(elems, _) => Ok(i64_type.const_int(elems.len() as u64, false)),
                _ => Err(LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    ".len() only supported on arrays and strings".into(),
                )),
            },
            _ => {
                if let Expr::Ident(var_name, _) = obj {
                    if let Some(struct_type) = self.var_types.get(var_name) {
                        let key = (struct_type.clone(), method.to_string());
                        if let Some(mangled) = self.methods.get(&key).cloned() {
                            let func = ctx.get_function(&mangled).ok_or_else(|| {
                                LeoError::new(
                                    ErrorKind::Syntax,
                                    ErrorCode::CodegenLLVMError,
                                    format!("method function {} not found", mangled),
                                )
                            })?;
                            let obj_val = self.eval_int(obj, ctx)?;
                            let mut arg_values: Vec<BasicValueEnum> = vec![obj_val.into()];
                            for arg in _args {
                                let val = self.eval_int(arg, ctx)?;
                                arg_values.push(val.into());
                            }
                            let call_site = ctx
                                .builder()
                                .build_call(
                                    func,
                                    &arg_values.iter().map(|v| (*v).into()).collect::<Vec<_>>(),
                                    "method_call",
                                )
                                .map_err(|_| {
                                    LeoError::new(
                                        ErrorKind::Syntax,
                                        ErrorCode::CodegenLLVMError,
                                        format!("call method {} failed", mangled),
                                    )
                                })?;
                            let ret = call_site.try_as_basic_value().left().ok_or_else(|| {
                                LeoError::new(
                                    ErrorKind::Syntax,
                                    ErrorCode::CodegenLLVMError,
                                    format!("method {} returned void", mangled),
                                )
                            })?;
                            return Ok(ret.into_int_value());
                        }
                    }
                }
                Err(LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    format!("unknown method: .{}", method),
                ))
            }
        }
    }

    /// Call strlen at runtime on a string variable, returning the length as i64.
    pub(super) fn runtime_strlen<'a>(
        &mut self,
        name: &str,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let str_ptr = self.eval_string_arg(
            &Expr::Ident(name.to_string(), crate::common::span::Span::dummy()),
            ctx,
        )?;
        let strlen_fn = ctx.module().get_function("strlen").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strlen not declared".into(),
            )
        })?;
        let result = ctx
            .builder()
            .build_call(strlen_fn, &[str_ptr.into()], "strlen_call")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen call failed".into(),
                )
            })?;
        Ok(result
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen returned void".into(),
                )
            })?
            .into_int_value())
    }

    /// Try string comparison via strcmp for Eq/Ne when both operands are strings.
    /// Returns None if not a string comparison (caller falls through to int compare).
    pub(super) fn try_string_compare<'a>(
        &mut self,
        op: &BinOp,
        left: &Expr,
        right: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<Option<inkwell::values::IntValue<'a>>> {
        if !matches!(op, BinOp::Eq | BinOp::Ne) {
            return Ok(None);
        }
        let left_is_str = self.expr_is_string(left);
        let right_is_str = self.expr_is_string(right);
        if !left_is_str || !right_is_str {
            return Ok(None);
        }
        let left_ptr = self.eval_string_arg(left, ctx)?;
        let right_ptr = self.eval_string_arg(right, ctx)?;
        let strcmp_fn = ctx.module().get_function("strcmp").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strcmp not declared".into(),
            )
        })?;
        let cmp_result = ctx
            .builder()
            .build_call(
                strcmp_fn,
                &[left_ptr.into(), right_ptr.into()],
                "strcmp_call",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strcmp call failed".into(),
                )
            })?;
        let cmp_val = cmp_result
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strcmp returned void".into(),
                )
            })?
            .into_int_value();
        let i64_type = ctx.module().get_context().i64_type();
        let zero = ctx.module().get_context().i32_type().const_int(0, false);
        let predicate = match op {
            BinOp::Eq => IntPredicate::EQ,
            BinOp::Ne => IntPredicate::NE,
            _ => return Ok(None),
        };
        let cmp_bool = ctx
            .builder()
            .build_int_compare(predicate, cmp_val, zero, "str_cmp")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "str compare failed".into(),
                )
            })?;
        let result = ctx
            .builder()
            .build_int_z_extend(cmp_bool, i64_type, "str_cmp_ext")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "zext failed".into(),
                )
            })?;
        Ok(Some(result))
    }

    /// Try string concat via strcat for Add when at least one operand is a string.
    /// Allocates a new buffer, copies left then right via strcpy/strcat.
    fn try_string_concat<'a>(
        &mut self,
        op: &BinOp,
        left: &Expr,
        right: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<Option<inkwell::values::IntValue<'a>>> {
        if !matches!(op, BinOp::Add) {
            return Ok(None);
        }
        let left_is_str = self.expr_is_string(left);
        let right_is_str = self.expr_is_string(right);
        if !left_is_str && !right_is_str {
            return Ok(None);
        }
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i8_ptr_type = context.i8_type().ptr_type(AddressSpace::default());
        let malloc_fn = ctx.module().get_function("malloc").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "malloc not declared".into(),
            )
        })?;
        let strcpy_fn = ctx.module().get_function("strcpy").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strcpy not declared".into(),
            )
        })?;
        let strcat_fn = ctx.module().get_function("strcat").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strcat not declared".into(),
            )
        })?;
        let left_ptr = self.eval_string_arg(left, ctx)?;
        let right_ptr = self.eval_string_arg(right, ctx)?;
        let strlen_fn = ctx.module().get_function("strlen").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strlen not declared".into(),
            )
        })?;
        let left_len_call = ctx
            .builder()
            .build_call(strlen_fn, &[left_ptr.into()], "left_len")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen left failed".into(),
                )
            })?;
        let left_len = left_len_call
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen void".into(),
                )
            })?
            .into_int_value();
        let right_len_call = ctx
            .builder()
            .build_call(strlen_fn, &[right_ptr.into()], "right_len")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen right failed".into(),
                )
            })?;
        let right_len = right_len_call
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen void".into(),
                )
            })?
            .into_int_value();
        let one = i64_type.const_int(1, false);
        let sum_len = ctx
            .builder()
            .build_int_add(left_len, right_len, "sum_len")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "add failed".into(),
                )
            })?;
        let buf_size = ctx
            .builder()
            .build_int_add(sum_len, one, "total_size")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "add failed".into(),
                )
            })?;
        let buf_alloc = ctx
            .builder()
            .build_call(malloc_fn, &[buf_size.into()], "concat_buf")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "malloc concat failed".into(),
                )
            })?;
        let buf_raw = buf_alloc
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "malloc void".into(),
                )
            })?
            .into_pointer_value();
        // NULL check: abort if concat buffer malloc failed
        let buf_i64 = ctx
            .builder()
            .build_ptr_to_int(buf_raw, i64_type, "concat_i64")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        self.emit_null_check(buf_i64, "runtime error: out of memory\n", ctx)?;
        let buf_ptr = ctx
            .builder()
            .build_pointer_cast(buf_raw, i8_ptr_type, "concat_i8")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cast failed".into(),
                )
            })?;
        // strcpy(buf, left) then strcat(buf, right)
        ctx.builder()
            .build_call(
                strcpy_fn,
                &[buf_ptr.into(), left_ptr.into()],
                "strcpy_concat",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strcpy failed".into(),
                )
            })?;
        ctx.builder()
            .build_call(
                strcat_fn,
                &[buf_ptr.into(), right_ptr.into()],
                "strcat_concat",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strcat failed".into(),
                )
            })?;
        let result = ctx
            .builder()
            .build_ptr_to_int(buf_ptr, i64_type, "concat_result")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        Ok(Some(result))
    }

    pub(super) fn expr_is_string(&self, expr: &Expr) -> bool {
        match expr {
            Expr::String(_, _) => true,
            Expr::Ident(name, _) => self.string_vars.contains(name),
            Expr::Call(callee, _, _) => {
                if let Expr::Ident(fn_name, _) = callee.as_ref() {
                    matches!(
                        fn_name.as_str(),
                        "char_to_str" | "to_string" | "str_concat" | "str_slice" | "file_read"
                    )
                } else {
                    false
                }
            }
            Expr::Binary(BinOp::Add, left, right, _) => {
                self.expr_is_string(left) || self.expr_is_string(right)
            }
            Expr::Select(obj, field, _) => {
                if let Expr::Ident(var_name, _) = obj.as_ref() {
                    if let Some(struct_type) = self.var_types.get(var_name) {
                        if let Some(fields) = self.struct_fields.get(struct_type) {
                            if let Some(field_types) = self.struct_field_types.get(struct_type) {
                                if let Some(idx) = fields.iter().position(|f| f == field) {
                                    if let Some(ty) = field_types.get(idx) {
                                        return ty == "str";
                                    }
                                }
                            }
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// Emit binary arithmetic/comparison/logic (z-extends comparison results to i64)
    pub(super) fn emit_binop<'a>(
        &mut self,
        op: &BinOp,
        lv: inkwell::values::IntValue<'a>,
        rv: inkwell::values::IntValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let i64_type = ctx.module().get_context().i64_type();
        match op {
            BinOp::Add => ctx.builder().build_int_add(lv, rv, "add"),
            BinOp::Sub => ctx.builder().build_int_sub(lv, rv, "sub"),
            BinOp::Mul => ctx.builder().build_int_mul(lv, rv, "mul"),
            BinOp::Div => {
                self.emit_div_zero_check(rv, "runtime error: division by zero\n", ctx)?;
                ctx.builder().build_int_signed_div(lv, rv, "div")
            }
            BinOp::Mod => {
                self.emit_div_zero_check(rv, "runtime error: division by zero\n", ctx)?;
                ctx.builder().build_int_signed_rem(lv, rv, "rem")
            }
            BinOp::Eq => ctx
                .builder()
                .build_int_compare(IntPredicate::EQ, lv, rv, "eq")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "eq.ext")),
            BinOp::Ne => ctx
                .builder()
                .build_int_compare(IntPredicate::NE, lv, rv, "ne")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "ne.ext")),
            BinOp::Lt => ctx
                .builder()
                .build_int_compare(IntPredicate::SLT, lv, rv, "lt")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "lt.ext")),
            BinOp::Le => ctx
                .builder()
                .build_int_compare(IntPredicate::SLE, lv, rv, "le")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "le.ext")),
            BinOp::Gt => ctx
                .builder()
                .build_int_compare(IntPredicate::SGT, lv, rv, "gt")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "gt.ext")),
            BinOp::Ge => ctx
                .builder()
                .build_int_compare(IntPredicate::SGE, lv, rv, "ge")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "ge.ext")),
            BinOp::And => ctx.builder().build_and(lv, rv, "and"),
            BinOp::Or => ctx.builder().build_or(lv, rv, "or"),
            BinOp::BitAnd => ctx.builder().build_and(lv, rv, "band"),
            BinOp::BitOr => ctx.builder().build_or(lv, rv, "bor"),
            BinOp::Shl => ctx.builder().build_left_shift(lv, rv, "shl"),
            BinOp::Shr => ctx.builder().build_right_shift(lv, rv, true, "shr"),
        }
        .map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("{:?} failed", op),
            )
        })
    }

    /// Emit unary operation (negate, bitwise not)
    pub(super) fn emit_unop<'a>(
        &mut self,
        op: &UnOp,
        val: inkwell::values::IntValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        match op {
            UnOp::Neg | UnOp::Minus => {
                let zero = ctx.module().get_context().i64_type().const_int(0, false);
                ctx.builder().build_int_sub(zero, val, "neg")
            }
            UnOp::Not => {
                let ones = ctx
                    .module()
                    .get_context()
                    .i64_type()
                    .const_int(u64::MAX, true);
                ctx.builder().build_xor(val, ones, "not")
            }
            _ => return Ok(val),
        }
        .map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("{:?} failed", op),
            )
        })
    }

    pub(super) fn is_string_var(&self, name: &str, ctx: &LlvmContext) -> bool {
        use crate::llvm::context::LeoType;
        if let Some(ty) = ctx.get_type(name) {
            return *ty == LeoType::Str;
        }
        self.string_vars.contains(name)
    }

    /// Check if obj.field is a string-typed struct field
    fn select_is_string(&self, obj: &Expr, field: &str, ctx: &LlvmContext) -> bool {
        use crate::llvm::context::LeoType;
        if let Expr::Ident(var_name, _) = obj {
            if let Some(struct_type) = self.var_types.get(var_name) {
                if let Some(fields) = self.struct_fields.get(struct_type) {
                    if let Some(field_types) = self.struct_field_types.get(struct_type) {
                        if let Some(idx) = fields.iter().position(|f| f == field) {
                            if let Some(ty_name) = field_types.get(idx) {
                                return ty_name == "str";
                            }
                        }
                    }
                }
            }
        }
        if let Some(ty) = ctx.get_type(field) {
            return *ty == LeoType::Str;
        }
        false
    }
}
