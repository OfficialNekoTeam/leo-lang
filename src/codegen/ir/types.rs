use super::*;
use crate::common::types::LeoType;
use inkwell::values::BasicValueEnum;

#[derive(Debug, Clone)]
pub struct TypedValue<'ctx> {
    pub value: BasicValueEnum<'ctx>,
    pub ty: LeoType,
}

impl<'ctx> TypedValue<'ctx> {
    pub fn new(value: BasicValueEnum<'ctx>, ty: LeoType) -> Self {
        Self { value, ty }
    }
}

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
                self.emit_abort(ctx)?;
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
            Expr::StructInit(name, fields, type_args, _) => {
                // If this is a generic struct with type args, instantiate it
                let _effective_name = if !type_args.is_empty()
                    && self.generic_structs.contains_key(name)
                {
                    self.instantiate_generic_struct(name, type_args, ctx)?
                } else {
                    name.clone()
                };
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

}
