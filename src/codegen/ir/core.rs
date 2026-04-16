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
            Stmt::Function(name, params, ret, body, type_params, _)
            | Stmt::AsyncFunction(name, params, ret, body, type_params, _) => {
                if !type_params.is_empty() {
                    // Store generic function definition for later instantiation
                    self.generic_fns.insert(
                        name.clone(),
                        super::mono::GenericFnDef {
                            type_params: type_params.clone(),
                            params: params.clone(),
                            ret: ret.clone(),
                            body: body.clone(),
                        },
                    );
                } else {
                    self.build_fn(name, params, ret, body, ctx)?;
                }
            }
            Stmt::Const(name, ty, expr, _span) => {
                self.build_const(name, ty, expr, ctx)?;
            }
            Stmt::Struct(name, fields, type_params, _) => {
                if !type_params.is_empty() {
                    // Store generic struct definition for later instantiation
                    self.generic_structs.insert(
                        name.clone(),
                        super::mono::GenericStructDef {
                            type_params: type_params.clone(),
                            fields: fields.clone(),
                        },
                    );
                } else {
                    let context = ctx.module().get_context();
                    let struct_type = context.opaque_struct_type(name);
                    let field_types: Vec<BasicTypeEnum> = fields
                        .iter()
                        .map(|(_, ty)| Self::llvm_type(ty, ctx))
                        .collect();
                    struct_type.set_body(&field_types, false);
                    let field_names: Vec<String> =
                        fields.iter().map(|(n, _)| n.clone()).collect();
                    let field_type_names: Vec<String> =
                        fields.iter().map(|(_, ty)| ty.clone()).collect();
                    self.struct_fields.insert(name.clone(), field_names);
                    self.struct_field_types
                        .insert(name.clone(), field_type_names);
                }
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
            Stmt::Impl(struct_name, _trait, methods, _, _) => {
                for method in methods {
                    if let Stmt::Function(name, params, ret, _, _, _) = method {
                        let mangled = format!("{}_{}", struct_name, name);
                        self.methods
                            .insert((struct_name.clone(), name.clone()), mangled.clone());
                        Self::declare_fn(&mangled, params, ret, ctx);
                    }
                }
                for method in methods {
                    if let Stmt::Function(name, params, ret, body, _, _span) = method {
                        let mangled = format!("{}_{}", struct_name, name);
                        self.build_fn(&mangled, params, ret, body, ctx)?;
                    }
                }
            }
            _ => {}
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
        val: inkwell::values::BasicValueEnum<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<()> {
        let context = ctx.module().get_context();
        let ret_val: BasicValueEnum = if let Some(fv) = ctx.current_fn() {
            let fn_type = fv.get_type();
            match fn_type.get_return_type() {
                Some(BasicTypeEnum::IntType(int_ty)) if int_ty == context.i32_type() => {
                    let trunc = ctx
                        .builder()
                        .build_int_truncate(val.into_int_value(), context.i32_type(), "ret.trunc")
                        .map_err(|_| {
                            LeoError::new(
                                ErrorKind::Syntax,
                                ErrorCode::CodegenLLVMError,
                                "truncate failed".into(),
                            )
                        })?;
                    trunc.into()
                }
                _ => val,
            }
        } else {
            val
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
            "bool" => context.bool_type().into(),
            "str" | "string" => context.i8_type().ptr_type(AddressSpace::default()).into(),
            "unit" | "void" => context.struct_type(&[], false).into(),
            _ => {
                if let Some(st) = ctx.module().get_struct_type(ty) {
                    st.ptr_type(AddressSpace::default()).into()
                } else {
                    context.i8_type().ptr_type(AddressSpace::default()).into()
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
            LeoType::Bool => context.bool_type().into(),
            LeoType::Char => context.i8_type().into(),
            LeoType::Str | LeoType::Ptr => {
                context.i8_type().ptr_type(AddressSpace::default()).into()
            }
            LeoType::Struct(name) => {
                if let Some(st) = ctx.module().get_struct_type(name) {
                    st.ptr_type(AddressSpace::default()).into()
                } else {
                    context.i8_type().ptr_type(AddressSpace::default()).into()
                }
            }
            LeoType::Enum(name) => {
                if let Some(st) = ctx.module().get_struct_type(name) {
                    st.ptr_type(AddressSpace::default()).into()
                } else {
                    context.i8_type().ptr_type(AddressSpace::default()).into()
                }
            }
            LeoType::Vec(_) => context.i8_type().ptr_type(AddressSpace::default()).into(),
            LeoType::Array(elem, n) => {
                let elem_type = Self::leo_type_to_llvm(elem, ctx);
                match elem_type {
                    BasicTypeEnum::IntType(it) => it.array_type(*n as u32).into(),
                    BasicTypeEnum::FloatType(ft) => ft.array_type(*n as u32).into(),
                    BasicTypeEnum::PointerType(pt) => pt.array_type(*n as u32).into(),
                    BasicTypeEnum::StructType(st) => st.array_type(*n as u32).into(),
                    _ => context.i64_type().array_type(*n as u32).into(),
                }
            }
            LeoType::Fn(_, _) => context.i8_type().ptr_type(AddressSpace::default()).into(),
            LeoType::Unit => context.struct_type(&[], false).into(),
            LeoType::TypeVar(_) => {
                // Unresolved type var defaults to i64 at codegen time
                context.i64_type().into()
            }
            LeoType::Generic(name, _args) => {
                // Look up the monomorphized struct by name
                if let Some(st) = ctx.module().get_struct_type(name) {
                    st.ptr_type(AddressSpace::default()).into()
                } else {
                    context.i8_type().ptr_type(AddressSpace::default()).into()
                }
            }
        }
    }
}
