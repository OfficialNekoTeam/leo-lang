use super::*;

impl IrBuilder {
    pub(super) fn declare_c_runtime(&mut self, ctx: &mut LlvmContext) {
        let i8_ptr = ctx
            .module()
            .get_context()
            .i8_type()
            .ptr_type(AddressSpace::default());
        let i64_type = ctx.module().get_context().i64_type();
        let i32_type = ctx.module().get_context().i32_type();
        let void_type = ctx.module().get_context().void_type();
        ctx.module_mut()
            .add_function("puts", i8_ptr.fn_type(&[], false), None);
        ctx.module_mut().add_function(
            "printf",
            i32_type.fn_type(&[i8_ptr.into(), i64_type.into()], true),
            None,
        );
        ctx.module_mut()
            .add_function("strlen", i64_type.fn_type(&[i8_ptr.into()], false), None);
        ctx.module_mut()
            .add_function("malloc", i8_ptr.fn_type(&[i64_type.into()], false), None);
        ctx.module_mut().add_function(
            "memcpy",
            i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_type.into()], false),
            None,
        );
        ctx.module_mut().add_function(
            "strcpy",
            i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false),
            None,
        );
        ctx.module_mut().add_function(
            "strcat",
            i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false),
            None,
        );
        ctx.module_mut().add_function(
            "strcmp",
            i32_type.fn_type(&[i8_ptr.into(), i8_ptr.into()], false),
            None,
        );
        ctx.module_mut().add_function(
            "strstr",
            i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false),
            None,
        );
        ctx.module_mut().add_function(
            "realloc",
            i8_ptr.fn_type(&[i8_ptr.into(), i64_type.into()], false),
            None,
        );
        ctx.module_mut().add_function(
            "fopen",
            i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false),
            None,
        );
        ctx.module_mut().add_function(
            "fread",
            i64_type.fn_type(
                &[
                    i8_ptr.into(),
                    i64_type.into(),
                    i64_type.into(),
                    i8_ptr.into(),
                ],
                false,
            ),
            None,
        );
        ctx.module_mut().add_function(
            "fwrite",
            i64_type.fn_type(
                &[
                    i8_ptr.into(),
                    i64_type.into(),
                    i64_type.into(),
                    i8_ptr.into(),
                ],
                false,
            ),
            None,
        );
        ctx.module_mut()
            .add_function("fclose", i32_type.fn_type(&[i8_ptr.into()], false), None);
        ctx.module_mut().add_function(
            "fseek",
            i32_type.fn_type(&[i8_ptr.into(), i64_type.into(), i32_type.into()], false),
            None,
        );
        ctx.module_mut()
            .add_function("ftell", i64_type.fn_type(&[i8_ptr.into()], false), None);
        ctx.module_mut()
            .add_function("free", void_type.fn_type(&[i8_ptr.into()], false), None);
        ctx.module_mut().add_function(
            "snprintf",
            i32_type.fn_type(&[i8_ptr.into(), i64_type.into(), i8_ptr.into()], true),
            None,
        );
    }

    pub(super) fn build_stmt(&mut self, stmt: &Stmt, ctx: &mut LlvmContext) -> LeoResult<()> {
        match stmt {
            Stmt::Function(name, params, ret, body, _)
            | Stmt::AsyncFunction(name, params, ret, body, _) => {
                self.build_fn(name, params, ret, body, ctx)?;
            }
            Stmt::Const(name, ty, expr, _span) => {
                self.build_const(name, ty, expr, ctx)?;
            }
            Stmt::Struct(name, fields, _) => {
                let context = ctx.module().get_context();
                let struct_type = context.opaque_struct_type(name);
                let field_types: Vec<BasicTypeEnum> = fields
                    .iter()
                    .map(|(_, ty)| Self::llvm_type(ty, ctx))
                    .collect();
                struct_type.set_body(&field_types, false);
                let field_names: Vec<String> = fields.iter().map(|(n, _)| n.clone()).collect();
                let field_type_names: Vec<String> =
                    fields.iter().map(|(_, ty)| ty.clone()).collect();
                self.struct_fields.insert(name.clone(), field_names);
                self.struct_field_types
                    .insert(name.clone(), field_type_names);
            }
            Stmt::Enum(name, variants, _) => {
                let context = ctx.module().get_context();
                let i32_type = context.i32_type();
                let max_payload: u32 = variants
                    .iter()
                    .map(|(_, payload)| {
                        if payload.is_empty() {
                            0
                        } else {
                            payload.len() as u32 * 8
                        }
                    })
                    .max()
                    .unwrap_or(0);
                let payload_type = if max_payload > 0 {
                    context.i8_type().array_type(max_payload).into()
                } else {
                    context.i8_type().array_type(1).into()
                };
                let enum_struct = context.opaque_struct_type(name);
                enum_struct.set_body(&[i32_type.into(), payload_type], false);
                let variant_names: Vec<String> = variants.iter().map(|(n, _)| n.clone()).collect();
                ctx.register_enum(name.clone(), variant_names);
                for (vname, payload) in variants {
                    let qualified = format!("{}::{}", name, vname);
                    let types: Vec<String> = payload
                        .iter()
                        .map(|e| {
                            if let Expr::Ident(t, _) = e {
                                t.clone()
                            } else {
                                "i64".to_string()
                            }
                        })
                        .collect();
                    self.enum_payload_types.insert(qualified, types);
                }
            }
            Stmt::Impl(struct_name, _trait, methods, _) => {
                for method in methods {
                    if let Stmt::Function(name, params, ret, _, _) = method {
                        let mangled = format!("{}_{}", struct_name, name);
                        self.methods
                            .insert((struct_name.clone(), name.clone()), mangled.clone());
                        Self::declare_fn(&mangled, params, ret, ctx);
                    }
                }
                for method in methods {
                    if let Stmt::Function(name, params, ret, body, _span) = method {
                        let mangled = format!("{}_{}", struct_name, name);
                        self.build_fn(&mangled, params, ret, body, ctx)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn declare_fn(
        name: &str,
        params: &[(String, String)],
        ret: &Option<String>,
        ctx: &mut LlvmContext,
    ) {
        if ctx.module().get_function(name).is_some() {
            return;
        }
        let context = ctx.module().get_context();
        let is_main = name == "main";
        let param_types: Vec<BasicTypeEnum> = params
            .iter()
            .map(|(_, pty)| Self::llvm_type(pty, ctx))
            .collect();
        let param_meta: Vec<_> = param_types.iter().map(|t| (*t).into()).collect();
        let fn_type = if is_main {
            context.i32_type().fn_type(&param_meta, false)
        } else {
            let ret_type = ret
                .as_deref()
                .map(|r| Self::llvm_type(r, ctx))
                .unwrap_or_else(|| context.i64_type().into());
            match ret_type {
                BasicTypeEnum::IntType(it) => it.fn_type(&param_meta, false),
                BasicTypeEnum::FloatType(ft) => ft.fn_type(&param_meta, false),
                BasicTypeEnum::PointerType(pt) => pt.fn_type(&param_meta, false),
                _ => context.i64_type().fn_type(&param_meta, false),
            }
        };
        let fv = ctx.module_mut().add_function(name, fn_type, None);
        ctx.register_function(name.to_string(), fv);
        let ret_leo = if is_main {
            LeoType::I32
        } else {
            ret.as_deref()
                .map(LeoType::from_str)
                .unwrap_or(LeoType::I64)
        };
        ctx.register_fn_return_type(name.to_string(), ret_leo);
        let param_leo_types: Vec<LeoType> = params
            .iter()
            .map(|(_, pty)| LeoType::from_str(pty))
            .collect();
        ctx.register_fn_param_types(name.to_string(), param_leo_types);
    }

    pub(super) fn build_fn(
        &mut self,
        name: &str,
        params: &[(String, String)],
        ret: &Option<String>,
        body: &[Stmt],
        ctx: &mut LlvmContext,
    ) -> LeoResult<()> {
        ctx.clear_variables();
        let context = ctx.module().get_context();
        let is_main = name == "main";

        Self::declare_fn(name, params, ret, ctx);
        let function = ctx.get_function(name).ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("function {} not found after declare", name),
            )
        })?;

        let param_types: Vec<BasicTypeEnum> = params
            .iter()
            .map(|(_, pty)| Self::llvm_type(pty, ctx))
            .collect();

        if !is_main && body.len() <= 3 {
            let always_inline = context.create_enum_attribute(
                inkwell::attributes::Attribute::get_named_enum_kind_id("alwaysinline"),
                0,
            );
            function.add_attribute(AttributeLoc::Function, always_inline);
        }

        let entry = context.append_basic_block(function, "entry");
        ctx.builder().position_at_end(entry);
        ctx.register_function(name.to_string(), function);
        ctx.set_current_fn(function);

        for (i, (pname, pty)) in params.iter().enumerate() {
            let ptr = ctx
                .builder()
                .build_alloca(param_types[i], pname)
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        format!("alloca param {} failed", pname),
                    )
                })?;
            let param_val = function.get_nth_param(i as u32).ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    format!("param {} not found", pname),
                )
            })?;
            ctx.builder().build_store(ptr, param_val).map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    format!("store param {} failed", pname),
                )
            })?;
            ctx.register_variable(pname.clone(), ptr);
            let param_leo_type = LeoType::from_str(pty);
            ctx.register_type(pname.clone(), param_leo_type);
            if self.struct_fields.contains_key(pty) {
                self.var_types.insert(pname.clone(), pty.clone());
            }
        }

        for (idx, stmt) in body.iter().enumerate() {
            let is_last = idx == body.len() - 1;
            match stmt {
                Stmt::Expr(expr) if !is_main => {
                    if is_last {
                        if !Self::block_is_terminated(ctx) {
                            let val = self.eval_expr_to_value(expr, ctx)?;
                            self.build_return_with(val, ctx)?;
                        }
                    } else {
                        let _ = self.eval_int(expr, ctx)?;
                    }
                }
                _ => self.build_body_stmt(stmt, ctx)?,
            }
        }

        if !Self::block_is_terminated(ctx) {
            if is_main {
                let zero = context.i32_type().const_int(0, false);
                ctx.builder().build_return(Some(&zero)).map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "return failed".into(),
                    )
                })?;
            } else {
                let zero = context.i64_type().const_int(0, false);
                ctx.builder().build_return(Some(&zero)).map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "return failed".into(),
                    )
                })?;
            }
        }
        Ok(())
    }

    pub(super) fn block_is_terminated(ctx: &LlvmContext) -> bool {
        ctx.builder()
            .get_insert_block()
            .map_or(true, |bb| bb.get_terminator().is_some())
    }

    pub(super) fn emit_branch(
        &mut self,
        target: inkwell::basic_block::BasicBlock,
        ctx: &mut LlvmContext,
    ) -> LeoResult<()> {
        if Self::block_is_terminated(ctx) {
            return Ok(());
        }
        ctx.builder()
            .build_unconditional_branch(target)
            .map(|_| ())
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "branch failed".into(),
                )
            })
    }

    pub(super) fn fold_constants(op: &BinOp, l: i64, r: i64) -> Option<i64> {
        match op {
            BinOp::Add => Some(l.wrapping_add(r)),
            BinOp::Sub => Some(l.wrapping_sub(r)),
            BinOp::Mul => Some(l.wrapping_mul(r)),
            BinOp::Div | BinOp::Mod if r == 0 => None,
            BinOp::Div => Some(l / r),
            BinOp::Mod => Some(l % r),
            BinOp::Eq => Some(if l == r { 1 } else { 0 }),
            BinOp::Ne => Some(if l != r { 1 } else { 0 }),
            BinOp::Lt => Some(if l < r { 1 } else { 0 }),
            BinOp::Le => Some(if l <= r { 1 } else { 0 }),
            BinOp::Gt => Some(if l > r { 1 } else { 0 }),
            BinOp::Ge => Some(if l >= r { 1 } else { 0 }),
            _ => None,
        }
    }

    pub(super) fn build_return_with<'a>(
        &mut self,
        val: inkwell::values::IntValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<()> {
        let context = ctx.module().get_context();
        let ret_val: BasicValueEnum = if let Some(fv) = ctx.current_fn() {
            let fn_type = fv.get_type();
            match fn_type.get_return_type() {
                Some(BasicTypeEnum::IntType(int_ty)) if int_ty == context.i32_type() => {
                    let trunc = ctx
                        .builder()
                        .build_int_truncate(val, context.i32_type(), "ret.trunc")
                        .map_err(|_| {
                            LeoError::new(
                                ErrorKind::Syntax,
                                ErrorCode::CodegenLLVMError,
                                "truncate failed".into(),
                            )
                        })?;
                    trunc.into()
                }
                _ => val.into(),
            }
        } else {
            val.into()
        };
        ctx.builder().build_return(Some(&ret_val)).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "return failed".into(),
            )
        })?;
        Ok(())
    }

    pub fn llvm_type<'ctx>(ty: &str, ctx: &LlvmContext<'ctx>) -> BasicTypeEnum<'ctx> {
        let context = ctx.module().get_context();
        match ty {
            "i8" | "u8" | "char" => context.i8_type().into(),
            "i16" | "u16" => context.i16_type().into(),
            "i32" => context.i32_type().into(),
            "i64" => context.i64_type().into(),
            "f32" => context.f32_type().into(),
            "f64" => context.f64_type().into(),
            "bool" => context.i8_type().into(),
            "str" | "string" => context.i8_type().ptr_type(AddressSpace::default()).into(),
            _ => {
                if ctx.module().get_struct_type(ty).is_some() {
                    context.i8_type().ptr_type(AddressSpace::default()).into()
                } else {
                    context.i64_type().into()
                }
            }
        }
    }

    pub fn leo_type_to_llvm<'ctx>(leo: &LeoType, ctx: &LlvmContext<'ctx>) -> BasicTypeEnum<'ctx> {
        let context = ctx.module().get_context();
        match leo {
            LeoType::I64 => context.i64_type().into(),
            LeoType::I32 => context.i32_type().into(),
            LeoType::F64 => context.f64_type().into(),
            LeoType::Bool => context.i8_type().into(),
            LeoType::Char => context.i8_type().into(),
            LeoType::Str | LeoType::Ptr => {
                context.i8_type().ptr_type(AddressSpace::default()).into()
            }
            LeoType::Struct(_) => context.i8_type().ptr_type(AddressSpace::default()).into(),
            LeoType::Enum(_) => context.i8_type().ptr_type(AddressSpace::default()).into(),
            LeoType::Vec(_) => context.i8_type().ptr_type(AddressSpace::default()).into(),
            LeoType::Array(elem, n) => {
                let elem_type = Self::leo_type_to_llvm(elem, ctx);
                match elem_type {
                    BasicTypeEnum::IntType(it) => it.array_type(*n as u32).into(),
                    BasicTypeEnum::FloatType(ft) => ft.array_type(*n as u32).into(),
                    _ => context.i64_type().array_type(*n as u32).into(),
                }
            }
            LeoType::Fn(_, ret) => Self::leo_type_to_llvm(ret, ctx),
            LeoType::Unit => context.i64_type().into(),
        }
    }
}
