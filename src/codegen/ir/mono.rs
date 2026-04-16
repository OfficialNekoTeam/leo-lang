use super::*;
use crate::ast::expr::Expr;
use crate::ast::stmt::Stmt;
use crate::common::error::{ErrorCode, ErrorKind, LeoError, LeoResult};

/// A stored generic function definition, waiting for instantiation.
#[derive(Debug, Clone)]
pub struct GenericFnDef {
    pub type_params: Vec<String>,
    pub params: Vec<(String, String)>,
    pub ret: Option<String>,
    pub body: Vec<Stmt>,
}

/// A stored generic struct definition, waiting for instantiation.
#[derive(Debug, Clone)]
pub struct GenericStructDef {
    pub type_params: Vec<String>,
    pub fields: Vec<(String, String)>,
}

impl IrBuilder {
    /// Build a mangled name for a monomorphized instance.
    /// e.g. "max" + ["i64"] => "max_i64"
    /// e.g. "Stack" + ["i64"] => "Stack_i64"
    pub(super) fn mangle_generic_name(
        base: &str,
        type_args: &[String],
    ) -> String {
        if type_args.is_empty() {
            return base.to_string();
        }
        format!("{}_{}", base, type_args.join("_"))
    }

    /// Substitute type parameters in a parameter type string.
    /// e.g. "T" with mapping {"T": "i64"} => "i64"
    fn substitute_type(
        ty: &str,
        type_map: &std::collections::HashMap<String, String>,
    ) -> String {
        type_map.get(ty).cloned().unwrap_or_else(|| ty.to_string())
    }

    /// Substitute type parameters throughout a function's params and return type.
    fn substitute_fn_signature(
        params: &[(String, String)],
        ret: &Option<String>,
        type_map: &std::collections::HashMap<String, String>,
    ) -> (Vec<(String, String)>, Option<String>) {
        let new_params: Vec<(String, String)> = params
            .iter()
            .map(|(name, ty)| {
                (name.clone(), Self::substitute_type(ty, type_map))
            })
            .collect();
        let new_ret = ret
            .as_ref()
            .map(|r| Self::substitute_type(r, type_map));
        (new_params, new_ret)
    }

    /// Substitute type parameters throughout a struct's fields.
    fn substitute_struct_fields(
        fields: &[(String, String)],
        type_map: &std::collections::HashMap<String, String>,
    ) -> Vec<(String, String)> {
        fields
            .iter()
            .map(|(name, ty)| {
                (name.clone(), Self::substitute_type(ty, type_map))
            })
            .collect()
    }

