use super::*;
use crate::codegen::ir::types::TypedValue;
use crate::ast::expr::{BinOp, Expr};
use crate::common::types::LeoType;
use crate::common::error::{ErrorCode, ErrorKind, LeoError, LeoResult};
use inkwell::AddressSpace;
use inkwell::values::BasicValueEnum;

impl IrBuilder {
    pub(super) fn eval_and_emit(&mut self, expr: &Expr, ctx: &mut LlvmContext) -> LeoResult<()> {
        match expr {
            Expr::String(s, _) => self.emit_puts(s, ctx),
            Expr::Call(_, _, _, _) | Expr::Match(_, _, _) => {
                let _ = self.eval_int(expr, ctx)?;
                Ok(())
            }
            Expr::Ident(name, _) => {
                let tv = self.load_ident(name, ctx)?;
                if tv.ty.is_string() || tv.ty.is_pointer() {
                    self.emit_print_str_ptr(tv.value.into_pointer_value(), ctx)?;
                } else if tv.ty.is_float() {
                    // fall back print for now, float printing will be properly added in Phase 2 stdlib
                    let as_int = ctx.builder().build_float_to_signed_int(tv.value.into_float_value(), ctx.module().get_context().i64_type(), "fptosi").map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "fptosi float".into()))?;
                    self.emit_print_int(as_int, ctx)?;
                } else {
                    self.emit_print_int(tv.value.into_int_value(), ctx)?;
                }
                Ok(())
            }
            _ => {
                let val = self.eval_int(expr, ctx)?;
                self.emit_print_int(val, ctx)?;
                Ok(())
            }
        }
    }

    /// Load int identifier value
    pub(super) fn load_ident<'a>(
        &mut self,
        name: &str,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<TypedValue<'a>> {
        let ptr = ctx.get_variable(name).ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("undefined variable: {}", name),
            )
        })?;
        let ty = ctx.get_type(name).cloned().unwrap_or(LeoType::I64);
        let loaded = ctx.builder().build_load(ptr, name).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("load failed for {}", name),
            )
        })?;
        Ok(TypedValue::new(loaded, ty))
    }

    pub(super) fn eval_expr<'a>(
        &mut self,
        expr: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<TypedValue<'a>> {
        match expr {
            Expr::Number(n, _) => Ok(TypedValue::new(
                BasicValueEnum::IntValue(ctx.module().get_context().i64_type().const_int(*n as u64, false)),
                LeoType::I64,
            )),
            Expr::Bool(b, _) => {
                let i1_type = ctx.module().get_context().bool_type();
                Ok(TypedValue::new(
                    BasicValueEnum::IntValue(i1_type.const_int(*b as u64, false)),
                    LeoType::Bool,
                ))
            }
            Expr::Char(c, _) => {
                let i8_type = ctx.module().get_context().i8_type();
                Ok(TypedValue::new(
                    BasicValueEnum::IntValue(i8_type.const_int(*c as u64, false)),
                    LeoType::Char,
                ))
            }
            Expr::Float(f, _) => {
                let f64_type = ctx.module().get_context().f64_type();
                Ok(TypedValue::new(
                    BasicValueEnum::FloatValue(f64_type.const_float(*f)),
                    LeoType::F64,
                ))
            }
            Expr::Ident(name, _) => self.load_ident(name, ctx),
            Expr::Binary(op, left, right, _) => {
                if let (Expr::Number(l, _), Expr::Number(r, _)) = (left.as_ref(), right.as_ref()) {
                    if let Some(folded) = Self::fold_constants(op, *l, *r) {
                        return Ok(TypedValue::new(
                            BasicValueEnum::IntValue(ctx.module().get_context().i64_type().const_int(folded as u64, false)),
                            LeoType::I64,
                        ));
                    }
                }
                if self.expr_is_float(left, ctx) || self.expr_is_float(right, ctx) {
                    return self.eval_float_binop(op, left, right, ctx);
                }
                if let Some(result) = self.try_string_compare(op, left, right, ctx)? {
                    return Ok(TypedValue::new(BasicValueEnum::IntValue(result), LeoType::Bool));
                }
                if let Some(result) = self.try_string_concat(op, left, right, ctx)? {
                    return Ok(TypedValue::new(BasicValueEnum::IntValue(result), LeoType::Str));
                }
                if matches!(op, BinOp::And | BinOp::Or) {
                    return self.eval_short_circuit(op, left, right, ctx);
                }
                let lv = self.eval_expr(left, ctx)?;
                let rv = self.eval_expr(right, ctx)?;
                self.emit_binop(op, lv, rv, ctx)
            }
            Expr::Unary(op, e, _) => {
                let val = self.eval_expr(e, ctx)?;
                self.emit_unop(op, val, ctx)
            }
            Expr::Call(callee, args, type_args, _) => {
                if !type_args.is_empty() {
                    if let Expr::Ident(name, span) = callee.as_ref() {
                        if self.generic_fns.contains_key(name) {
                            let mangled = self.instantiate_generic_fn(
                                name, type_args, ctx,
                            )?;
                            let mangled_callee = Expr::Ident(mangled, *span);
                            return self.eval_call(&mangled_callee, args, ctx);
                        }
                    }
                }
                self.eval_call(callee, args, ctx)
            }
            Expr::String(s, _) => {
                let gv = self.emit_string_global(s, ctx);
                let i8_ptr = ctx.module().get_context().i8_type().ptr_type(AddressSpace::default());
                let ptr = gv.as_pointer_value().const_cast(i8_ptr);
                Ok(TypedValue::new(BasicValueEnum::PointerValue(ptr), LeoType::Str))
            }
            Expr::Index(obj, idx, _) => {
                let val = self.eval_index(obj, idx, ctx)?;
                let elem_ty = self.infer_index_type(obj, ctx);
                Ok(TypedValue::new(BasicValueEnum::IntValue(val), elem_ty))
            }
            Expr::Select(obj, field, _) => {
                let val = self.eval_select(obj, field, ctx)?;
                let field_ty = self.infer_select_type(obj, field, ctx);
                Ok(TypedValue::new(BasicValueEnum::IntValue(val), field_ty))
            }
            Expr::Array(_, _) | Expr::ArrayRepeat(_, _, _) => {
                let val = self.eval_array_alloc(expr, ctx)?;
                let arr_ty = self.infer_array_type(expr, ctx);
                Ok(TypedValue::new(BasicValueEnum::IntValue(val), arr_ty))
            }
            Expr::StructInit(name, _, type_args, _) => {
                let val = self.eval_struct_init(expr, ctx)?;
                let struct_ty = if !type_args.is_empty() {
                    LeoType::Struct(Self::mangle_generic_name(name, type_args))
                } else {
                    LeoType::Struct(name.clone())
                };
                Ok(TypedValue::new(BasicValueEnum::IntValue(val), struct_ty))
            }
            Expr::Match(scrutinee, arms, _) => {
                let val = self.eval_match(scrutinee, arms, ctx)?;
                let match_ty = self.infer_match_type(arms, ctx);
                Ok(TypedValue::new(BasicValueEnum::IntValue(val), match_ty))
            }
            _ => Err(LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("cannot evaluate {:?} as TypedValue", expr),
            )),
        }
    }

    /// Evaluate integer expression (number, bool, binary, unary, call)
    /// LEGACY: This now wraps eval_expr and casts to i64
    pub fn eval_int<'a>(
        &mut self,
        expr: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        self.eval_expr_to_value(expr, ctx)
    }

    /// Load identifier as TypedValue
    pub fn eval_expr_to_value<'a>(
        &mut self,
        expr: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let tv = self.eval_expr(expr, ctx)?;
        let i64_type = ctx.module().get_context().i64_type();
        match tv.value {
            BasicValueEnum::IntValue(iv) => {
                let bit_width = iv.get_type().get_bit_width();
                if bit_width < 64 {
                    ctx.builder().build_int_z_extend(iv, i64_type, "zext64").map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "zext to i64 failed".into(),
                        )
                    })
                } else if bit_width > 64 {
                    ctx.builder().build_int_truncate(iv, i64_type, "trunc64").map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "trunc to i64 failed".into(),
                        )
                    })
                } else {
                    Ok(iv)
                }
            }
            BasicValueEnum::PointerValue(pv) => ctx
                .builder()
                .build_ptr_to_int(pv, i64_type, "ptr2int")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "ptr_to_int failed".into(),
                    )
                }),
            BasicValueEnum::FloatValue(fv) => ctx
                .builder()
                .build_float_to_signed_int(fv, i64_type, "fptosi")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "float to int failed".into(),
                    )
                }),
            _ => Err(LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("cannot cast {:?} to i64", tv.ty),
            )),
        }
    }

    /// Evaluate function call: check builtins first, then user functions
    pub(super) fn eval_call<'a>(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<TypedValue<'a>> {
        match callee {
            Expr::Ident(name, _) => {
                let func_name = name.clone();
                if func_name.contains("::") {
                    let ptr = self.eval_enum_constructor(&func_name, args, ctx)?;
                    return Ok(TypedValue::new(BasicValueEnum::IntValue(ptr), LeoType::Ptr));
                }
                if let Some(res) = self.try_dispatch_builtin(func_name.as_str(), args, ctx)? {
                    let ret_ty = ctx.get_fn_return_type(func_name.as_str()).cloned().unwrap_or(LeoType::I64);
                    return Ok(TypedValue::new(BasicValueEnum::IntValue(res), ret_ty));
                }
                let func = ctx.get_function(&func_name).ok_or_else(|| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        format!("undefined function: {}", func_name),
                    )
                })?;
                let mut arg_values: Vec<_> = Vec::new();
                let fn_param_types = ctx.get_fn_param_types(&func_name).cloned();
                for (i, arg) in args.iter().enumerate() {
                    let tv = self.eval_expr(arg, ctx)?;
                    let coerced = if let Some(ref ptypes) = fn_param_types {
                        if let Some(param_type) = ptypes.get(i) {
                            self.coerce_arg_to_type(tv, param_type, ctx)?
                        } else {
                            tv.value
                        }
                    } else {
                        tv.value
                    };
                    arg_values.push(coerced.into());
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
let ret_ty = ctx.get_fn_return_type(&func_name).cloned().unwrap_or(LeoType::I64);
                Ok(TypedValue::new(ret, ret_ty))
            }
            Expr::Select(obj, method, _) => self.eval_method_call(obj, method, args, ctx),
            _ => Err(LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "only direct function calls supported".into(),
            )),
        }
    }

    fn coerce_arg_to_type<'a>(
        &self,
        tv: TypedValue<'a>,
        target_type: &LeoType,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<BasicValueEnum<'a>> {
        let context = ctx.module().get_context();
        // Same type — no coercion needed
        if tv.ty == *target_type {
            return Ok(tv.value);
        }
        // Pointer passthrough
        if tv.value.is_pointer_value()
            && (target_type.is_string() || *target_type == LeoType::Ptr)
        {
            return Ok(tv.value);
        }
        // Int → Float
        if target_type.is_float() && tv.ty.is_integer() {
            let float_ty = if *target_type == LeoType::F32 {
                context.f32_type()
            } else {
                context.f64_type()
            };
            return ctx
                .builder()
                .build_signed_int_to_float(
                    tv.value.into_int_value(),
                    float_ty,
                    "arg.itof",
                )
                .map(BasicValueEnum::from)
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "int_to_float coercion failed".into(),
                    )
                });
        }
        // Float → Int
        if target_type.is_integer() && tv.ty.is_float() {
            let int_ty = Self::leo_int_type(target_type, &context);
            return ctx
                .builder()
                .build_float_to_signed_int(
                    tv.value.into_float_value(),
                    int_ty,
                    "arg.ftoi",
                )
                .map(BasicValueEnum::from)
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "float_to_int coercion failed".into(),
                    )
                });
        }
        // F32 ↔ F64
        if tv.ty.is_float() && target_type.is_float() {
            let fv = tv.value.into_float_value();
            if *target_type == LeoType::F64 {
                return ctx
                    .builder()
                    .build_float_ext(fv, context.f64_type(), "arg.fext")
                    .map(BasicValueEnum::from)
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "float ext failed".into(),
                        )
                    });
            } else {
                return ctx
                    .builder()
                    .build_float_trunc(fv, context.f32_type(), "arg.ftrunc")
                    .map(BasicValueEnum::from)
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "float trunc failed".into(),
                        )
                    });
            }
        }
        // Int → Ptr
        if matches!(target_type, LeoType::Str | LeoType::Ptr) && tv.ty.is_integer() {
            let i8_ptr = context.i8_type().ptr_type(AddressSpace::default());
            return ctx
                .builder()
                .build_int_to_ptr(tv.value.into_int_value(), i8_ptr, "arg.as_ptr")
                .map(BasicValueEnum::from)
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "int_to_ptr for arg failed".into(),
                    )
                });
        }
        // Int → Int (trunc or zext)
        if target_type.is_integer() && tv.ty.is_integer() {
            let val = tv.value.into_int_value();
            let target_llvm = Self::leo_int_type(target_type, &context);
            let src_bits = val.get_type().get_bit_width();
            let dst_bits = target_llvm.get_bit_width();
            if src_bits == dst_bits {
                return Ok(BasicValueEnum::from(val));
            } else if src_bits > dst_bits {
                return ctx
                    .builder()
                    .build_int_truncate(val, target_llvm, "arg.trunc")
                    .map(BasicValueEnum::from)
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "int truncate failed".into(),
                        )
                    });
            } else {
                // Widen: sign decided by SOURCE type, not target type
                let is_unsigned = matches!(
                    tv.ty,
                    LeoType::U8 | LeoType::U16 | LeoType::U32 | LeoType::U64
                );
                if is_unsigned {
                    return ctx
                        .builder()
                        .build_int_z_extend(val, target_llvm, "arg.zext")
                        .map(BasicValueEnum::from)
                        .map_err(|_| {
                            LeoError::new(
                                ErrorKind::Syntax,
                                ErrorCode::CodegenLLVMError,
                                "int zext failed".into(),
                            )
                        });
                } else {
                    return ctx
                        .builder()
                        .build_int_s_extend(val, target_llvm, "arg.sext")
                        .map(BasicValueEnum::from)
                        .map_err(|_| {
                            LeoError::new(
                                ErrorKind::Syntax,
                                ErrorCode::CodegenLLVMError,
                                "int sext failed".into(),
                            )
                        });
                }
            }
        }
        // Fallback — pass through unchanged
        Ok(tv.value)
    }

    /// Map LeoType integer variants to LLVM IntType.
    fn leo_int_type<'ctx>(
        ty: &LeoType,
        context: &inkwell::context::ContextRef<'ctx>,
    ) -> inkwell::types::IntType<'ctx> {
        match ty {
            LeoType::I8 | LeoType::U8 | LeoType::Char => context.i8_type(),
            LeoType::I16 | LeoType::U16 => context.i16_type(),
            LeoType::I32 | LeoType::U32 => context.i32_type(),
            LeoType::Bool => context.bool_type(),
            _ => context.i64_type(), // I64, U64, and fallback
        }
    }


    pub(super) fn eval_method_call<'a>(
        &mut self,
        obj: &Expr,
        method: &str,
        _args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<TypedValue<'a>> {
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        match method {
            "len" => match obj {
                Expr::Ident(name, _) => {
                    if let Some(size) = self.array_sizes.get(name).copied() {
                        if self.is_string_var(name, ctx) {
                            let len_val = self.runtime_strlen(name, ctx)?;
                            return Ok(TypedValue::new(BasicValueEnum::IntValue(len_val), LeoType::I64));
                        }
                        return Ok(TypedValue::new(BasicValueEnum::IntValue(i64_type.const_int(size as u64, false)), LeoType::I64));
                    }
                    if self.is_string_var(name, ctx) {
                        let len_val = self.runtime_strlen(name, ctx)?;
                        return Ok(TypedValue::new(BasicValueEnum::IntValue(len_val), LeoType::I64));
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
                        return Ok(TypedValue::new(BasicValueEnum::IntValue(result
                            .try_as_basic_value()
                            .left()
                            .ok_or_else(|| {
                                LeoError::new(
                                    ErrorKind::Syntax,
                                    ErrorCode::CodegenLLVMError,
                                    "strlen void".into(),
                                )
                            })?
                            .into_int_value()), LeoType::I64));
                    }
                    Err(LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        ".len() on select: not a string field".into(),
                    ))
                }
                Expr::String(s, _) => Ok(TypedValue::new(BasicValueEnum::IntValue(i64_type.const_int(s.len() as u64, false)), LeoType::I64)),
                Expr::Array(elems, _) => Ok(TypedValue::new(BasicValueEnum::IntValue(i64_type.const_int(elems.len() as u64, false)), LeoType::I64)),
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
                            let method_param_types = ctx.get_fn_param_types(&mangled).cloned();
                            for (i, arg) in _args.iter().enumerate() {
                                let tv = self.eval_expr(arg, ctx)?;
                                let coerced = if let Some(ref ptypes) = method_param_types {
                                    if let Some(ptype) = ptypes.get(i + 1) {
                                        self.coerce_arg_to_type(tv, ptype, ctx)?
                                    } else {
                                        tv.value
                                    }
                                } else {
                                    tv.value
                                };
                                arg_values.push(coerced);
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
                            let ret_ty = ctx.get_fn_return_type(&mangled).cloned().unwrap_or(LeoType::I64);
                            return Ok(TypedValue::new(ret, ret_ty));
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



    pub(super) fn expr_is_string(&self, expr: &Expr, ctx: &LlvmContext) -> bool {
        match expr {
            Expr::String(_, _) => true,
            Expr::Ident(name, _) => ctx.is_string_var(name),
            Expr::Call(callee, _, _, _) => {
                if let Expr::Ident(fn_name, _) = callee.as_ref() {
                    match fn_name.as_str() {
                        "char_to_str" | "to_string" | "str_concat" | "str_slice" | "file_read" => {
                            true
                        }
                        _ => ctx
                            .get_fn_return_type(fn_name)
                            .map(|t| t.is_string())
                            .unwrap_or(false),
                    }
                } else {
                    false
                }
            }
            Expr::Binary(BinOp::Add, left, right, _) => {
                self.expr_is_string(left, ctx) || self.expr_is_string(right, ctx)
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



    fn expr_is_float(&self, expr: &Expr, ctx: &LlvmContext) -> bool {
        match expr {
            Expr::Float(_, _) => true,
            Expr::Ident(name, _) => ctx
                .get_type(name)
                .map(|t| t.is_float())
                .unwrap_or(false),
            Expr::Call(callee, _, _, _) => {
                if let Expr::Ident(fn_name, _) = callee.as_ref() {
                    ctx.get_fn_return_type(fn_name)
                        .map(|t| t.is_float())
                        .unwrap_or(false)
                } else {
                    false
                }
            }
            Expr::Binary(_, left, right, _) => {
                self.expr_is_float(left, ctx) || self.expr_is_float(right, ctx)
            }
            _ => false,
        }
    }



    pub(super) fn is_string_var(&self, name: &str, ctx: &LlvmContext) -> bool {
        ctx.is_string_var(name)
    }

    fn select_is_string(&self, obj: &Expr, field: &str, ctx: &LlvmContext) -> bool {
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
        ctx.is_string_var(field)
    }

    /// Infer the element type of an index expression (arr[i] or s[i]).
    fn infer_index_type(&self, obj: &Expr, ctx: &LlvmContext) -> LeoType {
        if self.expr_is_string(obj, ctx) {
            return LeoType::Char;
        }
        if let Expr::Ident(name, _) = obj {
            if let Some(ty) = ctx.get_type(name) {
                return match ty {
                    LeoType::Array(elem, _) => *elem.clone(),
                    LeoType::Vec(elem) => *elem.clone(),
                    LeoType::Str => LeoType::Char,
                    _ => LeoType::I64,
                };
            }
        }
        LeoType::I64
    }

    /// Infer the type of a field access (obj.field).
    fn infer_select_type(
        &self,
        obj: &Expr,
        field: &str,
        _ctx: &LlvmContext,
    ) -> LeoType {
        if field == "len" {
            return LeoType::I64;
        }
        if let Expr::Ident(var_name, _) = obj {
            if let Some(struct_name) = self.var_types.get(var_name) {
                if let Some(field_types) = self.struct_field_types.get(struct_name) {
                    if let Some(fields) = self.struct_fields.get(struct_name) {
                        if let Some(idx) = fields.iter().position(|f| f == field) {
                            if let Some(ty_str) = field_types.get(idx) {
                                return LeoType::from_str(ty_str);
                            }
                        }
                    }
                }
            }
        }
        LeoType::I64
    }

    /// Infer the return type of a match expression from its first arm body.
    fn infer_match_type(
        &self,
        arms: &[(Expr, Expr)],
        ctx: &LlvmContext,
    ) -> LeoType {
        if let Some((_pattern, body)) = arms.first() {
            return self.infer_expr_type(body, ctx);
        }
        LeoType::I64
    }

    /// Infer the type of an array literal or array repeat expression.
    fn infer_array_type(
        &self,
        expr: &Expr,
        ctx: &LlvmContext,
    ) -> LeoType {
        match expr {
            Expr::Array(elems, _) => {
                let elem_ty = if let Some(first) = elems.first() {
                    self.infer_expr_type(first, ctx)
                } else {
                    LeoType::I64
                };
                LeoType::Array(Box::new(elem_ty), elems.len())
            }
            Expr::ArrayRepeat(val, count, _) => {
                let elem_ty = self.infer_expr_type(val, ctx);
                let size = if let Expr::Number(n, _) = count.as_ref() {
                    *n as usize
                } else {
                    0
                };
                LeoType::Array(Box::new(elem_ty), size)
            }
            _ => LeoType::Ptr,
        }
    }

    /// Lightweight type inference for an expression (no code generation).
    fn infer_expr_type(
        &self,
        expr: &Expr,
        ctx: &LlvmContext,
    ) -> LeoType {
        match expr {
            Expr::Number(_, _) => LeoType::I64,
            Expr::Float(_, _) => LeoType::F64,
            Expr::Bool(_, _) => LeoType::Bool,
            Expr::Char(_, _) => LeoType::Char,
            Expr::String(_, _) => LeoType::Str,
            Expr::Ident(name, _) => {
                ctx.get_type(name).cloned().unwrap_or(LeoType::I64)
            }
            Expr::Call(callee, _, _, _) => {
                if let Expr::Ident(fn_name, _) = callee.as_ref() {
                    ctx.get_fn_return_type(fn_name)
                        .cloned()
                        .unwrap_or(LeoType::I64)
                } else {
                    LeoType::I64
                }
            }
            Expr::Binary(op, left, right, _) => {
                match op {
                    BinOp::Eq | BinOp::Ne | BinOp::Lt
                    | BinOp::Le | BinOp::Gt | BinOp::Ge => LeoType::Bool,
                    BinOp::And | BinOp::Or => LeoType::Bool,
                    BinOp::Add => {
                        if self.expr_is_string(left, ctx)
                            || self.expr_is_string(right, ctx)
                        {
                            LeoType::Str
                        } else {
                            LeoType::I64
                        }
                    }
                    _ => LeoType::I64,
                }
            }
            Expr::StructInit(name, _, _, _) => LeoType::Struct(name.clone()),
            Expr::Array(elems, _) => {
                let elem_ty = if let Some(first) = elems.first() {
                    self.infer_expr_type(first, ctx)
                } else {
                    LeoType::I64
                };
                LeoType::Array(Box::new(elem_ty), elems.len())
            }
            _ => LeoType::I64,
        }
    }
}
