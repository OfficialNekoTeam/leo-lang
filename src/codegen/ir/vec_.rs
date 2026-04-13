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
        let header = ctx
            .builder()
            .build_pointer_cast(header_raw.into_pointer_value(), i64_ptr_type, "vec_hdr")
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
        let di = ctx
            .builder()
            .build_ptr_to_int(dr.into_pointer_value(), i64_type, "di")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int".into(),
                )
            })?;
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
        let dps = unsafe {
            ctx.builder()
                .build_in_bounds_gep(header, &[two], "dps")
                .map_err(|_| {
                    LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep".into())
                })?
        };
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
        let ndp = ctx
            .builder()
            .build_pointer_cast(
                nd.try_as_basic_value()
                    .left()
                    .ok_or_else(|| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "void".into(),
                        )
                    })?
                    .into_pointer_value(),
                i64_ptr_type,
                "ndp",
            )
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
        let header = ctx
            .builder()
            .build_int_to_ptr(vec_val, i64_ptr_type, "vh")
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "i2p".into())
            })?;
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

    pub(super) fn builtin_file_read<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.is_empty() {
            return Ok(ctx.module().get_context().i64_type().const_int(0, false));
        }
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i8_ptr_type = context.i8_type().ptr_type(AddressSpace::default());
        let path_ptr = self.eval_string_arg(&args[0], ctx)?;
        let fopen_fn = ctx.module().get_function("fopen").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "fopen not declared".into(),
            )
        })?;
        let fread_fn = ctx.module().get_function("fread").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "fread not declared".into(),
            )
        })?;
        let fclose_fn = ctx.module().get_function("fclose").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "fclose not declared".into(),
            )
        })?;
        let malloc_fn = ctx.module().get_function("malloc").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "malloc not declared".into(),
            )
        })?;
        let mode_gv = self.emit_string_global("rb", ctx);
        let mode_ptr = mode_gv.as_pointer_value().const_cast(i8_ptr_type);
        let fp = ctx
            .builder()
            .build_call(fopen_fn, &[path_ptr.into(), mode_ptr.into()], "fopen")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fopen".into(),
                )
            })?;
        let fp_ptr = fp
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
        let buf_sz = i64_type.const_int(65536, false);
        let buf = ctx
            .builder()
            .build_call(malloc_fn, &[buf_sz.into()], "fbuf")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "malloc".into(),
                )
            })?;
        let buf_ptr = ctx
            .builder()
            .build_pointer_cast(
                buf.try_as_basic_value()
                    .left()
                    .ok_or_else(|| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "void".into(),
                        )
                    })?
                    .into_pointer_value(),
                i8_ptr_type,
                "fbuf",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cast".into(),
                )
            })?;
        let one = i64_type.const_int(1, false);
        ctx.builder()
            .build_call(
                fread_fn,
                &[buf_ptr.into(), one.into(), buf_sz.into(), fp_ptr.into()],
                "fread",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fread".into(),
                )
            })?;
        ctx.builder()
            .build_call(fclose_fn, &[fp_ptr.into()], "fclose")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fclose".into(),
                )
            })?;
        let null_pos = unsafe {
            ctx.builder()
                .build_in_bounds_gep(buf_ptr, &[i64_type.const_int(65535, false)], "np")
                .map_err(|_| {
                    LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep".into())
                })?
        };
        ctx.builder()
            .build_store(null_pos, context.i8_type().const_int(0, false))
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "store".into(),
                )
            })?;
        ctx.builder()
            .build_ptr_to_int(buf_ptr, i64_type, "fbuf_i64")
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "p2i".into())
            })
    }

    pub(super) fn builtin_file_write<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.len() < 2 {
            return Ok(ctx.module().get_context().i64_type().const_int(0, false));
        }
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i8_ptr_type = context.i8_type().ptr_type(AddressSpace::default());
        let path_ptr = self.eval_string_arg(&args[0], ctx)?;
        let content_ptr = self.eval_string_arg(&args[1], ctx)?;
        let fopen_fn = ctx.module().get_function("fopen").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "fopen".into(),
            )
        })?;
        let fwrite_fn = ctx.module().get_function("fwrite").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "fwrite".into(),
            )
        })?;
        let fclose_fn = ctx.module().get_function("fclose").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "fclose".into(),
            )
        })?;
        let strlen_fn = ctx.module().get_function("strlen").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strlen".into(),
            )
        })?;
        let mode_gv = self.emit_string_global("w", ctx);
        let mode_ptr = mode_gv.as_pointer_value().const_cast(i8_ptr_type);
        let fp = ctx
            .builder()
            .build_call(fopen_fn, &[path_ptr.into(), mode_ptr.into()], "fopen_w")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fopen".into(),
                )
            })?;
        let fp_ptr = fp
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
        let clen = ctx
            .builder()
            .build_call(strlen_fn, &[content_ptr.into()], "clen")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen".into(),
                )
            })?;
        let clen_val = clen
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "void".into(),
                )
            })?
            .into_int_value();
        let one = i64_type.const_int(1, false);
        ctx.builder()
            .build_call(
                fwrite_fn,
                &[
                    content_ptr.into(),
                    one.into(),
                    clen_val.into(),
                    fp_ptr.into(),
                ],
                "fwrite",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fwrite".into(),
                )
            })?;
        ctx.builder()
            .build_call(fclose_fn, &[fp_ptr.into()], "fclose_w")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fclose".into(),
                )
            })?;
        Ok(i64_type.const_int(0, false))
    }
}
