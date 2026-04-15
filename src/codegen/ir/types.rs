use super::*;

impl IrBuilder {
    pub(super) fn eval_index<'a>(
        &mut self,
        obj: &Expr,
        idx: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if self.expr_is_string(obj, ctx) {
            return self.eval_string_index(obj, idx, ctx);
        }
        let obj_val = self.eval_int(obj, ctx)?;
        let idx_val = self.eval_int(idx, ctx)?;
        self.emit_nonneg_check(idx_val, "runtime error: negative index\n", ctx)?;
        if let Expr::Ident(name, _) = obj {
            if let Some(&size) = self.array_sizes.get(name) {
                let arr_len = ctx
                    .module()
                    .get_context()
                    .i64_type()
                    .const_int(size as u64, false);
                self.emit_bounds_check(
                    idx_val,
                    arr_len,
                    "runtime error: array index out of bounds\n",
                    ctx,
                )?;
            }
        }
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
        let obj_ptr = ctx
            .builder()
            .build_int_to_ptr(obj_val, i64_ptr_type, "obj_ptr")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "int_to_ptr failed".into(),
                )
            })?;
        let elem_ptr = unsafe {
            ctx.builder()
                .build_in_bounds_gep(obj_ptr, &[idx_val], "elem_ptr")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "gep failed".into(),
                    )
                })?
        };
        let loaded = ctx
            .builder()
            .build_load(elem_ptr, "elem_val")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load elem failed".into(),
                )
            })?;
        Ok(loaded.into_int_value())
    }

    pub(super) fn eval_select<'a>(
        &mut self,
        obj: &Expr,
        field: &str,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        match field {
            "len" => match obj {
                Expr::Ident(name, _) => {
                    let struct_len_idx = self
                        .var_types
                        .get(name)
                        .and_then(|st| self.struct_fields.get(st))
                        .and_then(|fields| fields.iter().position(|f| f == "len"));
                    if let Some(idx) = struct_len_idx {
                        let obj_val = self.eval_int(obj, ctx)?;
                        let obj_ptr = ctx
                            .builder()
                            .build_int_to_ptr(
                                obj_val,
                                i64_type.ptr_type(AddressSpace::default()),
                                "struct_ptr",
                            )
                            .map_err(|_| {
                                LeoError::new(
                                    ErrorKind::Syntax,
                                    ErrorCode::CodegenLLVMError,
                                    "int_to_ptr failed".into(),
                                )
                            })?;
                        let field_ptr = unsafe {
                            ctx.builder()
                                .build_in_bounds_gep(
                                    obj_ptr,
                                    &[i64_type.const_int(idx as u64, false)],
                                    "len_field",
                                )
                                .map_err(|_| {
                                    LeoError::new(
                                        ErrorKind::Syntax,
                                        ErrorCode::CodegenLLVMError,
                                        "gep len field failed".into(),
                                    )
                                })?
                        };
                        let loaded =
                            ctx.builder()
                                .build_load(field_ptr, "len_val")
                                .map_err(|_| {
                                    LeoError::new(
                                        ErrorKind::Syntax,
                                        ErrorCode::CodegenLLVMError,
                                        "load len field failed".into(),
                                    )
                                })?;
                        return Ok(loaded.into_int_value());
                    }
                    if self.is_string_var(name, ctx) {
                        return self.runtime_strlen(name, ctx);
                    }
                    if let Some(size) = self.array_sizes.get(name).copied() {
                        return Ok(i64_type.const_int(size as u64, false));
                    }
                    Err(LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        format!("{} has no known length", name),
                    ))
                }
                Expr::String(s, _) => Ok(i64_type.const_int(s.len() as u64, false)),
                _ => Err(LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    ".len() only supported on named arrays and string literals".into(),
                )),
            },
            _ => {
                let obj_val = self.eval_int(obj, ctx)?;
                let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
                let obj_ptr = ctx
                    .builder()
                    .build_int_to_ptr(obj_val, i64_ptr_type, "struct_ptr")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "int_to_ptr failed".into(),
                        )
                    })?;
                let field_idx = if let Expr::Ident(var_name, _) = obj {
                    if let Some(struct_type) = self.var_types.get(var_name) {
                        if let Some(fields) = self.struct_fields.get(struct_type) {
                            fields
                                .iter()
                                .position(|f| f == field)
                                .map(|i| i64_type.const_int(i as u64, false))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                let field_idx = field_idx.ok_or_else(|| {
                    LeoError::new(
                        ErrorKind::Semantic,
                        ErrorCode::SemaTypeMismatch,
                        format!("unknown field: .{}", field),
                    )
                })?;
                let field_ptr = unsafe {
                    ctx.builder()
                        .build_in_bounds_gep(obj_ptr, &[field_idx], "field_ptr")
                        .map_err(|_| {
                            LeoError::new(
                                ErrorKind::Syntax,
                                ErrorCode::CodegenLLVMError,
                                "struct gep failed".into(),
                            )
                        })?
                };
                let loaded = ctx
                    .builder()
                    .build_load(field_ptr, "field_val")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "load field failed".into(),
                        )
                    })?;
                Ok(loaded.into_int_value())
            }
        }
    }

    pub(super) fn eval_array_alloc<'a>(
        &mut self,
        expr: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
        match expr {
            Expr::Array(elements, _) => {
                let count = elements.len() as u64;
                let alloc_size = i64_type.const_int(count * 8, false);
                let malloc_fn = ctx.module().get_function("malloc").ok_or_else(|| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "malloc not declared".into(),
                    )
                })?;
                let mem = ctx
                    .builder()
                    .build_call(malloc_fn, &[alloc_size.into()], "array_malloc")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "malloc failed".into(),
                        )
                    })?;
                let mem_ptr = mem.try_as_basic_value().left().ok_or_else(|| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "malloc void".into(),
                    )
                })?;
                let mem_pval = mem_ptr.into_pointer_value();
                // NULL check: abort if array malloc failed
                let mem_i64 = ctx
                    .builder()
                    .build_ptr_to_int(mem_pval, i64_type, "arr_i64")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "ptr_to_int failed".into(),
                        )
                    })?;
                self.emit_null_check(mem_i64, "runtime error: out of memory\n", ctx)?;
                let base = ctx
                    .builder()
                    .build_pointer_cast(mem_pval, i64_ptr_type, "array_base")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "ptr cast failed".into(),
                        )
                    })?;
                for (i, elem) in elements.iter().enumerate() {
                    let val = self.eval_int(elem, ctx)?;
                    let idx = i64_type.const_int(i as u64, false);
                    let elem_ptr = unsafe {
                        ctx.builder()
                            .build_in_bounds_gep(base, &[idx], "store_ptr")
                            .map_err(|_| {
                                LeoError::new(
                                    ErrorKind::Syntax,
                                    ErrorCode::CodegenLLVMError,
                                    "gep failed".into(),
                                )
                            })?
                    };
                    ctx.builder().build_store(elem_ptr, val).map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "store elem failed".into(),
                        )
                    })?;
                }
                let result = ctx
                    .builder()
                    .build_ptr_to_int(base, i64_type, "array_as_int")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "ptr_to_int failed".into(),
                        )
                    })?;
                Ok(result)
            }
            Expr::ArrayRepeat(val, count_expr, _) => {
                let count_val = self.eval_int(count_expr, ctx)?;
                let _count_const = match count_expr.as_ref() {
                    Expr::Number(n, _) => *n as u64,
                    _ => 1,
                };
                let context = ctx.module().get_context();
                let i64_type = context.i64_type();
                let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
                let max_alloc = i64_type.const_int((i64::MAX / 8) as u64, false);
                let too_big = ctx
                    .builder()
                    .build_int_compare(IntPredicate::SGT, count_val, max_alloc, "arr_overflow")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "overflow compare failed".into(),
                        )
                    })?;
                let func = ctx
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
                let overflow_fail = context.append_basic_block(func, "arrrep_overflow_fail");
                let overflow_ok = context.append_basic_block(func, "arrrep_overflow_ok");
                ctx.builder()
                    .build_conditional_branch(too_big, overflow_fail, overflow_ok)
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "overflow branch failed".into(),
                        )
                    })?;
                ctx.builder().position_at_end(overflow_fail);
                self.emit_puts("runtime error: array repeat size overflow\n", ctx)?;
                self.emit_abort(ctx);
                let _ = ctx.builder().build_unreachable();
                ctx.builder().position_at_end(overflow_ok);
                let alloc_size = ctx
                    .builder()
                    .build_int_mul(count_val, i64_type.const_int(8, false), "alloc_size")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "mul failed".into(),
                        )
                    })?;
                let malloc_fn = ctx.module().get_function("malloc").ok_or_else(|| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "malloc not declared".into(),
                    )
                })?;
                let mem = ctx
                    .builder()
                    .build_call(malloc_fn, &[alloc_size.into()], "array_malloc")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "malloc failed".into(),
                        )
                    })?;
                let mem_ptr = mem.try_as_basic_value().left().ok_or_else(|| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "malloc void".into(),
                    )
                })?;
                let mem_pval = mem_ptr.into_pointer_value();
                // NULL check: abort if array repeat malloc failed
                let mem_i64 = ctx
                    .builder()
                    .build_ptr_to_int(mem_pval, i64_type, "arr_i64")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "ptr_to_int failed".into(),
                        )
                    })?;
                self.emit_null_check(mem_i64, "runtime error: out of memory\n", ctx)?;
                let base = ctx
                    .builder()
                    .build_pointer_cast(mem_pval, i64_ptr_type, "array_base")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "ptr cast failed".into(),
                        )
                    })?;
                let fill_val = self.eval_int(val, ctx)?;
                let func2 = ctx
                    .builder()
                    .get_insert_block()
                    .and_then(|bb| bb.get_parent())
                    .ok_or_else(|| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "no function for fill loop".into(),
                        )
                    })?;
                let loop_cond = context.append_basic_block(func2, "fill_cond");
                let loop_body = context.append_basic_block(func2, "fill_body");
                let loop_end = context.append_basic_block(func2, "fill_end");
                let i_ptr = ctx
                    .builder()
                    .build_alloca(i64_type, "fill_i")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "alloca i failed".into(),
                        )
                    })?;
                ctx.builder()
                    .build_store(i_ptr, i64_type.const_int(0, false))
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "store i failed".into(),
                        )
                    })?;
                ctx.builder()
                    .build_unconditional_branch(loop_cond)
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "br to cond failed".into(),
                        )
                    })?;
                ctx.builder().position_at_end(loop_cond);
                let cur_i = ctx
                    .builder()
                    .build_load(i_ptr, "cur_i")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "load i failed".into(),
                        )
                    })?
                    .into_int_value();
                let keep_going = ctx
                    .builder()
                    .build_int_compare(IntPredicate::SLT, cur_i, count_val, "keep_going")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "cmp failed".into(),
                        )
                    })?;
                ctx.builder()
                    .build_conditional_branch(keep_going, loop_body, loop_end)
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "br cond failed".into(),
                        )
                    })?;
                ctx.builder().position_at_end(loop_body);
                let elem_ptr = unsafe {
                    ctx.builder()
                        .build_in_bounds_gep(base, &[cur_i], "store_ptr")
                        .map_err(|_| {
                            LeoError::new(
                                ErrorKind::Syntax,
                                ErrorCode::CodegenLLVMError,
                                "gep failed".into(),
                            )
                        })?
                };
                ctx.builder().build_store(elem_ptr, fill_val).map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "store elem failed".into(),
                    )
                })?;
                let next_i = ctx
                    .builder()
                    .build_int_add(cur_i, i64_type.const_int(1, false), "next_i")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "add i failed".into(),
                        )
                    })?;
                ctx.builder().build_store(i_ptr, next_i).map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "store next i failed".into(),
                    )
                })?;
                ctx.builder()
                    .build_unconditional_branch(loop_cond)
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "br loop failed".into(),
                        )
                    })?;
                ctx.builder().position_at_end(loop_end);
                let result = ctx
                    .builder()
                    .build_ptr_to_int(base, i64_type, "array_as_int")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "ptr_to_int failed".into(),
                        )
                    })?;
                Ok(result)
            }
            _ => Ok(i64_type.const_int(0, false)),
        }
    }

    pub(super) fn eval_struct_init<'a>(
        &mut self,
        expr: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
        match expr {
            Expr::StructInit(_name, fields, _) => {
                let total_size = i64_type.const_int(fields.len() as u64 * 8, false);
                let malloc_fn = ctx.module().get_function("malloc").ok_or_else(|| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "malloc not declared".into(),
                    )
                })?;
                let mem = ctx
                    .builder()
                    .build_call(malloc_fn, &[total_size.into()], "struct_malloc")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "malloc failed".into(),
                        )
                    })?;
                let mem_ptr = mem.try_as_basic_value().left().ok_or_else(|| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "malloc void".into(),
                    )
                })?;
                let mem_pval = mem_ptr.into_pointer_value();
                // NULL check: abort if struct malloc failed
                let mem_i64 = ctx
                    .builder()
                    .build_ptr_to_int(mem_pval, i64_type, "struct_i64")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "ptr_to_int failed".into(),
                        )
                    })?;
                self.emit_null_check(mem_i64, "runtime error: out of memory\n", ctx)?;
                let base = ctx
                    .builder()
                    .build_pointer_cast(mem_pval, i64_ptr_type, "struct_base")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "ptr cast failed".into(),
                        )
                    })?;
                for (i, (_fname, fval)) in fields.iter().enumerate() {
                    let val = self.eval_int(fval, ctx)?;
                    let idx = i64_type.const_int(i as u64, false);
                    let field_ptr = unsafe {
                        ctx.builder()
                            .build_in_bounds_gep(base, &[idx], "field_store")
                            .map_err(|_| {
                                LeoError::new(
                                    ErrorKind::Syntax,
                                    ErrorCode::CodegenLLVMError,
                                    "gep failed".into(),
                                )
                            })?
                    };
                    ctx.builder().build_store(field_ptr, val).map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "store field failed".into(),
                        )
                    })?;
                }
                let result = ctx
                    .builder()
                    .build_ptr_to_int(base, i64_type, "struct_as_int")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "ptr_to_int failed".into(),
                        )
                    })?;
                Ok(result)
            }
            _ => Ok(i64_type.const_int(0, false)),
        }
    }

    pub(super) fn eval_enum_constructor<'a>(
        &mut self,
        qualified: &str,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let parts: Vec<&str> = qualified.split("::").collect();
        if parts.len() != 2 {
            return Err(LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("invalid enum variant: {}", qualified),
            ));
        }
        let enum_name = parts[0];
        let variant_name = parts[1];
        let tag = ctx
            .get_enum_variant_tag(enum_name, variant_name)
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    format!("unknown variant: {}", qualified),
                )
            })?;
        let context = ctx.module().get_context();
        let i32_type = context.i32_type();
        let i64_type = context.i64_type();
        let struct_type = ctx.module().get_struct_type(enum_name).ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("enum {} not defined in LLVM", enum_name),
            )
        })?;
        let enum_ptr = ctx
            .builder()
            .build_alloca(struct_type, qualified)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "alloca enum failed".into(),
                )
            })?;
        let tag_ptr = ctx
            .builder()
            .build_struct_gep(enum_ptr, 0, "tag_ptr")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "struct gep tag failed".into(),
                )
            })?;
        ctx.builder()
            .build_store(tag_ptr, i32_type.const_int(tag as u64, false))
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "store tag failed".into(),
                )
            })?;
        if !args.is_empty() {
            let payload_ptr = ctx
                .builder()
                .build_struct_gep(enum_ptr, 1, "payload_ptr")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "struct gep payload failed".into(),
                    )
                })?;
            let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
            let payload_as_i64 = ctx
                .builder()
                .build_pointer_cast(payload_ptr, i64_ptr_type, "payload_i64")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "ptr cast failed".into(),
                    )
                })?;
            for (i, arg) in args.iter().enumerate() {
                let val = self.eval_int(arg, ctx)?;
                let idx = i64_type.const_int(i as u64, false);
                let field_ptr = unsafe {
                    ctx.builder()
                        .build_in_bounds_gep(payload_as_i64, &[idx], "field_ptr")
                        .map_err(|_| {
                            LeoError::new(
                                ErrorKind::Syntax,
                                ErrorCode::CodegenLLVMError,
                                "gep failed".into(),
                            )
                        })?
                };
                ctx.builder().build_store(field_ptr, val).map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "store field failed".into(),
                    )
                })?;
            }
        }
        let result = ctx
            .builder()
            .build_ptr_to_int(enum_ptr, i64_type, "enum_as_int")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        Ok(result)
    }

    pub(super) fn eval_match<'a>(
        &mut self,
        scrutinee: &Expr,
        arms: &[(Expr, Expr)],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let context = ctx.module().get_context();
        let i32_type = context.i32_type();
        let i64_type = context.i64_type();
        let scrut_val = self.eval_int(scrutinee, ctx)?;
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
        let is_enum_match = arms.iter().any(|(pat, _)| match pat {
            Expr::Ident(name, _) => name.contains("::"),
            Expr::Call(callee, _, _) => {
                matches!(callee.as_ref(), Expr::Ident(n, _) if n.contains("::"))
            }
            _ => false,
        });
        let switch_val = if is_enum_match {
            let scrut_ptr = ctx
                .builder()
                .build_int_to_ptr(
                    scrut_val,
                    i32_type.ptr_type(AddressSpace::default()),
                    "match_ptr",
                )
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "int_to_ptr failed".into(),
                    )
                })?;
            let tag = ctx
                .builder()
                .build_load(scrut_ptr, "match_tag")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "load tag failed".into(),
                    )
                })?
                .into_int_value();
            let tag_i64 = ctx
                .builder()
                .build_int_z_extend(tag, i64_type, "tag_i64")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "zext failed".into(),
                    )
                })?;
            tag_i64
        } else {
            scrut_val
        };
        let merge_block = context.append_basic_block(function, "match.merge");
        let mut cases: Vec<(inkwell::values::IntValue, inkwell::basic_block::BasicBlock)> =
            Vec::new();
        let mut arm_blocks: Vec<(inkwell::basic_block::BasicBlock, &Expr)> = Vec::new();
        let mut default_block: Option<inkwell::basic_block::BasicBlock> = None;
        for (i, (pattern, _body)) in arms.iter().enumerate() {
            let arm_block = context.append_basic_block(function, &format!("match.arm.{}", i));
            match pattern {
                Expr::Ident(name, _) if name == "_" => {
                    default_block = Some(arm_block);
                }
                Expr::Ident(name, _) if name.contains("::") => {
                    let parts: Vec<&str> = name.split("::").collect();
                    if parts.len() == 2 {
                        if let Some(tag_idx) = ctx.get_enum_variant_tag(parts[0], parts[1]) {
                            cases.push((i64_type.const_int(tag_idx as u64, false), arm_block));
                        }
                    }
                }
                Expr::Call(callee, _, _) => {
                    if let Expr::Ident(name, _) = callee.as_ref() {
                        if name.contains("::") {
                            let parts: Vec<&str> = name.split("::").collect();
                            if parts.len() == 2 {
                                if let Some(tag_idx) = ctx.get_enum_variant_tag(parts[0], parts[1])
                                {
                                    cases.push((
                                        i64_type.const_int(tag_idx as u64, false),
                                        arm_block,
                                    ));
                                }
                            }
                        }
                    }
                }
                _ => {
                    if let Expr::Number(n, _) = pattern {
                        cases.push((i64_type.const_int(*n as u64, false), arm_block));
                    } else {
                        cases.push((i64_type.const_int(i as u64, false), arm_block));
                    }
                }
            }
            arm_blocks.push((arm_block, _body));
        }
        let default =
            default_block.unwrap_or_else(|| context.append_basic_block(function, "match.default"));
        ctx.builder()
            .build_switch(switch_val, default, &cases)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "switch failed".into(),
                )
            })?;
        for (i, (arm_block, body)) in arm_blocks.iter().enumerate() {
            ctx.builder().position_at_end(*arm_block);
            // Extract payload bindings for destructuring patterns (e.g., Token::Number(n))
            let pattern = &arms[i].0;
            if is_enum_match {
                self.bind_match_payload(pattern, scrut_val, ctx)?;
            }
            let result = self.eval_int(body, ctx)?;
            let _ = result;
            self.emit_branch(merge_block, ctx)?;
        }
        if default_block.is_none() {
            ctx.builder().position_at_end(default);
            self.emit_branch(merge_block, ctx)?;
        }
        ctx.builder().position_at_end(merge_block);
        Ok(i64_type.const_int(0, false))
    }

    /// Route destructuring patterns to payload extraction for enum match arms
    fn bind_match_payload<'a>(
        &mut self,
        pattern: &Expr,
        scrut_val: inkwell::values::IntValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<()> {
        if let Expr::Call(callee, bindings, _) = pattern {
            if let Expr::Ident(name, _) = callee.as_ref() {
                if name.contains("::") {
                    let parts: Vec<&str> = name.split("::").collect();
                    if parts.len() >= 2 {
                        let payload = self.get_enum_payload_ptr(parts[0], scrut_val, ctx)?;
                        let i64_type = ctx.module().get_context().i64_type();
                        let i64_ptr = i64_type.ptr_type(AddressSpace::default());
                        let payload_i64 = ctx
                            .builder()
                            .build_pointer_cast(payload, i64_ptr, "destr_i64")
                            .map_err(|_| {
                                LeoError::new(
                                    ErrorKind::Syntax,
                                    ErrorCode::CodegenLLVMError,
                                    "cast payload for destr failed".into(),
                                )
                            })?;
                        let payload_types = self
                            .enum_payload_types
                            .get(name)
                            .cloned()
                            .unwrap_or_default();
                        return self.bind_payload_fields(
                            bindings,
                            payload_i64,
                            ctx,
                            &payload_types,
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Get pointer to enum payload area (struct gep index 1) from scrutinee i64 value
    fn get_enum_payload_ptr<'a>(
        &mut self,
        enum_name: &str,
        scrut_val: inkwell::values::IntValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::PointerValue<'a>> {
        let struct_type = ctx.module().get_struct_type(enum_name).ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("enum {} not defined for destructuring", enum_name),
            )
        })?;
        let struct_ptr = struct_type.ptr_type(AddressSpace::default());
        let enum_ptr = ctx
            .builder()
            .build_int_to_ptr(scrut_val, struct_ptr, "destr_enum")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "int_to_ptr for destr failed".into(),
                )
            })?;
        ctx.builder()
            .build_struct_gep(enum_ptr, 1, "destr_payload")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "gep payload for destr failed".into(),
                )
            })
    }

    /// Load each payload field into an alloca and register as local variable
    fn bind_payload_fields<'a>(
        &mut self,
        bindings: &[Expr],
        payload_i64: inkwell::values::PointerValue<'a>,
        ctx: &mut LlvmContext<'a>,
        payload_types: &[String],
    ) -> LeoResult<()> {
        let i64_type = ctx.module().get_context().i64_type();
        for (j, binding) in bindings.iter().enumerate() {
            if let Expr::Ident(var_name, _) = binding {
                let idx = i64_type.const_int(j as u64, false);
                let field_ptr = unsafe {
                    ctx.builder()
                        .build_in_bounds_gep(payload_i64, &[idx], &format!("destr_{}", var_name))
                        .map_err(|_| {
                            LeoError::new(
                                ErrorKind::Syntax,
                                ErrorCode::CodegenLLVMError,
                                format!("gep binding {} failed", var_name),
                            )
                        })?
                };
                let loaded = ctx
                    .builder()
                    .build_load(field_ptr, &format!("destr_val_{}", var_name))
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            format!("load binding {} failed", var_name),
                        )
                    })?;
                let var_ptr = ctx
                    .builder()
                    .build_alloca(i64_type, &format!("destr_var_{}", var_name))
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            format!("alloca binding {} failed", var_name),
                        )
                    })?;
                ctx.builder().build_store(var_ptr, loaded).map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        format!("store binding {} failed", var_name),
                    )
                })?;
                ctx.register_variable(var_name.clone(), var_ptr);
                if let Some(field_type) = payload_types.get(j) {
                    if field_type == "str" || field_type == "string" {
                        ctx.register_type(var_name.clone(), LeoType::Str);
                    } else {
                        ctx.register_type(var_name.clone(), LeoType::from_str(field_type));
                    }
                }
            }
        }
        Ok(())
    }
}
