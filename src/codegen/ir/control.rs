use super::*;
use crate::llvm::context::LoopTarget;

impl IrBuilder {
    /// Build statement inside function body (let, assign, while, if, expr, return)
    pub(super) fn build_body_stmt(&mut self, stmt: &Stmt, ctx: &mut LlvmContext) -> LeoResult<()> {
        match stmt {
            Stmt::Let(name, ty, init) => {
                self.build_let(name, ty, init, ctx)?;
            }
            Stmt::Assign(name, expr) => {
                self.build_assign(name, expr, ctx)?;
            }
            Stmt::FieldAssign(obj, field, expr) => {
                self.build_field_assign(obj, field, expr, ctx)?;
            }
            Stmt::While(cond, body, _span) => {
                self.build_while(cond, body, ctx)?;
            }
            Stmt::If(branches, else_body, _span) => {
                self.build_if(branches, else_body, ctx)?;
            }
            Stmt::For(var, iter, body, _span) => {
                self.build_for(var, iter, body, ctx)?;
            }
            Stmt::Return(Some(expr), _) => {
                let tv = self.eval_expr(expr, ctx)?;
                self.build_return_with(tv.value, ctx)?;
            }
            Stmt::Return(None, _) => {
                let context = ctx.module().get_context();
                let zero = context.i32_type().const_int(0, false);
                ctx.builder().build_return(Some(&zero)).map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "return failed".into(),
                    )
                })?;
            }
            Stmt::Expr(expr) => {
                self.eval_and_emit(expr, ctx)?;
            }
            Stmt::Break(_, _) => {
                if let Some(target) = ctx.loop_stack.last() {
                    self.emit_branch(target.merge_block, ctx)?;
                }
            }
            Stmt::Continue(_) => {
                if let Some(target) = ctx.loop_stack.last() {
                    self.emit_branch(target.continue_block, ctx)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Build const: treated as a global constant (evaluated at compile time)
    pub(super) fn build_const(
        &mut self,
        name: &str,
        ty: &str,
        expr: &Expr,
        ctx: &mut LlvmContext,
    ) -> LeoResult<()> {
        let context = ctx.module().get_context();
        let llvm_type = Self::llvm_type(ty, ctx);
        match expr {
            Expr::Number(n, _) => {
                let gv =
                    ctx.module_mut()
                        .add_global(llvm_type, Some(AddressSpace::default()), name);
                gv.set_initializer(&context.i64_type().const_int(*n as u64, false));
                gv.set_constant(true);
                gv.set_linkage(inkwell::module::Linkage::Private);
            }
            Expr::IntLiteral(n, lit_ty, _) => {
                let gv =
                    ctx.module_mut()
                        .add_global(llvm_type, Some(AddressSpace::default()), name);
                let int_ty = Self::leo_type_to_llvm(lit_ty, ctx).into_int_type();
                gv.set_initializer(&Self::const_int_value(int_ty, *n));
                gv.set_constant(true);
                gv.set_linkage(inkwell::module::Linkage::Private);
            }
            Expr::Bool(b, _) => {
                let gv =
                    ctx.module_mut()
                        .add_global(llvm_type, Some(AddressSpace::default()), name);
                gv.set_initializer(&context.i64_type().const_int(*b as u64, false));
                gv.set_constant(true);
                gv.set_linkage(inkwell::module::Linkage::Private);
            }
            _ => {
                // Fallback: evaluate and store
                let val = self.eval_int(expr, ctx)?;
                let gv =
                    ctx.module_mut()
                        .add_global(llvm_type, Some(AddressSpace::default()), name);
                gv.set_initializer(&val);
                gv.set_constant(true);
                gv.set_linkage(inkwell::module::Linkage::Private);
            }
        }
        Ok(())
    }
    pub(super) fn build_let(
        &mut self,
        name: &str,
        ty: &Option<String>,
        init: &Option<Expr>,
        ctx: &mut LlvmContext,
    ) -> LeoResult<()> {
        let declared_type = ty.as_deref().map(LeoType::from_str);
        let alloc_type = if let Some(ty) = &declared_type {
            ty.clone()
        } else if let Some(expr) = init {
            self.infer_expr_type(expr, ctx)
        } else {
            LeoType::I64
        };
        let storage_type = if matches!(
            alloc_type,
            LeoType::Array(_, _)
                | LeoType::Struct(_)
                | LeoType::Enum(_)
                | LeoType::Generic(_, _)
                | LeoType::Tuple(_)
        ) {
            LeoType::I64
        } else {
            alloc_type.clone()
        };
        let llvm_type = Self::leo_type_to_llvm(&storage_type, ctx);
        let ptr = ctx.builder().build_alloca(llvm_type, name).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("alloca failed for {}", name),
            )
        })?;
        ctx.register_variable(name.to_string(), ptr);

        if let Some(expr) = init {
            match expr {
                Expr::Array(elems, _) => {
                    self.array_sizes
                        .insert(name.to_string(), elems.len() as u32);
                }
                Expr::ArrayRepeat(_, count, _) => match count.as_ref() {
                    Expr::Number(n, _) => {
                        self.array_sizes.insert(name.to_string(), *n as u32);
                    }
                    Expr::IntLiteral(n, _, _) => {
                        self.array_sizes.insert(name.to_string(), *n as u32);
                    }
                    _ => {}
                },
                Expr::String(s, _) => {
                    self.array_sizes.insert(name.to_string(), s.len() as u32);
                }
                Expr::StructInit(struct_name, _, _, _) => {
                    self.var_types.insert(name.to_string(), struct_name.clone());
                }
                _ => {}
            }
            let inferred_type = declared_type
                .clone()
                .unwrap_or_else(|| self.infer_expr_type(expr, ctx));
            ctx.register_type(name.to_string(), inferred_type.clone());
            let tv = self.eval_expr(expr, ctx)?;
            let value = self.coerce_arg_to_type(tv, &storage_type, ctx)?;
            ctx.builder().build_store(ptr, value).map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    format!("store failed for {}", name),
                )
            })?;
        } else {
            ctx.register_type(name.to_string(), alloc_type);
        }
        Ok(())
    }

    pub(super) fn build_assign(
        &mut self,
        name: &str,
        expr: &Expr,
        ctx: &mut LlvmContext,
    ) -> LeoResult<()> {
        let ptr = ctx.get_variable(name).ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("undefined variable: {}", name),
            )
        })?;
        let tv = self.eval_expr(expr, ctx)?;
        ctx.builder().build_store(ptr, tv.value).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("store failed for {}", name),
            )
        })?;
        let existing_type = ctx.get_type(name).cloned();
        if let Some(et) = existing_type {
            if et.is_string() {
                return Ok(());
            }
        }
        let inferred = self.infer_type_with_ctx(expr, "", ctx);
        if !matches!(inferred, LeoType::Struct(_)) {
            ctx.register_type(name.to_string(), inferred);
        }
        Ok(())
    }

    /// Build while loop: condition block → body block → merge block
    pub(super) fn build_while(
        &mut self,
        cond: &Expr,
        body: &[Stmt],
        ctx: &mut LlvmContext,
    ) -> LeoResult<()> {
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
        let context = ctx.module().get_context();

        let cond_block = context.append_basic_block(function, "while.cond");
        let body_block = context.append_basic_block(function, "while.body");
        let merge_block = context.append_basic_block(function, "while.merge");

        ctx.builder()
            .build_unconditional_branch(cond_block)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "branch failed".into(),
                )
            })?;

        ctx.builder().position_at_end(cond_block);
        let cond_val = self.eval_int(cond, ctx)?;
        let zero = context.i64_type().const_int(0, false);
        let cmp = ctx
            .builder()
            .build_int_compare(IntPredicate::NE, cond_val, zero, "while.test")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "compare failed".into(),
                )
            })?;
        ctx.builder()
            .build_conditional_branch(cmp, body_block, merge_block)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cond branch failed".into(),
                )
            })?;

        ctx.builder().position_at_end(body_block);
        ctx.loop_stack.push(LoopTarget {
            continue_block: cond_block,
            merge_block,
        });
        for stmt in body {
            self.build_body_stmt(stmt, ctx)?;
        }
        ctx.loop_stack.pop();
        self.emit_branch(cond_block, ctx)?;

        ctx.builder().position_at_end(merge_block);
        Ok(())
    }

    /// Build if/else-if/else chain using LLVM conditional branches
    pub(super) fn build_if(
        &mut self,
        branches: &[(Expr, Vec<Stmt>)],
        else_body: &Option<Vec<Stmt>>,
        ctx: &mut LlvmContext,
    ) -> LeoResult<()> {
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
        let context = ctx.module().get_context();

        let merge_block = context.append_basic_block(function, "if.merge");

        // Pre-create all condition and then blocks to avoid duplicates
        let cond_blocks: Vec<_> = (0..branches.len())
            .map(|i| context.append_basic_block(function, &format!("if.cond.{}", i)))
            .collect();
        let then_blocks: Vec<_> = (0..branches.len())
            .map(|i| context.append_basic_block(function, &format!("if.then.{}", i)))
            .collect();

        let else_block = if else_body.is_some() {
            Some(context.append_basic_block(function, "if.else"))
        } else {
            None
        };

        // Branch from current position to first condition
        ctx.builder()
            .build_unconditional_branch(cond_blocks[0])
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "branch failed".into(),
                )
            })?;

        for (i, (cond, body)) in branches.iter().enumerate() {
            // Evaluate condition in cond_block
            ctx.builder().position_at_end(cond_blocks[i]);
            let cond_val = self.eval_int(cond, ctx)?;
            let zero = context.i64_type().const_int(0, false);
            let cmp = ctx
                .builder()
                .build_int_compare(IntPredicate::NE, cond_val, zero, "if.test")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "compare failed".into(),
                    )
                })?;

            let false_block = if i + 1 < branches.len() {
                cond_blocks[i + 1]
            } else {
                else_block.unwrap_or(merge_block)
            };
            ctx.builder()
                .build_conditional_branch(cmp, then_blocks[i], false_block)
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "cond branch failed".into(),
                    )
                })?;

            // Build then body
            ctx.builder().position_at_end(then_blocks[i]);
            for stmt in body {
                self.build_body_stmt(stmt, ctx)?;
            }
            self.emit_branch(merge_block, ctx)?;
        }

        // Build else body
        if let (Some(else_stmts), Some(eb)) = (else_body, else_block) {
            ctx.builder().position_at_end(eb);
            for stmt in else_stmts {
                self.build_body_stmt(stmt, ctx)?;
            }
            self.emit_branch(merge_block, ctx)?;
        }

        ctx.builder().position_at_end(merge_block);
        Ok(())
    }

    /// Build for-in loop over string or array: for ch in s { body } / for x in arr { body }
    pub(super) fn build_for(
        &mut self,
        var: &str,
        iter: &Expr,
        body: &[Stmt],
        ctx: &mut LlvmContext,
    ) -> LeoResult<()> {
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
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();

        let iter_ptr = ctx
            .builder()
            .build_alloca(i64_type, "__for_idx")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "alloca for_idx failed".into(),
                )
            })?;
        ctx.builder()
            .build_store(iter_ptr, i64_type.const_int(0, false))
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "store for_idx failed".into(),
                )
            })?;

        let var_ptr = ctx.builder().build_alloca(i64_type, var).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("alloca {} failed", var).into(),
            )
        })?;
        ctx.register_variable(var.to_string(), var_ptr);

        let len_val = if self.expr_is_string(iter, ctx) {
            let str_ptr = self.eval_string_arg(iter, ctx)?;
            let strlen_fn = ctx.module().get_function("strlen").ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen not declared".into(),
                )
            })?;
            let result = ctx
                .builder()
                .build_call(strlen_fn, &[str_ptr.into()], "for_strlen")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "strlen call failed".into(),
                    )
                })?;
            result
                .try_as_basic_value()
                .left()
                .ok_or_else(|| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "strlen void".into(),
                    )
                })?
                .into_int_value()
        } else if let Expr::Ident(name, _) = iter {
            if let Some(&size) = self.array_sizes.get(name) {
                i64_type.const_int(size as u64, false)
            } else {
                return Err(LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    format!("for-in: cannot iterate over {}", name),
                ));
            }
        } else {
            return Err(LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "for-in only supports string and array iteration".into(),
            ));
        };

        let is_string_iter = self.expr_is_string(iter, ctx);

        let cond_block = context.append_basic_block(function, "for.cond");
        let body_block = context.append_basic_block(function, "for.body");
        let merge_block = context.append_basic_block(function, "for.merge");

        ctx.builder()
            .build_unconditional_branch(cond_block)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "branch failed".into(),
                )
            })?;

        ctx.builder().position_at_end(cond_block);
        let idx_val = ctx
            .builder()
            .build_load(iter_ptr, "for_idx_load")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load idx failed".into(),
                )
            })?
            .into_int_value();
        let cmp = ctx
            .builder()
            .build_int_compare(IntPredicate::SLT, idx_val, len_val, "for.test")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "for compare failed".into(),
                )
            })?;
        ctx.builder()
            .build_conditional_branch(cmp, body_block, merge_block)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cond branch failed".into(),
                )
            })?;

        ctx.builder().position_at_end(body_block);

        // Load current index
        let cur_idx = ctx
            .builder()
            .build_load(iter_ptr, "for.cur_idx")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load cur_idx failed".into(),
                )
            })?
            .into_int_value();

        // Load element at cur_idx into loop variable
        let elem_val = if is_string_iter {
            // String iteration: GEP into i8* + load byte + zext to i64
            let str_ptr = self.eval_string_arg(iter, ctx)?;
            let elem_ptr = unsafe {
                ctx.builder()
                    .build_in_bounds_gep(str_ptr, &[cur_idx], "for.char_ptr")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "gep for char failed".into(),
                        )
                    })?
            };
            let char_val = ctx
                .builder()
                .build_load(elem_ptr, "for.char_val")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "load char failed".into(),
                    )
                })?
                .into_int_value();
            ctx.builder()
                .build_int_z_extend(char_val, i64_type, "for.char_i64")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "zext char failed".into(),
                    )
                })?
        } else {
            // Array iteration: int_to_ptr + GEP + load i64 element
            let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
            let arr_val = self.eval_int(iter, ctx)?;
            let arr_ptr = ctx
                .builder()
                .build_int_to_ptr(arr_val, i64_ptr_type, "for.arr_ptr")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "int_to_ptr for array failed".into(),
                    )
                })?;
            let elem_ptr = unsafe {
                ctx.builder()
                    .build_in_bounds_gep(arr_ptr, &[cur_idx], "for.elem_ptr")
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "gep for elem failed".into(),
                        )
                    })?
            };
            ctx.builder()
                .build_load(elem_ptr, "for.elem_val")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "load elem failed".into(),
                    )
                })?
                .into_int_value()
        };

        // Store element into loop variable
        ctx.builder().build_store(var_ptr, elem_val).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("store {} failed", var).into(),
            )
        })?;

        ctx.loop_stack.push(LoopTarget {
            continue_block: cond_block,
            merge_block,
        });
        for stmt in body {
            self.build_body_stmt(stmt, ctx)?;
        }
        ctx.loop_stack.pop();

        // Increment index: i = i + 1
        let one = i64_type.const_int(1, false);
        let next_idx = ctx
            .builder()
            .build_int_add(cur_idx, one, "for.next_idx")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "inc idx failed".into(),
                )
            })?;
        ctx.builder().build_store(iter_ptr, next_idx).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "store next_idx failed".into(),
            )
        })?;

        self.emit_branch(cond_block, ctx)?;

        ctx.builder().position_at_end(merge_block);
        Ok(())
    }

    /// Build field assignment: obj.field = expr (GEP to field ptr, then store)
    pub(super) fn build_field_assign(
        &mut self,
        obj: &Expr,
        field: &str,
        expr: &Expr,
        ctx: &mut LlvmContext,
    ) -> LeoResult<()> {
        let i64_type = ctx.module().get_context().i64_type();
        let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
        let obj_val = self.eval_int(obj, ctx)?;
        let tv = self.eval_expr(expr, ctx)?;
        let rhs_val = tv.value;
        let obj_ptr = ctx
            .builder()
            .build_int_to_ptr(obj_val, i64_ptr_type, "fassign_ptr")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "int_to_ptr for field assign failed".into(),
                )
            })?;
        let field_idx = self.resolve_field_index(obj, field, i64_type)?;
        let field_ptr = unsafe {
            ctx.builder()
                .build_in_bounds_gep(obj_ptr, &[field_idx], "fassign_field")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "gep for field assign failed".into(),
                    )
                })?
        };
        ctx.builder().build_store(field_ptr, rhs_val).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "store field assign failed".into(),
            )
        })?;
        Ok(())
    }

    /// Resolve field name to LLVM index constant, using struct_fields map or fallback
    fn resolve_field_index<'a>(
        &self,
        obj: &Expr,
        field: &str,
        i64_type: inkwell::types::IntType<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if let Expr::Ident(var_name, _) = obj {
            if let Some(struct_type) = self.var_types.get(var_name) {
                if let Some(fields) = self.struct_fields.get(struct_type) {
                    if let Some(idx) = fields.iter().position(|f| f == field) {
                        return Ok(i64_type.const_int(idx as u64, false));
                    }
                }
            }
        }
        Err(LeoError::new(
            ErrorKind::Semantic,
            ErrorCode::SemaTypeMismatch,
            format!("unknown field: .{}", field),
        ))
    }

    fn infer_type_with_ctx(&self, expr: &Expr, type_str: &str, ctx: &LlvmContext) -> LeoType {
        if type_str != "i64" {
            return LeoType::from_str(type_str);
        }
        self.infer_expr_type(expr, ctx)
    }
}
