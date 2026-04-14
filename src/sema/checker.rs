use crate::ast::expr::{BinOp, Expr, UnOp};
use crate::ast::stmt::Stmt;
use crate::common::{ErrorCode, ErrorKind, LeoError, LeoResult};
use crate::sema::scope::Scope;
use std::collections::{HashMap, HashSet};
use std::mem;

pub struct Checker {
    scope: Scope,
    functions: HashSet<String>,
    fn_params: HashMap<String, usize>,
    constants: HashSet<String>,
    enum_variants: HashMap<String, (String, u32)>,
}

impl Checker {
    /// Create new checker with root scope
    pub fn new() -> Self {
        let mut functions = HashSet::new();
        functions.insert("println".to_string());
        functions.insert("print".to_string());
        functions.insert("panic".to_string());
        functions.insert("assert".to_string());
        functions.insert("str_len".to_string());
        functions.insert("str_char_at".to_string());
        functions.insert("str_slice".to_string());
        functions.insert("str_concat".to_string());
        functions.insert("vec_new".to_string());
        functions.insert("vec_push".to_string());
        functions.insert("vec_get".to_string());
        functions.insert("vec_len".to_string());
        functions.insert("file_read".to_string());
        functions.insert("file_write".to_string());
        functions.insert("char_to_str".to_string());
        functions.insert("is_digit".to_string());
        functions.insert("is_alpha".to_string());
        functions.insert("is_alnum".to_string());
        functions.insert("to_string".to_string());
        Self {
            scope: Scope::new(),
            functions,
            fn_params: HashMap::new(),
            constants: HashSet::new(),
            enum_variants: HashMap::new(),
        }
    }

    /// Check list of statements
    pub fn check(&mut self, stmts: &[Stmt]) -> LeoResult<()> {
        for stmt in stmts {
            self.check_stmt(stmt)?;
        }
        Ok(())
    }