    /// Substitute type parameters in expressions (recursive).
    /// Replaces type args in Call and StructInit, and rewrites
    /// identifiers that match type param names.
    fn substitute_expr(
        expr: &Expr,
        type_map: &std::collections::HashMap<String, String>,
    ) -> Expr {
        match expr {
            Expr::Call(callee, args, type_args, span) => {
                let new_callee = Box::new(Self::substitute_expr(callee, type_map));
                let new_args: Vec<Expr> = args
                    .iter()
                    .map(|a| Self::substitute_expr(a, type_map))
                    .collect();
                let new_type_args: Vec<String> = type_args
                    .iter()
                    .map(|t| Self::substitute_type(t, type_map))
                    .collect();
                Expr::Call(new_callee, new_args, new_type_args, *span)
            }
            Expr::StructInit(name, fields, type_args, span) => {
                let new_name = Self::substitute_type(name, type_map);
                let new_fields: Vec<(String, Expr)> = fields
                    .iter()
                    .map(|(n, e)| (n.clone(), Self::substitute_expr(e, type_map)))
                    .collect();
                let new_type_args: Vec<String> = type_args
                    .iter()
                    .map(|t| Self::substitute_type(t, type_map))
                    .collect();
                Expr::StructInit(new_name, new_fields, new_type_args, *span)
            }
            Expr::Binary(op, l, r, span) => {
                Expr::Binary(
                    op.clone(),
                    Box::new(Self::substitute_expr(l, type_map)),
                    Box::new(Self::substitute_expr(r, type_map)),
                    *span,
                )
            }
            Expr::Unary(op, e, span) => {
                Expr::Unary(
                    op.clone(),
                    Box::new(Self::substitute_expr(e, type_map)),
                    *span,
                )
            }
            Expr::If(cond, then, els, span) => {
                Expr::If(
                    Box::new(Self::substitute_expr(cond, type_map)),
                    Box::new(Self::substitute_expr(then, type_map)),
                    els.as_ref()
                        .map(|e| Box::new(Self::substitute_expr(e, type_map))),
                    *span,
                )
            }
            Expr::Index(obj, idx, span) => {
                Expr::Index(
                    Box::new(Self::substitute_expr(obj, type_map)),
                    Box::new(Self::substitute_expr(idx, type_map)),
                    *span,
                )
            }
            Expr::Select(obj, field, span) => {
                Expr::Select(
                    Box::new(Self::substitute_expr(obj, type_map)),
                    field.clone(),
                    *span,
                )
            }
            Expr::Array(elems, span) => {
                let new_elems: Vec<Expr> = elems
                    .iter()
                    .map(|e| Self::substitute_expr(e, type_map))
                    .collect();
                Expr::Array(new_elems, *span)
            }
            Expr::ArrayRepeat(val, count, span) => {
                Expr::ArrayRepeat(
                    Box::new(Self::substitute_expr(val, type_map)),
                    Box::new(Self::substitute_expr(count, type_map)),
                    *span,
                )
            }
            Expr::Match(scrut, arms, span) => {
                let new_scrut = Box::new(Self::substitute_expr(scrut, type_map));
                let new_arms: Vec<(Expr, Expr)> = arms
                    .iter()
                    .map(|(p, b)| {
                        (
                            Self::substitute_expr(p, type_map),
                            Self::substitute_expr(b, type_map),
                        )
                    })
                    .collect();
                Expr::Match(new_scrut, new_arms, *span)
            }
            Expr::Block(exprs, span) => {
                let new_exprs: Vec<Expr> = exprs
                    .iter()
                    .map(|e| Self::substitute_expr(e, type_map))
                    .collect();
                Expr::Block(new_exprs, *span)
            }
            Expr::Lambda(params, body, span) => {
                let new_params: Vec<(String, String)> = params
                    .iter()
                    .map(|(n, t)| (n.clone(), Self::substitute_type(t, type_map)))
                    .collect();
                Expr::Lambda(
                    new_params,
                    Box::new(Self::substitute_expr(body, type_map)),
                    *span,
                )
            }
            Expr::Await(e, span) => {
                Expr::Await(Box::new(Self::substitute_expr(e, type_map)), *span)
            }
            // Literals and identifiers pass through unchanged
            other => other.clone(),
        }
    }

    /// Substitute type parameters in statements (recursive).
    fn substitute_stmt(
        stmt: &Stmt,
        type_map: &std::collections::HashMap<String, String>,
    ) -> Stmt {
        match stmt {
            Stmt::Expr(e) => Stmt::Expr(Self::substitute_expr(e, type_map)),
            Stmt::Let(name, ty, init) => {
                let new_ty = ty.as_ref().map(|t| Self::substitute_type(t, type_map));
                let new_init = init.as_ref().map(|e| Self::substitute_expr(e, type_map));
                Stmt::Let(name.clone(), new_ty, new_init)
            }
            Stmt::Assign(name, e) => {
                Stmt::Assign(name.clone(), Self::substitute_expr(e, type_map))
            }
            Stmt::MutAssign(name, e) => {
                Stmt::MutAssign(name.clone(), Self::substitute_expr(e, type_map))
            }
            Stmt::FieldAssign(obj, field, e) => {
                Stmt::FieldAssign(
                    Box::new(Self::substitute_expr(obj, type_map)),
                    field.clone(),
                    Self::substitute_expr(e, type_map),
                )
            }
            Stmt::Return(e, span) => {
                Stmt::Return(
                    e.as_ref().map(|e| Self::substitute_expr(e, type_map)),
                    *span,
                )
            }
            Stmt::If(branches, els, span) => {
                let new_branches: Vec<(Expr, Vec<Stmt>)> = branches
                    .iter()
                    .map(|(cond, body)| {
                        (
                            Self::substitute_expr(cond, type_map),
                            body.iter()
                                .map(|s| Self::substitute_stmt(s, type_map))
                                .collect(),
                        )
                    })
                    .collect();
                let new_els = els.as_ref().map(|stmts| {
                    stmts
                        .iter()
                        .map(|s| Self::substitute_stmt(s, type_map))
                        .collect()
                });
                Stmt::If(new_branches, new_els, *span)
            }
            Stmt::While(cond, body, span) => {
                Stmt::While(
                    Self::substitute_expr(cond, type_map),
                    body.iter()
                        .map(|s| Self::substitute_stmt(s, type_map))
                        .collect(),
                    *span,
                )
            }
            Stmt::For(name, iter, body, span) => {
                Stmt::For(
                    name.clone(),
                    Self::substitute_expr(iter, type_map),
                    body.iter()
                        .map(|s| Self::substitute_stmt(s, type_map))
                        .collect(),
                    *span,
                )
            }
            Stmt::Function(name, params, ret, body, tparams, span) => {
                let (new_params, new_ret) =
                    Self::substitute_fn_signature(params, ret, type_map);
                let new_body: Vec<Stmt> = body
                    .iter()
                    .map(|s| Self::substitute_stmt(s, type_map))
                    .collect();
                Stmt::Function(
                    name.clone(),
                    new_params,
                    new_ret,
                    new_body,
                    tparams.clone(),
                    *span,
                )
            }
            // Pass through other statements unchanged
            other => other.clone(),
        }
    }

