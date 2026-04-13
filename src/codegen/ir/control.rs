use super::*;

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
                let val = self.eval_expr_to_value(expr, ctx)?;
                self.build_return_with(val, ctx)?;
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
        let type_str = ty.as_deref().unwrap_or("i64");
        let llvm_type = Self::llvm_type(type_str, ctx);
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
                Expr::ArrayRepeat(_, count, _) => {
                    if let Expr::Number(n, _) = count.as_ref() {
                        self.array_sizes.insert(name.to_string(), *n as u32);
                    }
                }
                Expr::String(s, _) => {
                    self.string_vars.insert(name.to_string());
                    self.array_sizes.insert(name.to_string(), s.len() as u32);
                }
                _ => {
                    if type_str == "str" {
                        self.string_vars.insert(name.to_string());
                    }
                }
            }
            let val = self.eval_expr_to_value(expr, ctx)?;
            ctx.builder().build_store(ptr, val).map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    format!("store failed for {}", name),
                )
            })?;
        }
        Ok(())
    }

    /// Build assignment: load value, store into existing variable
    pub(super) fn build_assign(&mut self, name: &str, expr: &Expr, ctx: &mut LlvmContext) -> LeoResult<()> {
        let ptr = ctx.get_variable(name).ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("undefined variable: {}", name),
            )
        })?;
        let val = self.eval_expr_to_value(expr, ctx)?;
        ctx.builder().build_store(ptr, val).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("store failed for {}", name),
            )
        })?;
        Ok(())
    }

    /// Build while loop: condition block → body block → merge block
    pub(super) fn build_while(&mut self, cond: &Expr, body: &[Stmt], ctx: &mut LlvmContext) -> LeoResult<()> {
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
        for stmt in body {
            self.build_body_stmt(stmt, ctx)?;
        }
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

    /// Build for-in loop over a string: for ch in s { body }
    /// Compiles to: alloca i, alloca ch, br cond; cond: i < strlen(s) ? body : merge; body: ch=s[i], body_stmts, i++, br cond
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

        let len_val = if self.expr_is_string(iter) {
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
        } else {
            return Err(LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "for-in only supports string iteration".into(),
            ));
        };

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
        let str_ptr = self.eval_string_arg(iter, ctx)?;
        let i8_ptr_type = context.i8_type().ptr_type(AddressSpace::default());
        let casted = ctx
            .builder()
            .build_pointer_cast(str_ptr, i8_ptr_type, "for_str_i8")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr cast failed".into(),
                )
            })?;
        let current_idx = ctx
            .builder()
            .build_load(iter_ptr, "cur_idx")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load cur_idx failed".into(),
                )
            })?
            .into_int_value();
        let char_ptr = unsafe {
            ctx.builder()
                .build_in_bounds_gep(casted, &[current_idx], "for_char_ptr")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "gep failed".into(),
                    )
                })?
        };
        let char_val = ctx
            .builder()
            .build_load(char_ptr, "for_char")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load char failed".into(),
                )
            })?;
        let extended = ctx
            .builder()
            .build_int_z_extend(char_val.into_int_value(), i64_type, "for_char_ext")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "zext failed".into(),
                )
            })?;
        ctx.builder().build_store(var_ptr, extended).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "store iter var failed".into(),
            )
        })?;

        for stmt in body {
            self.build_body_stmt(stmt, ctx)?;
        }

        let one = i64_type.const_int(1, false);
        let cur = ctx
            .builder()
            .build_load(iter_ptr, "idx_load")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load idx failed".into(),
                )
            })?
            .into_int_value();
        let next = ctx
            .builder()
            .build_int_add(cur, one, "idx_next")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "add failed".into(),
                )
            })?;
        ctx.builder().build_store(iter_ptr, next).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "store idx failed".into(),
            )
        })?;
        self.emit_branch(cond_block, ctx)?;

        ctx.builder().position_at_end(merge_block);
        Ok(())
    }
}
