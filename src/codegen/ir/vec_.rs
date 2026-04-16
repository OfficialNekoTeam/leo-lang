use super::*;

impl IrBuilder {
    pub(super) fn builtin_vec_new<'a>(
        &mut self,
        _args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
        let malloc_fn = ctx.module().get_function("malloc").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "malloc not declared".into(),
            )
        })?;
        let header_size = i64_type.const_int(24, false);
        let header_alloc = ctx
            .builder()
            .build_call(malloc_fn, &[header_size.into()], "vec_hdr_malloc")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "malloc vec header failed".into(),
                )
            })?;
        let header_raw = header_alloc.try_as_basic_value().left().ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "malloc void".into(),
            )
        })?;
        // NULL check: abort if header malloc failed
        let header_ptr = header_raw.into_pointer_value();
        let header_i64 = ctx
            .builder()
            .build_ptr_to_int(header_ptr, i64_type, "vec_hdr_i64")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        self.emit_null_check(header_i64, "runtime error: out of memory\n", ctx)?;
        let header = ctx
            .builder()
            .build_pointer_cast(header_ptr, i64_ptr_type, "vec_hdr")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr cast failed".into(),
                )
            })?;
        let zero = i64_type.const_int(0, false);
        let one = i64_type.const_int(1, false);
        let two = i64_type.const_int(2, false);
        // len = 0
        let lp = unsafe {
            ctx.builder()
                .build_in_bounds_gep(header, &[zero], "vlp")
                .map_err(|_| {
                    LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep".into())
                })?
        };
        ctx.builder().build_store(lp, zero).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "store".into(),
            )
        })?;
        // cap = 4
        let cp = unsafe {
            ctx.builder()
                .build_in_bounds_gep(header, &[one], "vcp")
                .map_err(|_| {
                    LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep".into())
                })?
        };
        ctx.builder()
            .build_store(cp, i64_type.const_int(4, false))
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "store".into(),
                )
            })?;
        // data = malloc(32)
        let dm = ctx
            .builder()
            .build_call(malloc_fn, &[i64_type.const_int(32, false).into()], "vec_dm")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "malloc data".into(),
                )
            })?;
        let dr = dm.try_as_basic_value().left().ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "void".into(),
            )
        })?;
        // NULL check: abort if data malloc failed
        let dr_ptr = dr.into_pointer_value();
        let di = ctx
            .builder()
            .build_ptr_to_int(dr_ptr, i64_type, "di")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int".into(),
                )
            })?;
        self.emit_null_check(di, "runtime error: out of memory\n", ctx)?;
        let dp = unsafe {
            ctx.builder()
                .build_in_bounds_gep(header, &[two], "vdp")
                .map_err(|_| {
                    LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep".into())
                })?
        };
        ctx.builder().build_store(dp, di).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "store".into(),
            )
        })?;
        ctx.builder()
            .build_ptr_to_int(header, i64_type, "vec_int")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int".into(),
                )
            })
    }

    pub(super) fn builtin_vec_push<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.len() < 2 {
            return Ok(ctx.module().get_context().i64_type().const_int(0, false));
        }
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
        let i8_ptr_type = context.i8_type().ptr_type(AddressSpace::default());
        let vec_val = self.eval_int(&args[0], ctx)?;
        let push_val = self.eval_int(&args[1], ctx)?;
        let header = ctx
            .builder()
            .build_int_to_ptr(vec_val, i64_ptr_type, "vh")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "int_to_ptr".into(),
                )
            })?;
        let zero = i64_type.const_int(0, false);
        let one = i64_type.const_int(1, false);
        let two = i64_type.const_int(2, false);
        let eight = i64_type.const_int(8, false);
        let lp = unsafe {
            ctx.builder()
                .build_in_bounds_gep(header, &[zero], "lp")
                .map_err(|_| {
                    LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep".into())
                })?
        };
        let len_v = ctx
            .builder()
            .build_load(lp, "vl")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load".into(),
                )
            })?
            .into_int_value();
        let cp = unsafe {
            ctx.builder()
                .build_in_bounds_gep(header, &[one], "cp")
                .map_err(|_| {
                    LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep".into())
                })?
        };
        let cap_v = ctx
            .builder()
            .build_load(cp, "vc")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load".into(),
                )
            })?
            .into_int_value();
        let dps = unsafe {
            ctx.builder()
                .build_in_bounds_gep(header, &[two], "dps")
                .map_err(|_| {
                    LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep".into())
                })?
        };
        let full = ctx
            .builder()
            .build_int_compare(IntPredicate::EQ, len_v, cap_v, "full")
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "cmp".into())
            })?;
        let func = ctx
            .builder()
            .get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "no fn".into(),
                )
            })?;
        let grow_bb = context.append_basic_block(func, "v.grow");
        let push_bb = context.append_basic_block(func, "v.push");
        ctx.builder()
            .build_conditional_branch(full, grow_bb, push_bb)
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "br".into())
            })?;
        // grow block
        ctx.builder().position_at_end(grow_bb);
        // Check cap won't overflow when doubled
        let max_half = i64_type.const_int((i64::MAX / 2) as u64, false);
        let cap_too_big = ctx
            .builder()
            .build_int_compare(IntPredicate::SGT, cap_v, max_half, "cap_overflow")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "overflow compare failed".into(),
                )
            })?;
        let overflow_fail = context.append_basic_block(func, "vec_overflow_fail");
        let overflow_ok = context.append_basic_block(func, "vec_overflow_ok");
        ctx.builder()
            .build_conditional_branch(cap_too_big, overflow_fail, overflow_ok)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "overflow branch failed".into(),
                )
            })?;
        ctx.builder().position_at_end(overflow_fail);
        self.emit_puts("runtime error: vec capacity overflow\n", ctx)?;
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();
        ctx.builder().position_at_end(overflow_ok);
        let new_cap = ctx
            .builder()
            .build_int_mul(cap_v, i64_type.const_int(2, false), "nc")
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "mul".into())
            })?;
        ctx.builder().build_store(cp, new_cap).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "store".into(),
            )
        })?;
        let old_di = ctx
            .builder()
            .build_load(dps, "odi")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load".into(),
                )
            })?
            .into_int_value();
        let old_dp = ctx
            .builder()
            .build_int_to_ptr(old_di, i8_ptr_type, "odp")
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "i2p".into())
            })?;
        let asz = ctx
            .builder()
            .build_int_mul(new_cap, eight, "asz")
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "mul".into())
            })?;
        let realloc_fn = ctx.module().get_function("realloc").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "no realloc".into(),
            )
        })?;
        let nd = ctx
            .builder()
            .build_call(realloc_fn, &[old_dp.into(), asz.into()], "rea")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "realloc".into(),
                )
            })?;
        let nd_raw = nd
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "void".into(),
                )
            })?
            .into_pointer_value();
        // NULL check: abort if realloc failed
        let nd_i64 = ctx
            .builder()
            .build_ptr_to_int(nd_raw, i64_type, "nd_i64")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        self.emit_null_check(nd_i64, "runtime error: out of memory\n", ctx)?;
        let ndp = ctx
            .builder()
            .build_pointer_cast(nd_raw, i64_ptr_type, "ndp")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cast".into(),
                )
            })?;
        let ndi = ctx
            .builder()
            .build_ptr_to_int(ndp, i64_type, "ndi")
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "p2i".into())
            })?;
        ctx.builder().build_store(dps, ndi).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "store".into(),
            )
        })?;
        ctx.builder()
            .build_unconditional_branch(push_bb)
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "br".into())
            })?;
        // push block
        ctx.builder().position_at_end(push_bb);
        let cl = ctx
            .builder()
            .build_load(lp, "cl")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load".into(),
                )
            })?
            .into_int_value();
        let cdi = ctx
            .builder()
            .build_load(dps, "cdi")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load".into(),
                )
            })?
            .into_int_value();
        let cdp = ctx
            .builder()
            .build_int_to_ptr(cdi, i64_ptr_type, "cdp")
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "i2p".into())
            })?;
        let ep = unsafe {
            ctx.builder()
                .build_in_bounds_gep(cdp, &[cl], "ep")
                .map_err(|_| {
                    LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep".into())
                })?
        };
        ctx.builder().build_store(ep, push_val).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "store".into(),
            )
        })?;
        let nl = ctx.builder().build_int_add(cl, one, "nl").map_err(|_| {
            LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "add".into())
        })?;
        ctx.builder().build_store(lp, nl).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "store".into(),
            )
        })?;
        Ok(i64_type.const_int(0, false))
    }

    pub(super) fn builtin_vec_get<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.len() < 2 {
            return Ok(ctx.module().get_context().i64_type().const_int(0, false));
        }
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
        let vec_val = self.eval_int(&args[0], ctx)?;
        let idx_val = self.eval_int(&args[1], ctx)?;
        self.emit_nonneg_check(idx_val, "runtime error: negative vec index\n", ctx)?;
        let header = ctx
            .builder()
            .build_int_to_ptr(vec_val, i64_ptr_type, "vh")
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "i2p".into())
            })?;
        let len_ptr = unsafe {
            ctx.builder()
                .build_in_bounds_gep(header, &[i64_type.const_int(0, false)], "len_ptr")
                .map_err(|_| {
                    LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep".into())
                })?
        };
        let len_val = ctx
            .builder()
            .build_load(len_ptr, "vec_len")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load".into(),
                )
            })?
            .into_int_value();
        self.emit_bounds_check(
            idx_val,
            len_val,
            "runtime error: vec index out of bounds\n",
            ctx,
        )?;
        let dps = unsafe {
            ctx.builder()
                .build_in_bounds_gep(header, &[i64_type.const_int(2, false)], "dps")
                .map_err(|_| {
                    LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep".into())
                })?
        };
        let di = ctx
            .builder()
            .build_load(dps, "di")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load".into(),
                )
            })?
            .into_int_value();
        let dp = ctx
            .builder()
            .build_int_to_ptr(di, i64_ptr_type, "dp")
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "i2p".into())
            })?;
        let ep = unsafe {
            ctx.builder()
                .build_in_bounds_gep(dp, &[idx_val], "ep")
                .map_err(|_| {
                    LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep".into())
                })?
        };
        let val = ctx.builder().build_load(ep, "ev").map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "load".into(),
            )
        })?;
        Ok(val.into_int_value())
    }

    pub(super) fn builtin_vec_len<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.is_empty() {
            return Ok(ctx.module().get_context().i64_type().const_int(0, false));
        }
        let i64_type = ctx.module().get_context().i64_type();
        let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
        let vec_val = self.eval_int(&args[0], ctx)?;
        let header = ctx
            .builder()
            .build_int_to_ptr(vec_val, i64_ptr_type, "vh")
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "i2p".into())
            })?;
        let lp = unsafe {
            ctx.builder()
                .build_in_bounds_gep(header, &[i64_type.const_int(0, false)], "lp")
                .map_err(|_| {
                    LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep".into())
                })?
        };
        let val = ctx.builder().build_load(lp, "vl").map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "load".into(),
            )
        })?;
        Ok(val.into_int_value())
    }
}