    /// Substitute type parameters in a function body.
    pub(super) fn substitute_body(
        body: &[Stmt],
        type_map: &std::collections::HashMap<String, String>,
    ) -> Vec<Stmt> {
        body.iter()
            .map(|s| Self::substitute_stmt(s, type_map))
            .collect()
    }

    /// Instantiate a generic function: clone its AST, substitute types,
    /// and compile as a regular function with a mangled name.
    pub(super) fn instantiate_generic_fn(
        &mut self,
        base_name: &str,
        type_args: &[String],
        ctx: &mut LlvmContext,
    ) -> LeoResult<String> {
        let mangled = Self::mangle_generic_name(base_name, type_args);

        // Already instantiated?
        if ctx.get_function(&mangled).is_some() {
            return Ok(mangled);
        }

        let def = self
            .generic_fns
            .get(base_name)
            .cloned()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    format!("generic function '{}' not defined", base_name),
                )
            })?;

        if type_args.len() != def.type_params.len() {
            return Err(LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!(
                    "generic function '{}' expects {} type args, got {}",
                    base_name,
                    def.type_params.len(),
                    type_args.len()
                ),
            ));
        }

        let mut type_map = std::collections::HashMap::new();
        for (param, arg) in def.type_params.iter().zip(type_args.iter()) {
            type_map.insert(param.clone(), arg.clone());
        }

        let (new_params, new_ret) =
            Self::substitute_fn_signature(&def.params, &def.ret, &type_map);
        let new_body = Self::substitute_body(&def.body, &type_map);

        // Save the current builder position (we're mid-codegen in another fn)
        let saved_block = ctx.builder().get_insert_block();
        let saved_fn = ctx.current_fn();
        let saved_vars = ctx.save_variables();

        self.build_fn(&mangled, &new_params, &new_ret, &new_body, ctx)?;

        // Restore previous builder position
        if let Some(block) = saved_block {
            ctx.builder().position_at_end(block);
        }
        if let Some(fv) = saved_fn {
            ctx.set_current_fn(fv);
        }
        ctx.restore_variables(saved_vars);

        Ok(mangled)
    }

    /// Instantiate a generic struct: clone its field definitions,
    /// substitute types, and register the LLVM struct type.
    pub(super) fn instantiate_generic_struct(
        &mut self,
        base_name: &str,
        type_args: &[String],
        ctx: &mut LlvmContext,
    ) -> LeoResult<String> {
        let mangled = Self::mangle_generic_name(base_name, type_args);

        // Already instantiated?
        if ctx.module().get_struct_type(&mangled).is_some() {
            return Ok(mangled);
        }

        let def = self
            .generic_structs
            .get(base_name)
            .cloned()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    format!("generic struct '{}' not defined", base_name),
                )
            })?;

        if type_args.len() != def.type_params.len() {
            return Err(LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!(
                    "generic struct '{}' expects {} type args, got {}",
                    base_name,
                    def.type_params.len(),
                    type_args.len()
                ),
            ));
        }

        let mut type_map = std::collections::HashMap::new();
        for (param, arg) in def.type_params.iter().zip(type_args.iter()) {
            type_map.insert(param.clone(), arg.clone());
        }

        let new_fields = Self::substitute_struct_fields(&def.fields, &type_map);

        // Create the LLVM struct
        let context = ctx.module().get_context();
        let struct_type = context.opaque_struct_type(&mangled);
        let field_types: Vec<BasicTypeEnum> = new_fields
            .iter()
            .map(|(_, ty)| Self::llvm_type(ty, ctx))
            .collect();
        struct_type.set_body(&field_types, false);

        // Register field metadata
        let field_names: Vec<String> = new_fields.iter().map(|(n, _)| n.clone()).collect();
        let field_type_names: Vec<String> =
            new_fields.iter().map(|(_, ty)| ty.clone()).collect();
        self.struct_fields.insert(mangled.clone(), field_names);
        self.struct_field_types
            .insert(mangled.clone(), field_type_names);

        Ok(mangled)
    }
}
