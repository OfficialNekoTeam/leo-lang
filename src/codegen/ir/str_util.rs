use super::*;
use crate::ast::expr::Expr;
use crate::common::error::{ErrorCode, ErrorKind, LeoError, LeoResult};
use inkwell::AddressSpace;
use inkwell::IntPredicate;

impl IrBuilder {
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
        let buf_size = i64_type.const_int(2, false);
        let buf_raw = self.emit_checked_malloc(buf_size, "c2s_malloc", ctx)?;
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
        let buf_size = i64_type.const_int(32, false);
        let buf_raw = self.emit_checked_malloc(buf_size, "ts_malloc", ctx)?;
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