    /// Check match pattern: enum destructuring args are bindings, not value expressions
    fn check_pattern(&mut self, pattern: &Expr) -> LeoResult<()> {
        match pattern {
            Expr::Ident(name, _) => {
                if name == "_" || name.contains("::") {
                    return Ok(());
                }
                self.check_expr(pattern)?;
            }
            Expr::Call(callee, args, _) => {
                self.check_pattern(callee)?;
                // Enum destructuring: args like Number(n) are variable bindings
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if name.contains("::") {
                        for arg in args {
                            if let Expr::Ident(var_name, _) = arg {
                                self.scope
                                    .define(var_name.clone(), "unknown".to_string(), true);
                            }
                        }
                        return Ok(());
                    }
                }
                for arg in args {
                    self.check_expr(arg)?;
                }
            }
            _ => {
                self.check_expr(pattern)?;
            }
        }
        Ok(())
    }

    fn check_stmt(&mut self, stmt: &Stmt) -> LeoResult<()> {
        match stmt {
            Stmt::Expr(e) => {
                self.check_expr(e)?;
            }
            Stmt::Let(name, ty, init) => self.check_let(name, ty, init)?,
            Stmt::Assign(_, e) | Stmt::MutAssign(_, e) | Stmt::FieldAssign(_, _, e) => {
                self.check_expr(e)?;
            }
            Stmt::Return(e, _) => {
                if let Some(e) = e {
                    self.check_expr(e)?;
                }
            }
            Stmt::If(branches, els, _) => self.check_if(branches, els)?,
            Stmt::While(cond, body, _) => self.check_while(cond, body)?,
            Stmt::For(name, iter, body, _) => self.check_for(name, iter, body)?,
            Stmt::Function(name, params, ret, body, _)
            | Stmt::AsyncFunction(name, params, ret, body, _) => {
                self.functions.insert(name.clone());
                self.fn_params.insert(name.clone(), params.len());
                self.check_fn(name, params, ret, body)?;
            }
            Stmt::Struct(name, fields, _) => self.check_struct(name, fields)?,
            Stmt::Import(_, _, _) | Stmt::FromImport(_, _, _) => {}
            Stmt::Module(_, body, _) => {
                self.check(body)?;
            }
            Stmt::Break(_, _) | Stmt::Continue(_) => {}
            Stmt::Const(name, _, _, _) => {
                self.constants.insert(name.clone());
            }
            Stmt::Trait(_, _, _) => {}
            Stmt::Impl(struct_name, _trait_name, methods, _) => {
                for method in methods {
                    if let Stmt::Function(name, params, ret, body, _) = method {
                        let mangled = format!("{}_{}", struct_name, name);
                        self.functions.insert(mangled.clone());
                        self.fn_params.insert(mangled.clone(), params.len());
                        self.check_fn(&mangled, params, ret, body)?;
                    }
                }
            }
            Stmt::Pub(inner) => {
                self.check_stmt(inner)?;
            }
            Stmt::Enum(name, variants, _) => self.check_enum(name, variants)?,
        }
        Ok(())
    }

    /// Check let binding
    fn check_let(&mut self, name: &str, ty: &Option<String>, init: &Option<Expr>) -> LeoResult<()> {
        let inferred = match init {
            Some(e) => Some(self.check_expr(e)?),
            None => None,
        };
        let final_ty = ty
            .as_ref()
            .cloned()
            .or(inferred)
            .unwrap_or_else(|| "unknown".to_string());
        self.scope.define(name.to_string(), final_ty, true);
        Ok(())
    }

    /// Check if with branches
    fn check_if(
        &mut self,
        branches: &[(Expr, Vec<Stmt>)],
        els: &Option<Vec<Stmt>>,
    ) -> LeoResult<()> {
        for (cond, body) in branches {
            let cond_ty = self.check_expr(cond)?;
            if cond_ty != "bool" && cond_ty != "unknown" && cond_ty != "i64" {
                return Err(LeoError::new(
                    ErrorKind::Semantic,
                    ErrorCode::SemaTypeMismatch,
                    format!("if condition must be bool, got {}", cond_ty),
                ));
            }
            let old = mem::replace(&mut self.scope, Scope::new());
            self.scope = Scope::with_parent(old);
            self.check(body)?;
            let child = mem::replace(&mut self.scope, Scope::new());
            self.scope = child.into_parent().unwrap_or(Scope::new());
        }
        if let Some(els) = els {
            self.check(els)?;
        }
        Ok(())
    }

    /// Check while loop
    fn check_while(&mut self, cond: &Expr, body: &[Stmt]) -> LeoResult<()> {
        let cond_ty = self.check_expr(cond)?;
        if cond_ty != "bool" && cond_ty != "unknown" && cond_ty != "i64" {
            return Err(LeoError::new(
                ErrorKind::Semantic,
                ErrorCode::SemaTypeMismatch,
                format!("while condition must be bool, got {}", cond_ty),
            ));
        }
        self.check(body)
    }

    /// Check for loop
    fn check_for(&mut self, name: &str, iter: &Expr, body: &[Stmt]) -> LeoResult<()> {
        let iter_ty = self.check_expr(iter)?;
        let old = mem::replace(&mut self.scope, Scope::new());
        let mut child = Scope::with_parent(old);
        child.define(name.to_string(), iter_ty, true);
        self.scope = child;
        self.check(body)?;
        let child = mem::replace(&mut self.scope, Scope::new());
        self.scope = child.into_parent().unwrap_or(Scope::new());
        Ok(())
    }

    /// Check function definition
    fn check_fn(
        &mut self,
        name: &str,
        params: &[(String, String)],
        _ret: &Option<String>,
        body: &[Stmt],
    ) -> LeoResult<()> {
        let old = mem::replace(&mut self.scope, Scope::new());
        let mut child = Scope::with_parent(old);
        for (pname, pty) in params {
            child.define(pname.clone(), pty.clone(), false);
        }
        self.scope = child;
        self.check(body)?;
        let child = mem::replace(&mut self.scope, Scope::new());
        self.scope = child.into_parent().unwrap_or(Scope::new());
        let param_types: Vec<&str> = params.iter().map(|(_, t)| t.as_str()).collect();
        let fn_ty = format!("fn({})", param_types.join(", "));
        self.scope.define(name.to_string(), fn_ty, false);
        Ok(())
    }

    /// Check struct definition
    fn check_struct(&mut self, name: &str, fields: &[(String, String)]) -> LeoResult<()> {
        let field_types: Vec<&str> = fields.iter().map(|(_, t)| t.as_str()).collect();
        let struct_ty = format!("struct({})", field_types.join(", "));
        self.scope.define(name.to_string(), struct_ty, false);
        Ok(())
    }

    /// Check enum variants — payload expressions are type names (i64, str), not values
    fn check_enum(&mut self, name: &str, variants: &[(String, Vec<Expr>)]) -> LeoResult<()> {
        for (i, (vname, _exprs)) in variants.iter().enumerate() {
            let qualified = format!("{}::{}", name, vname);
            self.enum_variants
                .insert(qualified.clone(), (name.to_string(), i as u32));
            self.functions.insert(qualified);
        }
        self.scope
            .define(name.to_string(), format!("enum({})", variants.len()), false);
        Ok(())
    }

    /// Check expression and return its type
    fn check_expr(&mut self, expr: &Expr) -> LeoResult<String> {
        match expr {
            Expr::Number(_, _) => Ok("i64".to_string()),
            Expr::Float(_, _) => Ok("f64".to_string()),
            Expr::String(_, _) => Ok("str".to_string()),
            Expr::Char(_, _) => Ok("u8".to_string()),
            Expr::Bool(_, _) => Ok("bool".to_string()),
            Expr::Unit(_) => Ok("unit".to_string()),
            Expr::Ident(name, _) => {
                if self.constants.contains(name) {
                    return Ok("i64".to_string());
                }
                self.scope
                    .resolve(name)
                    .map(|e| e.ty.clone())
                    .ok_or_else(|| {
                        LeoError::new(
                            ErrorKind::Semantic,
                            ErrorCode::SemaUndefinedVariable,
                            format!("undefined variable: {}", name),
                        )
                    })
            }
            Expr::Binary(op, left, right, _) => self.check_binary(op, left, right),
            Expr::Unary(op, e, _) => self.check_unary(op, e),
            Expr::Call(callee, args, _) => self.check_call(callee, args),
            Expr::Index(obj, idx, _) => self.check_index(obj, idx),
            Expr::Select(_, _, _) => Ok("unknown".to_string()),
            Expr::Array(elements, _) => {
                for e in elements {
                    self.check_expr(e)?;
                }
                Ok("array".to_string())
            }
            Expr::ArrayRepeat(val, count, _) => {
                self.check_expr(val)?;
                self.check_expr(count)?;
                Ok("array".to_string())
            }
            Expr::StructInit(name, fields, _) => {
                for (_, val) in fields {
                    self.check_expr(val)?;
                }
                Ok(name.clone())
            }
            Expr::Lambda(params, body, _) => self.check_lambda(params, body),
            Expr::If(_, _, _, _) => Ok("unknown".to_string()),
            Expr::Block(stmts, _) => {
                for s in stmts {
                    self.check_expr(s)?;
                }
                Ok("unknown".to_string())
            }
            Expr::Await(e, _) => self.check_expr(e),
            Expr::Match(scrutinee, arms, _) => {
                self.check_expr(scrutinee)?;
                for (pattern, body) in arms {
                    self.check_pattern(pattern)?;
                    self.check_expr(body)?;
                }
                Ok("unknown".to_string())
            }
        }
    }

    /// Check binary expression type compatibility
    fn check_binary(&mut self, op: &BinOp, left: &Expr, right: &Expr) -> LeoResult<String> {
        let lt = self.check_expr(left)?;
        let rt = self.check_expr(right)?;
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                if lt == "bool" || rt == "bool" {
                    return Err(LeoError::new(
                        ErrorKind::Semantic,
                        ErrorCode::SemaTypeMismatch,
                        "arithmetic on bool is not allowed".into(),
                    ));
                }
                if lt != rt && lt != "unknown" && rt != "unknown" {
                    return Err(LeoError::new(
                        ErrorKind::Semantic,
                        ErrorCode::SemaTypeMismatch,
                        format!("type mismatch: {} vs {}", lt, rt),
                    ));
                }
                if lt != "unknown" {
                    Ok(lt)
                } else {
                    Ok(rt)
                }
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                Ok("bool".to_string())
            }
            BinOp::And | BinOp::Or => Ok("bool".to_string()),
            BinOp::BitAnd | BinOp::BitOr | BinOp::Shl | BinOp::Shr => Ok(lt),
        }
    }

    /// Check unary expression
    fn check_unary(&mut self, op: &UnOp, e: &Expr) -> LeoResult<String> {
        let ty = self.check_expr(e)?;
        match op {
            UnOp::Neg | UnOp::Minus => {
                if ty != "i64" && ty != "f64" && ty != "i32" {
                    return Err(LeoError::new(
                        ErrorKind::Semantic,
                        ErrorCode::SemaTypeMismatch,
                        format!("cannot negate type {}", ty),
                    ));
                }
                Ok(ty)
            }
            UnOp::Not => {
                if ty != "bool" {
                    return Err(LeoError::new(
                        ErrorKind::Semantic,
                        ErrorCode::SemaTypeMismatch,
                        format!("cannot 'not' type {}", ty),
                    ));
                }
                Ok(ty)
            }
            UnOp::Ref => Ok(format!("&{}", ty)),
            UnOp::Deref => ty.strip_prefix('&').map(|s| s.to_string()).ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Semantic,
                    ErrorCode::SemaTypeMismatch,
                    format!("cannot dereference non-pointer type {}", ty),
                )
            }),
        }
    }

    /// Check function call
    fn check_call(&mut self, callee: &Expr, args: &[Expr]) -> LeoResult<String> {
        match callee {
            Expr::Ident(name, _) => {
                if !self.functions.contains(name) && self.scope.resolve(name).is_none() {
                    return Err(LeoError::new(
                        ErrorKind::Semantic,
                        ErrorCode::SemaUndefinedVariable,
                        format!("undefined function or variable: {}", name),
                    ));
                }
                if let Some(&expected) = self.fn_params.get(name) {
                    if args.len() != expected {
                        return Err(LeoError::new(
                            ErrorKind::Semantic,
                            ErrorCode::SemaTypeMismatch,
                            format!(
                                "function {} expects {} args, got {}",
                                name,
                                expected,
                                args.len()
                            ),
                        ));
                    }
                }
            }
            _ => {
                self.check_expr(callee)?;
            }
        }
        for arg in args {
            self.check_expr(arg)?;
        }
        Ok("unknown".to_string())
    }

    /// Check index expression
    fn check_index(&mut self, obj: &Expr, idx: &Expr) -> LeoResult<String> {
        let obj_ty = self.check_expr(obj)?;
        let idx_ty = self.check_expr(idx)?;
        if idx_ty != "i64" && idx_ty != "i32" && idx_ty != "unknown" {
            return Err(LeoError::new(
                ErrorKind::Semantic,
                ErrorCode::SemaTypeMismatch,
                format!("index must be integer, got {}", idx_ty),
            ));
        }
        Ok(obj_ty)
    }

    /// Check lambda expression
    fn check_lambda(&mut self, params: &[(String, String)], body: &Expr) -> LeoResult<String> {
        let old = mem::replace(&mut self.scope, Scope::new());
        let mut child = Scope::with_parent(old);
        for (name, ty) in params {
            child.define(name.clone(), ty.clone(), false);
        }
        self.scope = child;
        let ret = self.check_expr(body)?;
        let child = mem::replace(&mut self.scope, Scope::new());
        self.scope = child.into_parent().unwrap_or(Scope::new());
        let param_types: Vec<&str> = params.iter().map(|(_, t)| t.as_str()).collect();
        Ok(format!("fn({}) -> {}", param_types.join(", "), ret))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::expr::Expr;
    use crate::common::span::Span;

    #[test]
    fn test_check_number() {
        let mut c = Checker::new();
        let ty = c.check_expr(&Expr::Number(42, Span::dummy())).unwrap();
        assert_eq!(ty, "i64");
    }

    #[test]
    fn test_check_bool() {
        let mut c = Checker::new();
        let ty = c.check_expr(&Expr::Bool(true, Span::dummy())).unwrap();
        assert_eq!(ty, "bool");
    }

    #[test]
    fn test_check_let_and_resolve() {
        let mut c = Checker::new();
        let stmt = Stmt::Let("x".into(), Some("i32".into()), None);
        c.check_stmt(&stmt).unwrap();
        let ty = c.scope.resolve("x").unwrap();
        assert_eq!(ty.ty, "i32");
    }

    #[test]
    fn test_check_undefined_var() {
        let mut c = Checker::new();
        let expr = Expr::Ident("x".into(), Span::dummy());
        let result = c.check_expr(&expr);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_binary_type_mismatch() {
        let mut c = Checker::new();
        let expr = Expr::Binary(
            BinOp::Add,
            Box::new(Expr::Number(1, Span::dummy())),
            Box::new(Expr::Bool(true, Span::dummy())),
            Span::dummy(),
        );
        let result = c.check_expr(&expr);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_fn_definition() {
        let mut c = Checker::new();
        let stmt = Stmt::Function(
            "add".into(),
            vec![("a".into(), "i32".into())],
            Some("i32".into()),
            vec![],
            Span::dummy(),
        );
        c.check_stmt(&stmt).unwrap();
        assert!(c.scope.resolve("add").is_some());
    }
}
