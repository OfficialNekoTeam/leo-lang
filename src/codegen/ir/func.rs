use super::*;
use crate::ast::Stmt;
use crate::common::error::{ErrorCode, ErrorKind, LeoError};
use crate::common::types::LeoType;
use inkwell::attributes::AttributeLoc;
use inkwell::types::BasicTypeEnum;

impl IrBuilder {
    pub(super) fn declare_fn(
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
            let ret_str = ret.as_deref().unwrap_or("i64");
            if ret_str == "unit" || ret_str == "void" {
                context.void_type().fn_type(&param_meta, false)
            } else {
                let ret_type = Self::llvm_type(ret_str, ctx);
                match ret_type {
                    BasicTypeEnum::IntType(it) => it.fn_type(&param_meta, false),
                    BasicTypeEnum::FloatType(ft) => ft.fn_type(&param_meta, false),
                    BasicTypeEnum::PointerType(pt) => pt.fn_type(&param_meta, false),
                    BasicTypeEnum::StructType(st) => st.fn_type(&param_meta, false),
                    _ => context.i64_type().fn_type(&param_meta, false),
                }
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
                            self.build_return_with(val.into(), ctx)?;
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
}
