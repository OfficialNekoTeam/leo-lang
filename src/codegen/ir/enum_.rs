use super::*;
use crate::ast::expr::Expr;
use crate::common::error::{ErrorCode, ErrorKind, LeoError, LeoResult};
use crate::common::types::LeoType;

impl IrBuilder {
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
        let size = struct_type.size_of().ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "enum size fail".into()))?;
        let malloc_fn = ctx.module().get_function("malloc").ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc not found".into()))?;
        let call_res = ctx.builder().build_call(malloc_fn, &[size.into()], "enum_alloc").map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "call failed".into()))?;
        let alloc_ptr = call_res.try_as_basic_value().left().ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc void".into()))?
            .into_pointer_value();
            
        let alloc_i64 = ctx.builder().build_ptr_to_int(alloc_ptr, i64_type, "enum_alloc_i64").map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "ptr_to_int enum malloc".into()))?;
        self.emit_null_check(alloc_i64, "runtime error: out of memory\n", ctx)?;

        let enum_ptr = ctx.builder().build_pointer_cast(alloc_ptr, struct_type.ptr_type(inkwell::AddressSpace::default()), "enum_ptr")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "enum pointer cast failed".into(),
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
            Expr::Call(callee, _, _, _) => {
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
        // Alloca for collecting match arm result values
        let result_ptr = ctx
            .builder()
            .build_alloca(i64_type, "match_result")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "match result alloca failed".into(),
                )
            })?;
        // Default value: 0 (safe fallback when no arm matches and no _ arm)
        ctx.builder()
            .build_store(result_ptr, i64_type.const_int(0, false))
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "match result init store failed".into(),
                )
            })?;
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
                Expr::Call(callee, _, _, _) => {
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
            let arm_val = self.eval_int(body, ctx)?;
            ctx.builder()
                .build_store(result_ptr, arm_val)
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "match arm store failed".into(),
                    )
                })?;
            self.emit_branch(merge_block, ctx)?;
        }
        if default_block.is_none() {
            ctx.builder().position_at_end(default);
            self.emit_branch(merge_block, ctx)?;
        }
        ctx.builder().position_at_end(merge_block);
        let result = ctx
            .builder()
            .build_load(result_ptr, "match_val")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "match result load failed".into(),
                )
            })?
            .into_int_value();
        Ok(result)
    }

    /// Route destructuring patterns to payload extraction for enum match arms
    fn bind_match_payload<'a>(
        &mut self,
        pattern: &Expr,
        scrut_val: inkwell::values::IntValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<()> {
        if let Expr::Call(callee, bindings, _, _) = pattern {
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
