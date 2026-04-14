use super::*;

impl IrBuilder {
    /// Builtin str_len(s): returns string length as i64
    pub(super) fn builtin_str_len<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.is_empty() {
            return Ok(ctx.module().get_context().i64_type().const_int(0, false));
        }
        let str_ptr = self.eval_string_arg(&args[0], ctx)?;
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

    /// Builtin str_char_at(s, i): returns ASCII code of char at index as i64
    pub(super) fn builtin_str_char_at<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.len() < 2 {
            return Ok(ctx.module().get_context().i64_type().const_int(0, false));
        }
        let str_ptr = self.eval_string_arg(&args[0], ctx)?;
        let idx = self.eval_int(&args[1], ctx)?;
        self.emit_nonneg_check(idx, "runtime error: negative string index\n", ctx)?;
        let strlen_fn = ctx.module().get_function("strlen").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strlen not declared".into(),
            )
        })?;
        let slen = ctx
            .builder()
            .build_call(strlen_fn, &[str_ptr.into()], "slen")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen call failed".into(),
                )
            })?
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen returned void".into(),
                )
            })?;
        self.emit_bounds_check(
            idx,
            slen.into_int_value(),
            "runtime error: string index out of bounds\n",
            ctx,
        )?;
        let context = ctx.module().get_context();
        let i8_type = context.i8_type();
        let i8_ptr = i8_type.ptr_type(AddressSpace::default());
        // GEP: get pointer to s[idx]
        let casted_ptr = ctx
            .builder()
            .build_pointer_cast(str_ptr, i8_ptr, "str_to_i8ptr")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr cast failed".into(),
                )
            })?;
        let offset_ptr = unsafe {
            ctx.builder()
                .build_in_bounds_gep(casted_ptr, &[idx], "char_ptr")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "gep failed".into(),
                    )
                })?
        };
        let char_val = ctx
            .builder()
            .build_load(offset_ptr, "char_val")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load char failed".into(),
                )
            })?;
        let extended = ctx
            .builder()
            .build_int_z_extend(char_val.into_int_value(), context.i64_type(), "char_ext")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "zext failed".into(),
                )
            })?;
        Ok(extended)
    }

    /// Builtin str_slice(s, start, end): returns new substring (allocated)
    pub(super) fn builtin_str_slice<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.len() < 3 {
            return Ok(ctx.module().get_context().i64_type().const_int(0, false));
        }
        let str_ptr = self.eval_string_arg(&args[0], ctx)?;
        let start = self.eval_int(&args[1], ctx)?;
        let end = self.eval_int(&args[2], ctx)?;
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i8_ptr_type = context.i8_type().ptr_type(AddressSpace::default());

        // a) start >= 0
        self.emit_nonneg_check(start, "runtime error: str_slice start < 0\n", ctx)?;

        // b) end <= strlen(s)
        let strlen_fn = ctx.module().get_function("strlen").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strlen not found".into(),
            )
        })?;
        let str_len_call = ctx
            .builder()
            .build_call(strlen_fn, &[str_ptr.into()], "slice_strlen")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen call failed".into(),
                )
            })?;
        let str_len_val = str_len_call
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
        let str_len_int = ctx
            .builder()
            .build_int_cast(str_len_val, i64_type, "slen_i64")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "int cast failed".into(),
                )
            })?;
        self.emit_bounds_check(
            end,
            str_len_int,
            "runtime error: str_slice end > string length\n",
            ctx,
        )?;

        // c) start <= end
        let start_gt_end = ctx
            .builder()
            .build_int_compare(IntPredicate::SGT, start, end, "start_gt_end")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "compare failed".into(),
                )
            })?;
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
        let fail_block = context.append_basic_block(function, "slice_range_fail");
        let ok_block = context.append_basic_block(function, "slice_range_ok");
        ctx.builder()
            .build_conditional_branch(start_gt_end, fail_block, ok_block)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "branch failed".into(),
                )
            })?;
        ctx.builder().position_at_end(fail_block);
        self.emit_puts("runtime error: str_slice start > end\n", ctx)?;
        self.emit_abort(ctx);
        let _ = ctx.builder().build_unreachable();
        ctx.builder().position_at_end(ok_block);

        // len = end - start + 1 (for null terminator)
        let one = i64_type.const_int(1, false);
        let len = ctx
            .builder()
            .build_int_sub(end, start, "slice_len")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "sub failed".into(),
                )
            })?;
        let alloc_size = ctx
            .builder()
            .build_int_add(len, one, "alloc_size")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "add failed".into(),
                )
            })?;

        // dest = malloc(alloc_size)
        let malloc_fn = ctx.module().get_function("malloc").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "malloc not declared".into(),
            )
        })?;
        let dest = ctx
            .builder()
            .build_call(malloc_fn, &[alloc_size.into()], "malloc_dest")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "malloc failed".into(),
                )
            })?;
        let dest_ptr = dest.try_as_basic_value().left().ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "malloc void".into(),
            )
        })?;
        let dest_pval = dest_ptr.into_pointer_value();
        // NULL check: abort if slice malloc failed
        let dest_i64 = ctx
            .builder()
            .build_ptr_to_int(dest_pval, i64_type, "dest_i64")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        self.emit_null_check(dest_i64, "runtime error: out of memory\n", ctx)?;
        let dest_i8 = ctx
            .builder()
            .build_pointer_cast(dest_pval, i8_ptr_type, "dest_i8")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr cast failed".into(),
                )
            })?;

        // src = str_ptr + start (GEP)
        let src_i8 = ctx
            .builder()
            .build_pointer_cast(str_ptr, i8_ptr_type, "src_i8")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr cast failed".into(),
                )
            })?;
        let src_offset = unsafe {
            ctx.builder()
                .build_in_bounds_gep(src_i8, &[start], "src_offset")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "gep failed".into(),
                    )
                })?
        };

        // memcpy(dest, src, len)
        let memcpy_fn = ctx.module().get_function("memcpy").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "memcpy not declared".into(),
            )
        })?;
        ctx.builder()
            .build_call(
                memcpy_fn,
                &[dest_i8.into(), src_offset.into(), len.into()],
                "memcpy_call",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "memcpy failed".into(),
                )
            })?;

        // null terminate: dest[len] = 0
        let null_pos = unsafe {
            ctx.builder()
                .build_in_bounds_gep(dest_i8, &[len], "null_pos")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "gep failed".into(),
                    )
                })?
        };
        ctx.builder()
            .build_store(null_pos, context.i8_type().const_int(0, false))
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "store null failed".into(),
                )
            })?;

        // Return dest as i64 (pointer cast)
        let dest_as_i64 = ctx
            .builder()
            .build_ptr_to_int(dest_i8, i64_type, "ptr_to_int")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        Ok(dest_as_i64)
    }

    /// Builtin str_concat(a, b): returns new concatenated string (allocated)
    pub(super) fn builtin_str_concat<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.len() < 2 {
            return Ok(ctx.module().get_context().i64_type().const_int(0, false));
        }
        let a_ptr = self.eval_string_arg(&args[0], ctx)?;
        let b_ptr = self.eval_string_arg(&args[1], ctx)?;
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i8_ptr_type = context.i8_type().ptr_type(AddressSpace::default());

        // total_len = strlen(a) + strlen(b) + 1
        let strlen_fn = ctx.module().get_function("strlen").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strlen not declared".into(),
            )
        })?;
        let a_len = ctx
            .builder()
            .build_call(strlen_fn, &[a_ptr.into()], "a_len")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen a failed".into(),
                )
            })?;
        let b_len = ctx
            .builder()
            .build_call(strlen_fn, &[b_ptr.into()], "b_len")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen b failed".into(),
                )
            })?;
        let a_len_val = a_len
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
        let b_len_val = b_len
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
        let total = ctx
            .builder()
            .build_int_add(
                ctx.builder()
                    .build_int_add(a_len_val, b_len_val, "sum")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "add failed".into(),
                        )
                    })?,
                one,
                "total",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "add failed".into(),
                )
            })?;

        // dest = malloc(total)
        let malloc_fn = ctx.module().get_function("malloc").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "malloc not declared".into(),
            )
        })?;
        let dest = ctx
            .builder()
            .build_call(malloc_fn, &[total.into()], "malloc_concat")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "malloc failed".into(),
                )
            })?;
        let dest_ptr = dest.try_as_basic_value().left().ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "malloc void".into(),
            )
        })?;
        let dest_pval = dest_ptr.into_pointer_value();
        // NULL check: abort if concat malloc failed
        let dest_i64 = ctx
            .builder()
            .build_ptr_to_int(dest_pval, i64_type, "dest_i64")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        self.emit_null_check(dest_i64, "runtime error: out of memory\n", ctx)?;
        let dest_i8 = ctx
            .builder()
            .build_pointer_cast(dest_pval, i8_ptr_type, "dest_i8")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr cast failed".into(),
                )
            })?;

        // strcpy(dest, a) + strcat(dest, b)
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
        ctx.builder()
            .build_call(strcpy_fn, &[dest_i8.into(), a_ptr.into()], "strcpy_call")
            .ok();
        ctx.builder()
            .build_call(strcat_fn, &[dest_i8.into(), b_ptr.into()], "strcat_call")
            .ok();

        let dest_as_i64 = ctx
            .builder()
            .build_ptr_to_int(dest_i8, i64_type, "ptr_to_int")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        Ok(dest_as_i64)
    }

    /// Index into a string variable: s[i] returns the i-th byte as i64 (char code).
    /// Panics if index is out of bounds.
    pub(super) fn eval_string_index<'a>(
        &mut self,
        obj: &Expr,
        idx: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let str_ptr = self.eval_string_arg(obj, ctx)?;
        let idx_val = self.eval_int(idx, ctx)?;
        self.emit_nonneg_check(idx_val, "runtime error: negative string index\n", ctx)?;
        let strlen_fn = ctx.module().get_function("strlen").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strlen not declared".into(),
            )
        })?;
        let slen = ctx
            .builder()
            .build_call(strlen_fn, &[str_ptr.into()], "slen")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen call failed".into(),
                )
            })?
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen returned void".into(),
                )
            })?;
        self.emit_bounds_check(
            idx_val,
            slen.into_int_value(),
            "runtime error: string index out of bounds\n",
            ctx,
        )?;
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i8_ptr_type = context.i8_type().ptr_type(AddressSpace::default());
        let casted = ctx
            .builder()
            .build_pointer_cast(str_ptr, i8_ptr_type, "str_i8ptr")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr cast failed".into(),
                )
            })?;
        let char_ptr = unsafe {
            ctx.builder()
                .build_in_bounds_gep(casted, &[idx_val], "char_ptr")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "gep failed".into(),
                    )
                })?
        };
        let char_val = ctx
            .builder()
            .build_load(char_ptr, "char_byte")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load char failed".into(),
                )
            })?;
        let extended = ctx
            .builder()
            .build_int_z_extend(char_val.into_int_value(), i64_type, "char_ext")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "zext failed".into(),
                )
            })?;
        Ok(extended)
    }

    /// Builtin char_to_str(ch): convert i64 ASCII code to single-char heap string
    pub(super) fn builtin_char_to_str<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i8_type = context.i8_type();
        let i8_ptr = i8_type.ptr_type(AddressSpace::default());
        if args.is_empty() {
            return Ok(i64_type.const_int(0, false));
        }
        let ch_val = self.eval_int(&args[0], ctx)?;
        let malloc_fn = ctx.module().get_function("malloc").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "malloc not declared".into(),
            )
        })?;
        let buf_size = i64_type.const_int(2, false);
        let buf_alloc = ctx
            .builder()
            .build_call(malloc_fn, &[buf_size.into()], "c2s_malloc")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "malloc char_to_str failed".into(),
                )
            })?;
        let buf_raw = buf_alloc
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "malloc returned void".into(),
                )
            })?
            .into_pointer_value();
        // NULL check: abort if char_to_str malloc failed
        let buf_i64 = ctx
            .builder()
            .build_ptr_to_int(buf_raw, i64_type, "c2s_i64")
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
            .build_pointer_cast(buf_raw, i8_ptr, "c2s_buf")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cast char_to_str buf failed".into(),
                )
            })?;
        let trunc = ctx
            .builder()
            .build_int_truncate(ch_val, i8_type, "c2s_trunc")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "trunc char failed".into(),
                )
            })?;
        unsafe {
            let char_ptr = ctx
                .builder()
                .build_in_bounds_gep(buf_ptr, &[i64_type.const_int(0, false)], "c2s_ch")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "gep char_to_str failed".into(),
                    )
                })?;
            ctx.builder().build_store(char_ptr, trunc).map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "store char failed".into(),
                )
            })?;
            let null_ptr = ctx
                .builder()
                .build_in_bounds_gep(buf_ptr, &[i64_type.const_int(1, false)], "c2s_null")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "gep null failed".into(),
                    )
                })?;
            ctx.builder()
                .build_store(null_ptr, i8_type.const_int(0, false))
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "store null failed".into(),
                    )
                })?;
        }
        ctx.builder()
            .build_ptr_to_int(buf_ptr, i64_type, "c2s_result")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int char_to_str failed".into(),
                )
            })
    }

    /// Builtin is_digit(ch): returns 1 if ch is ASCII digit (48-57), else 0
    pub(super) fn builtin_is_digit<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        if args.is_empty() {
            return Ok(i64_type.const_int(0, false));
        }
        let ch = self.eval_int(&args[0], ctx)?;
        let zero_code = i64_type.const_int('0' as u64, false);
        let nine_code = i64_type.const_int('9' as u64, false);
        let ge_zero = ctx
            .builder()
            .build_int_compare(IntPredicate::SGE, ch, zero_code, "id_ge0")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cmp ge0 failed".into(),
                )
            })?;
        let le_nine = ctx
            .builder()
            .build_int_compare(IntPredicate::SLE, ch, nine_code, "id_le9")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cmp le9 failed".into(),
                )
            })?;
        let is_d = ctx
            .builder()
            .build_and(ge_zero, le_nine, "is_d_and")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "and is_digit failed".into(),
                )
            })?;
        ctx.builder()
            .build_int_z_extend(is_d, i64_type, "is_digit")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "zext is_digit failed".into(),
                )
            })
    }

    /// Builtin is_alpha(ch): returns 1 if ch is ASCII letter, else 0
    pub(super) fn builtin_is_alpha<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        if args.is_empty() {
            return Ok(i64_type.const_int(0, false));
        }
        let ch = self.eval_int(&args[0], ctx)?;
        let la = i64_type.const_int('a' as u64, false);
        let lz = i64_type.const_int('z' as u64, false);
        let ua = i64_type.const_int('A' as u64, false);
        let uz = i64_type.const_int('Z' as u64, false);
        let ge_la = ctx
            .builder()
            .build_int_compare(IntPredicate::SGE, ch, la, "ia_gela")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cmp gela failed".into(),
                )
            })?;
        let le_lz = ctx
            .builder()
            .build_int_compare(IntPredicate::SLE, ch, lz, "ia_lelz")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cmp lelz failed".into(),
                )
            })?;
        let lower = ctx
            .builder()
            .build_and(ge_la, le_lz, "ia_lower")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "and lower failed".into(),
                )
            })?;
        let ge_ua = ctx
            .builder()
            .build_int_compare(IntPredicate::SGE, ch, ua, "ia_geua")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cmp geua failed".into(),
                )
            })?;
        let le_uz = ctx
            .builder()
            .build_int_compare(IntPredicate::SLE, ch, uz, "ia_leuz")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cmp leuz failed".into(),
                )
            })?;
        let upper = ctx
            .builder()
            .build_and(ge_ua, le_uz, "ia_upper")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "and upper failed".into(),
                )
            })?;
        let alpha = ctx.builder().build_or(lower, upper, "ia_or").map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "or alpha failed".into(),
            )
        })?;
        ctx.builder()
            .build_int_z_extend(alpha, i64_type, "is_alpha")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "zext is_alpha failed".into(),
                )
            })
    }

    /// Builtin is_alnum(ch): returns 1 if is_digit(ch) or is_alpha(ch), else 0
    pub(super) fn builtin_is_alnum<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let i64_type = ctx.module().get_context().i64_type();
        if args.is_empty() {
            return Ok(i64_type.const_int(0, false));
        }
        let d = self.builtin_is_digit(args, ctx)?;
        let a = self.builtin_is_alpha(args, ctx)?;
        let or_val = ctx.builder().build_or(d, a, "is_alnum").map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "or alnum failed".into(),
            )
        })?;
        let zero = i64_type.const_int(0, false);
        let ne = ctx
            .builder()
            .build_int_compare(IntPredicate::NE, or_val, zero, "alnum_ne")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cmp alnum failed".into(),
                )
            })?;
        ctx.builder()
            .build_int_z_extend(ne, i64_type, "is_alnum")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "zext alnum failed".into(),
                )
            })
    }

    /// Builtin to_string(n): convert i64 to decimal string using snprintf
    pub(super) fn builtin_to_string<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i8_type = context.i8_type();
        let i8_ptr = i8_type.ptr_type(AddressSpace::default());
        let i32_type = context.i32_type();
        if args.is_empty() {
            return Ok(i64_type.const_int(0, false));
        }
        let n_val = self.eval_int(&args[0], ctx)?;
        let malloc_fn = ctx.module().get_function("malloc").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "malloc not declared".into(),
            )
        })?;
        let buf_size = i64_type.const_int(32, false);
        let buf_alloc = ctx
            .builder()
            .build_call(malloc_fn, &[buf_size.into()], "ts_malloc")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "malloc to_string failed".into(),
                )
            })?;
        let buf_raw = buf_alloc
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "malloc returned void".into(),
                )
            })?
            .into_pointer_value();
        // NULL check: abort if to_string malloc failed
        let buf_i64 = ctx
            .builder()
            .build_ptr_to_int(buf_raw, i64_type, "ts_i64")
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
            .build_pointer_cast(buf_raw, i8_ptr, "ts_buf")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cast to_string buf failed".into(),
                )
            })?;
        // Declare snprintf if not already present
        if ctx.module().get_function("snprintf").is_none() {
            ctx.module_mut().add_function(
                "snprintf",
                i32_type.fn_type(&[i8_ptr.into(), i64_type.into(), i8_ptr.into()], true),
                None,
            );
        }
        let snprintf_fn = ctx.module().get_function("snprintf").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "snprintf not found".into(),
            )
        })?;
        let fmt = self.emit_string_global("%ld", ctx);
        let fmt_i8 = ctx
            .builder()
            .build_pointer_cast(fmt.as_pointer_value(), i8_ptr, "ts_fmt")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cast fmt failed".into(),
                )
            })?;
        ctx.builder()
            .build_call(
                snprintf_fn,
                &[buf_ptr.into(), buf_size.into(), fmt_i8.into(), n_val.into()],
                "ts_snprintf",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "snprintf call failed".into(),
                )
            })?;
        ctx.builder()
            .build_ptr_to_int(buf_ptr, i64_type, "ts_result")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int to_string failed".into(),
                )
            })
    }
}
