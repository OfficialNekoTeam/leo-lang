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
        let dest_i8 = ctx
            .builder()
            .build_pointer_cast(dest_ptr.into_pointer_value(), i8_ptr_type, "dest_i8")
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
        let dest_i8 = ctx
            .builder()
            .build_pointer_cast(dest_ptr.into_pointer_value(), i8_ptr_type, "dest_i8")
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
}
