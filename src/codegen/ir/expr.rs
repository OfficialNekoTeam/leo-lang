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
            Expr::Call(_, _, _) | Expr::Match(_, _, _) => {
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
            Expr::Call(callee, args, _) => {
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
                // FIXME: Proper type inference for index
                Ok(TypedValue::new(BasicValueEnum::IntValue(val), LeoType::I64))
            }
            Expr::Select(obj, field, _) => {
                let val = self.eval_select(obj, field, ctx)?;
                // FIXME: Proper type inference for select
                Ok(TypedValue::new(BasicValueEnum::IntValue(val), LeoType::I64))
            }
            Expr::Array(_, _) | Expr::ArrayRepeat(_, _, _) => {
                let val = self.eval_array_alloc(expr, ctx)?;
                Ok(TypedValue::new(BasicValueEnum::IntValue(val), LeoType::Ptr)) // Should be Array(..)
            }
            Expr::StructInit(_, _, _) => {
                let val = self.eval_struct_init(expr, ctx)?;
                Ok(TypedValue::new(BasicValueEnum::IntValue(val), LeoType::Ptr)) // Should be Struct(..)
            }
            Expr::Match(scrutinee, arms, _) => {
                let val = self.eval_match(scrutinee, arms, ctx)?;
                Ok(TypedValue::new(BasicValueEnum::IntValue(val), LeoType::I64)) // FIXME
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
        if target_type.is_float() && (tv.ty == LeoType::I64 || tv.ty == LeoType::I32 || tv.ty == LeoType::Bool) {
            return Ok(ctx.builder().build_signed_int_to_float(tv.value.into_int_value(), context.f64_type(), "arg.fptosi").map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "float conv fail".into()))?.into());
        }
        if (*target_type == LeoType::I64 || *target_type == LeoType::I32 || *target_type == LeoType::Bool) && tv.ty.is_float() {
            return Ok(ctx.builder().build_float_to_signed_int(tv.value.into_float_value(), context.i64_type(), "arg.sitofp").map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "int conv fail".into()))?.into());
        }
        if tv.ty == *target_type || (tv.value.is_pointer_value() && (target_type.is_string() || *target_type == LeoType::Ptr)) {
            return Ok(tv.value);
        }
        let val = tv.value.into_int_value(); // fallback for previous truncation logic

        match target_type {
            LeoType::Str | LeoType::Ptr => {
                let i8_ptr = context.i8_type().ptr_type(AddressSpace::default());
                ctx.builder()
                    .build_int_to_ptr(val, i8_ptr, "arg.as_ptr")
                    .map(BasicValueEnum::from)
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "int_to_ptr for arg failed".into(),
                        )
                    })
            }
            LeoType::F64 => {
                let f64_type = context.f64_type();
                ctx.builder()
                    .build_signed_int_to_float(val, f64_type, "arg.as_f64")
                    .map(BasicValueEnum::from)
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "int_to_float for arg failed".into(),
                        )
                    })
            }
            LeoType::Bool | LeoType::Char => {
                let target = if matches!(target_type, LeoType::Bool) {
                    context.i8_type()
                } else {
                    context.i8_type()
                };
                let truncated = ctx
                    .builder()
                    .build_int_truncate(val, target, "arg.trunc")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "truncate arg failed".into(),
                        )
                    })?;
                Ok(BasicValueEnum::from(truncated))
            }
            LeoType::I32 => {
                let truncated = ctx
                    .builder()
                    .build_int_truncate(val, context.i32_type(), "arg.trunc32")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "truncate arg to i32 failed".into(),
                        )
                    })?;
                Ok(BasicValueEnum::from(truncated))
            }
            _ => Ok(BasicValueEnum::from(val)),
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
            Expr::Call(callee, _, _) => {
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
                .map(|t| matches!(t, LeoType::F64))
                .unwrap_or(false),
            Expr::Call(callee, _, _) => {
                if let Expr::Ident(fn_name, _) = callee.as_ref() {
                    ctx.get_fn_return_type(fn_name)
                        .map(|t| matches!(t, LeoType::F64))
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
}
