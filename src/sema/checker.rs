use crate::ast::expr::{BinOp, Expr, UnOp};
use crate::ast::stmt::Stmt;
use crate::common::types::LeoType;
use crate::common::{ErrorCode, ErrorKind, LeoError, LeoResult};
use crate::sema::scope::Scope;
use std::collections::{HashMap, HashSet};
use std::mem;

pub struct Checker {
    scope: Scope,
    functions: HashSet<String>,
    fn_params: HashMap<String, usize>,
    constants: HashMap<String, LeoType>,
    enum_variants: HashMap<String, (String, u32)>,
}

impl Checker {
    /// Create new checker with root scope
    pub fn new() -> Self {
        let mut functions = HashSet::new();
        for name in [
            "println",
            "print",
            "panic",
            "assert",
            "str_len",
            "str_char_at",
            "str_slice",
            "str_concat",
            "vec_new",
            "vec_push",
            "vec_get",
            "vec_len",
            "file_read",
            "file_write",
            "char_to_str",
            "is_digit",
            "is_alpha",
            "is_alnum",
            "to_string",
        ] {
            functions.insert(name.to_string());
        }
        Self {
            scope: Scope::new(),
            functions,
            fn_params: HashMap::new(),
            constants: HashMap::new(),
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
            Expr::Call(callee, args, _, _) => {
                self.check_pattern(callee)?;
                if let Expr::Ident(name, _) = callee.as_ref() {
                    if name.contains("::") {
                        for arg in args {
                            if let Expr::Ident(var_name, _) = arg {
                                self.scope.define(var_name.clone(), LeoType::Unknown, true);
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
            Stmt::Assign(name, e) | Stmt::MutAssign(name, e) => {
                let rhs_ty = self.check_expr(e)?;
                match self.scope.resolve(name).cloned() {
                    None => {
                        return Err(LeoError::new(
                            ErrorKind::Semantic,
                            ErrorCode::SemaUndefinedVariable,
                            format!("undefined variable: {}", name),
                        ));
                    }
                    Some(sym) => {
                        if rhs_ty != LeoType::Unknown
                            && sym.ty != LeoType::Unknown
                            && rhs_ty != sym.ty
                            && !Self::types_coercible(&rhs_ty, &sym.ty)
                        {
                            return Err(LeoError::new(
                                ErrorKind::Semantic,
                                ErrorCode::SemaTypeMismatch,
                                format!(
                                    "assignment type mismatch: {} has type {}, got {}",
                                    name, sym.ty, rhs_ty
                                ),
                            ));
                        }
                    }
                }
            }
            Stmt::FieldAssign(_, _, e) => {
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
            Stmt::Function(name, params, ret, body, _, _)
            | Stmt::AsyncFunction(name, params, ret, body, _, _) => {
                self.functions.insert(name.clone());
                self.fn_params.insert(name.clone(), params.len());
                self.check_fn(name, params, ret, body)?;
            }
            Stmt::Struct(name, fields, _, _) => self.check_struct(name, fields)?,
            Stmt::Import(_, _, _) | Stmt::FromImport(_, _, _) => {}
            Stmt::Module(_, body, _) => {
                self.check(body)?;
            }
            Stmt::Break(_, _) | Stmt::Continue(_) => {}
            Stmt::Const(name, ty, expr, _) => self.check_const(name, ty, expr)?,
            Stmt::Trait(_, _, _) => {}
            Stmt::Impl(struct_name, _trait_name, methods, _, _) => {
                for method in methods {
                    if let Stmt::Function(name, params, ret, body, _, _) = method {
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

    fn check_let(&mut self, name: &str, ty: &Option<String>, init: &Option<Expr>) -> LeoResult<()> {
        if ty.is_none() && matches!(init, Some(Expr::Array(elements, _)) if elements.is_empty()) {
            return Err(LeoError::new(
                ErrorKind::Semantic,
                ErrorCode::SemaTypeMismatch,
                "empty array requires a type annotation".into(),
            ));
        }
        let inferred = match init {
            Some(Expr::Array(elements, _)) if elements.is_empty() => {
                ty.as_ref().map(|s| LeoType::parse(s)).transpose()?
            }
            Some(e) => Some(self.check_expr(e)?),
            None => None,
        };
        let anno_ty = ty.as_ref().map(|s| LeoType::parse(s)).transpose()?;
        // Reject annotation vs initializer mismatch
        if let (Some(anno), Some(inf)) = (&anno_ty, &inferred) {
            if *inf != LeoType::Unknown && *inf != *anno && !Self::types_coercible(inf, anno) {
                return Err(LeoError::new(
                    ErrorKind::Semantic,
                    ErrorCode::SemaTypeMismatch,
                    format!(
                        "type mismatch: annotation {} but initializer is {}",
                        anno, inf
                    ),
                ));
            }
        }
        let final_ty = anno_ty.or(inferred).unwrap_or(LeoType::Unknown);
        self.scope.define(name.to_string(), final_ty, true);
        Ok(())
    }

    fn check_const(&mut self, name: &str, ty: &str, expr: &Expr) -> LeoResult<()> {
        let actual = self.check_expr(expr)?;
        let expected = LeoType::parse(ty)?;
        if actual != LeoType::Unknown
            && actual != expected
            && !Self::types_coercible(&actual, &expected)
        {
            return Err(LeoError::new(
                ErrorKind::Semantic,
                ErrorCode::SemaTypeMismatch,
                format!("const type mismatch: expected {}, got {}", expected, actual),
            ));
        }
        self.constants.insert(name.to_string(), expected.clone());
        self.scope.define(name.to_string(), expected, false);
        Ok(())
    }

    fn check_if(
        &mut self,
        branches: &[(Expr, Vec<Stmt>)],
        els: &Option<Vec<Stmt>>,
    ) -> LeoResult<()> {
        for (cond, body) in branches {
            let cond_ty = self.check_expr(cond)?;
            if cond_ty != LeoType::Unknown && cond_ty != LeoType::Bool && !cond_ty.is_integer() {
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
            let old = mem::replace(&mut self.scope, Scope::new());
            self.scope = Scope::with_parent(old);
            self.check(els)?;
            let child = mem::replace(&mut self.scope, Scope::new());
            self.scope = child.into_parent().unwrap_or(Scope::new());
        }
        Ok(())
    }

    fn check_while(&mut self, cond: &Expr, body: &[Stmt]) -> LeoResult<()> {
        let cond_ty = self.check_expr(cond)?;
        if cond_ty != LeoType::Unknown && cond_ty != LeoType::Bool && !cond_ty.is_integer() {
            return Err(LeoError::new(
                ErrorKind::Semantic,
                ErrorCode::SemaTypeMismatch,
                format!("while condition must be bool, got {}", cond_ty),
            ));
        }
        self.check(body)
    }

    fn check_for(&mut self, name: &str, iter: &Expr, body: &[Stmt]) -> LeoResult<()> {
        let iter_ty = self.check_expr(iter)?;
        let elem_ty = match iter_ty {
            LeoType::Array(elem, _) => *elem,
            LeoType::Vec(elem) => *elem,
            LeoType::Str => LeoType::Char,
            _ => LeoType::Unknown,
        };
        let old = mem::replace(&mut self.scope, Scope::new());
        let mut child = Scope::with_parent(old);
        child.define(name.to_string(), elem_ty, true);
        self.scope = child;
        self.check(body)?;
        let child = mem::replace(&mut self.scope, Scope::new());
        self.scope = child.into_parent().unwrap_or(Scope::new());
        Ok(())
    }

    fn check_fn(
        &mut self,
        name: &str,
        params: &[(String, String)],
        ret: &Option<String>,
        body: &[Stmt],
    ) -> LeoResult<()> {
        let old = mem::replace(&mut self.scope, Scope::new());
        let mut child = Scope::with_parent(old);
        for (pname, pty) in params {
            child.define(pname.clone(), LeoType::parse(pty)?, false);
        }
        self.scope = child;
        self.check(body)?;
        let child = mem::replace(&mut self.scope, Scope::new());
        self.scope = child.into_parent().unwrap_or(Scope::new());
        let param_types: Vec<LeoType> = params
            .iter()
            .map(|(_, t)| LeoType::parse(t))
            .collect::<LeoResult<Vec<_>>>()?;
        let ret_ty = Box::new(
            ret.as_ref()
                .map(|r| LeoType::parse(r))
                .transpose()?
                .unwrap_or(LeoType::Unit),
        );
        self.scope
            .define(name.to_string(), LeoType::Fn(param_types, ret_ty), false);
        Ok(())
    }

    fn check_struct(&mut self, name: &str, _fields: &[(String, String)]) -> LeoResult<()> {
        self.scope
            .define(name.to_string(), LeoType::Struct(name.to_string()), false);
        Ok(())
    }

    fn check_enum(&mut self, name: &str, variants: &[(String, Vec<Expr>)]) -> LeoResult<()> {
        for (i, (vname, _exprs)) in variants.iter().enumerate() {
            let qualified = format!("{}::{}", name, vname);
            self.enum_variants
                .insert(qualified.clone(), (name.to_string(), i as u32));
            self.functions.insert(qualified);
        }
        self.scope
            .define(name.to_string(), LeoType::Enum(name.to_string()), false);
        Ok(())
    }

    /// Check expression and return its inferred type.
    pub(crate) fn check_expr(&mut self, expr: &Expr) -> LeoResult<LeoType> {
        match expr {
            Expr::Number(_, _) => Ok(LeoType::I64),
            Expr::IntLiteral(_, ty, _) => Ok(ty.clone()),
            Expr::Float(_, _) => Ok(LeoType::F64),
            Expr::FloatLiteral(_, ty, _) => Ok(ty.clone()),
            Expr::String(_, _) => Ok(LeoType::Str),
            Expr::Char(_, _) => Ok(LeoType::Char),
            Expr::Bool(_, _) => Ok(LeoType::Bool),
            Expr::Unit(_) => Ok(LeoType::Unit),
            Expr::Tuple(elems, _) => {
                let elem_tys = elems
                    .iter()
                    .map(|elem| self.check_expr(elem))
                    .collect::<LeoResult<Vec<_>>>()?;
                Ok(LeoType::Tuple(elem_tys))
            }
            Expr::Ident(name, _) => {
                if let Some(ty) = self.constants.get(name) {
                    return Ok(ty.clone());
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
            Expr::Call(callee, args, _, _) => self.check_call(callee, args),
            Expr::Index(obj, idx, _) => self.check_index(obj, idx),
            Expr::Select(_, _, _) => Ok(LeoType::Unknown),
            Expr::Array(elements, _) => {
                if elements.is_empty() {
                    return Err(LeoError::new(
                        ErrorKind::Semantic,
                        ErrorCode::SemaTypeMismatch,
                        "empty array requires a type annotation".into(),
                    ));
                }
                let mut elem_ty: Option<LeoType> = None;
                for e in elements {
                    let ty = self.check_expr(e)?;
                    if let Some(prev) = &elem_ty {
                        if *prev != ty && *prev != LeoType::Unknown && ty != LeoType::Unknown {
                            return Err(LeoError::new(
                                ErrorKind::Semantic,
                                ErrorCode::SemaTypeMismatch,
                                format!("array element type mismatch: {} vs {}", prev, ty),
                            ));
                        }
                    } else {
                        elem_ty = Some(ty);
                    }
                }
                let elem = elem_ty.unwrap_or(LeoType::Unknown);
                Ok(LeoType::Array(Box::new(elem), elements.len()))
            }
            Expr::ArrayRepeat(val, count, _) => {
                let elem_ty = self.check_expr(val)?;
                self.check_expr(count)?;
                let len = match count.as_ref() {
                    Expr::Number(n, _) if *n >= 0 => *n as usize,
                    Expr::IntLiteral(n, _, _) => (*n).try_into().map_err(|_| {
                        LeoError::new(
                            ErrorKind::Semantic,
                            ErrorCode::SemaTypeMismatch,
                            format!("array length too large: {}", n),
                        )
                    })?,
                    Expr::Number(n, _) => {
                        return Err(LeoError::new(
                            ErrorKind::Semantic,
                            ErrorCode::SemaTypeMismatch,
                            format!("array length must be non-negative, got {}", n),
                        ));
                    }
                    _ => {
                        return Err(LeoError::new(
                            ErrorKind::Semantic,
                            ErrorCode::SemaTypeMismatch,
                            "array repeat length must be an integer literal".into(),
                        ));
                    }
                };
                Ok(LeoType::Array(Box::new(elem_ty), len))
            }
            Expr::StructInit(name, fields, _, _) => {
                for (_, val) in fields {
                    self.check_expr(val)?;
                }
                Ok(LeoType::Struct(name.clone()))
            }
            Expr::Lambda(params, body, _) => self.check_lambda(params, body),
            Expr::If(_, _, _, _) => Ok(LeoType::Unknown),
            Expr::Block(exprs, _) => {
                let mut ty = LeoType::Unit;
                for e in exprs {
                    ty = self.check_expr(e)?;
                }
                Ok(ty)
            }
            Expr::Await(e, _) => self.check_expr(e),
            Expr::Match(scrutinee, arms, _) => {
                self.check_expr(scrutinee)?;
                for (pattern, body) in arms {
                    self.check_pattern(pattern)?;
                    self.check_expr(body)?;
                }
                Ok(LeoType::Unknown)
            }
        }
    }

    fn check_binary(&mut self, op: &BinOp, left: &Expr, right: &Expr) -> LeoResult<LeoType> {
        let lt = self.check_expr(left)?;
        let rt = self.check_expr(right)?;
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                if lt == LeoType::Bool || rt == LeoType::Bool {
                    return Err(LeoError::new(
                        ErrorKind::Semantic,
                        ErrorCode::SemaTypeMismatch,
                        "arithmetic on bool is not allowed".into(),
                    ));
                }
                if lt != rt && lt != LeoType::Unknown && rt != LeoType::Unknown {
                    return Err(LeoError::new(
                        ErrorKind::Semantic,
                        ErrorCode::SemaTypeMismatch,
                        format!("type mismatch: {} vs {}", lt, rt),
                    ));
                }
                if lt != LeoType::Unknown {
                    Ok(lt)
                } else {
                    Ok(rt)
                }
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                Ok(LeoType::Bool)
            }
            BinOp::And | BinOp::Or => Ok(LeoType::Bool),
            BinOp::BitAnd | BinOp::BitOr | BinOp::Shl | BinOp::Shr => Ok(lt),
        }
    }

    fn check_unary(&mut self, op: &UnOp, e: &Expr) -> LeoResult<LeoType> {
        let ty = self.check_expr(e)?;
        match op {
            UnOp::Neg | UnOp::Minus => {
                let is_signed_numeric = matches!(
                    ty,
                    LeoType::I8
                        | LeoType::I16
                        | LeoType::I32
                        | LeoType::I64
                        | LeoType::I128
                        | LeoType::ISize
                        | LeoType::F32
                        | LeoType::F64
                        | LeoType::Unknown
                );
                if !is_signed_numeric {
                    return Err(LeoError::new(
                        ErrorKind::Semantic,
                        ErrorCode::SemaTypeMismatch,
                        format!("cannot negate type {}", ty),
                    ));
                }
                Ok(ty)
            }
            UnOp::Not => {
                if ty != LeoType::Unknown && ty != LeoType::Bool {
                    return Err(LeoError::new(
                        ErrorKind::Semantic,
                        ErrorCode::SemaTypeMismatch,
                        format!("cannot 'not' type {}", ty),
                    ));
                }
                Ok(LeoType::Bool)
            }
            UnOp::Ref => Ok(LeoType::Ptr),
            UnOp::Deref => {
                if ty != LeoType::Unknown && !ty.is_pointer() {
                    return Err(LeoError::new(
                        ErrorKind::Semantic,
                        ErrorCode::SemaTypeMismatch,
                        format!("cannot dereference non-pointer type {}", ty),
                    ));
                }
                Ok(LeoType::Unknown)
            }
        }
    }

    fn check_call(&mut self, callee: &Expr, args: &[Expr]) -> LeoResult<LeoType> {
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
        Ok(LeoType::Unknown)
    }

    /// Integer/float literals are coercible to compatible numeric annotation types.
    /// e.g. `42` (I64) is coercible to i32 annotation; `3.14` (F64) to f32.
    /// Bool and Char are excluded — they are not numeric in the assignment sense.
    fn types_coercible(from: &LeoType, to: &LeoType) -> bool {
        match (from, to) {
            (LeoType::I64, t) => matches!(
                t,
                LeoType::I8
                    | LeoType::I16
                    | LeoType::I32
                    | LeoType::I64
                    | LeoType::U8
                    | LeoType::U16
                    | LeoType::U32
                    | LeoType::U64
                    | LeoType::F32
                    | LeoType::F64
            ),
            (LeoType::F64, LeoType::F32) => true,
            _ => false,
        }
    }

    fn check_index(&mut self, obj: &Expr, idx: &Expr) -> LeoResult<LeoType> {
        let obj_ty = self.check_expr(obj)?;
        let idx_ty = self.check_expr(idx)?;
        if idx_ty != LeoType::Unknown && !idx_ty.is_integer() {
            return Err(LeoError::new(
                ErrorKind::Semantic,
                ErrorCode::SemaTypeMismatch,
                format!("index must be integer, got {}", idx_ty),
            ));
        }
        match obj_ty {
            LeoType::Array(elem, _) => Ok(*elem),
            LeoType::Vec(elem) => Ok(*elem),
            LeoType::Str => Ok(LeoType::Char),
            _ => Ok(LeoType::Unknown),
        }
    }

    fn check_lambda(&mut self, params: &[(String, String)], body: &Expr) -> LeoResult<LeoType> {
        let old = mem::replace(&mut self.scope, Scope::new());
        let mut child = Scope::with_parent(old);
        for (name, ty) in params {
            child.define(name.clone(), LeoType::parse(ty)?, false);
        }
        self.scope = child;
        let ret = self.check_expr(body)?;
        let child = mem::replace(&mut self.scope, Scope::new());
        self.scope = child.into_parent().unwrap_or(Scope::new());
        let param_types: Vec<LeoType> = params
            .iter()
            .map(|(_, t)| LeoType::parse(t))
            .collect::<LeoResult<Vec<_>>>()?;
        Ok(LeoType::Fn(param_types, Box::new(ret)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::expr::Expr;
    use crate::common::span::Span;
    use crate::common::types::LeoType;

    #[test]
    fn test_check_number() {
        let mut c = Checker::new();
        let ty = c.check_expr(&Expr::Number(42, Span::dummy())).unwrap();
        assert_eq!(ty, LeoType::I64);
    }

    #[test]
    fn test_check_typed_literals() {
        let mut c = Checker::new();
        let int_ty = c
            .check_expr(&Expr::IntLiteral(255, LeoType::U8, Span::dummy()))
            .unwrap();
        let float_ty = c
            .check_expr(&Expr::FloatLiteral(1.5, LeoType::F32, Span::dummy()))
            .unwrap();
        assert_eq!(int_ty, LeoType::U8);
        assert_eq!(float_ty, LeoType::F32);
    }

    #[test]
    fn test_check_tuple() {
        let mut c = Checker::new();
        let ty = c
            .check_expr(&Expr::Tuple(
                vec![
                    Expr::Number(1, Span::dummy()),
                    Expr::Bool(true, Span::dummy()),
                ],
                Span::dummy(),
            ))
            .unwrap();
        assert_eq!(ty, LeoType::Tuple(vec![LeoType::I64, LeoType::Bool]));
    }

    #[test]
    fn test_check_bool() {
        let mut c = Checker::new();
        let ty = c.check_expr(&Expr::Bool(true, Span::dummy())).unwrap();
        assert_eq!(ty, LeoType::Bool);
    }

    #[test]
    fn test_check_let_and_resolve() {
        let mut c = Checker::new();
        let stmt = Stmt::Let("x".into(), Some("i32".into()), None);
        c.check_stmt(&stmt).unwrap();
        let entry = c.scope.resolve("x").unwrap();
        assert_eq!(entry.ty, LeoType::I32);
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
            vec![],
            Span::dummy(),
        );
        c.check_stmt(&stmt).unwrap();
        assert!(c.scope.resolve("add").is_some());
    }

    #[test]
    fn test_const_preserves_declared_type() {
        let mut c = Checker::new();
        let stmt = Stmt::Const(
            "FLAG".into(),
            "bool".into(),
            Expr::Bool(true, Span::dummy()),
            Span::dummy(),
        );
        c.check_stmt(&stmt).unwrap();
        let ty = c
            .check_expr(&Expr::Ident("FLAG".into(), Span::dummy()))
            .unwrap();
        assert_eq!(ty, LeoType::Bool);
    }

    #[test]
    fn test_empty_array_requires_type_annotation() {
        let mut c = Checker::new();
        let stmt = Stmt::Let(
            "items".into(),
            None,
            Some(Expr::Array(vec![], Span::dummy())),
        );
        let result = c.check_stmt(&stmt);
        assert!(result.is_err());
    }

    #[test]
    fn test_typed_empty_array_is_allowed() {
        let mut c = Checker::new();
        let stmt = Stmt::Let(
            "items".into(),
            Some("[i64; 0]".into()),
            Some(Expr::Array(vec![], Span::dummy())),
        );
        c.check_stmt(&stmt).unwrap();
        assert_eq!(
            c.scope.resolve("items").unwrap().ty,
            LeoType::Array(Box::new(LeoType::I64), 0)
        );
    }

    #[test]
    fn test_let_annotation_mismatch_is_error() {
        let mut c = Checker::new();
        // bool annotation, integer initializer — not coercible
        let stmt = Stmt::Let(
            "x".into(),
            Some("bool".into()),
            Some(Expr::Number(1, Span::dummy())),
        );
        assert!(c.check_stmt(&stmt).is_err());
    }

    #[test]
    fn test_let_i32_annotation_with_literal_is_ok() {
        let mut c = Checker::new();
        // i32 annotation, i64 literal — coercible
        let stmt = Stmt::Let(
            "x".into(),
            Some("i32".into()),
            Some(Expr::Number(42, Span::dummy())),
        );
        assert!(c.check_stmt(&stmt).is_ok());
    }

    #[test]
    fn test_assign_undefined_is_error() {
        let mut c = Checker::new();
        let stmt = Stmt::Assign("x".into(), Expr::Number(1, Span::dummy()));
        assert!(c.check_stmt(&stmt).is_err());
    }

    #[test]
    fn test_neg_bool_is_error() {
        let mut c = Checker::new();
        let expr = Expr::Unary(
            crate::ast::expr::UnOp::Neg,
            Box::new(Expr::Bool(true, Span::dummy())),
            Span::dummy(),
        );
        assert!(c.check_expr(&expr).is_err());
    }

    #[test]
    fn test_array_repeat_preserves_length() {
        let mut c = Checker::new();
        let expr = Expr::ArrayRepeat(
            Box::new(Expr::Number(0, Span::dummy())),
            Box::new(Expr::Number(4, Span::dummy())),
            Span::dummy(),
        );
        let ty = c.check_expr(&expr).unwrap();
        assert_eq!(ty, LeoType::Array(Box::new(LeoType::I64), 4));
    }

    #[test]
    fn test_array_repeat_negative_length_is_error() {
        let mut c = Checker::new();
        let expr = Expr::ArrayRepeat(
            Box::new(Expr::Number(0, Span::dummy())),
            Box::new(Expr::Number(-1, Span::dummy())),
            Span::dummy(),
        );
        assert!(c.check_expr(&expr).is_err());
    }

    #[test]
    fn test_const_i32_annotation_with_literal_is_ok() {
        let mut c = Checker::new();
        let stmt = Stmt::Const(
            "C".into(),
            "i32".into(),
            Expr::Number(99, Span::dummy()),
            Span::dummy(),
        );
        assert!(c.check_stmt(&stmt).is_ok());
    }

    #[test]
    fn test_assign_coercible_literal_is_ok() {
        let mut c = Checker::new();
        // define x: i32, then assign with i64 literal (coercible)
        let let_stmt = Stmt::Let(
            "x".into(),
            Some("i32".into()),
            Some(Expr::Number(1, Span::dummy())),
        );
        c.check_stmt(&let_stmt).unwrap();
        let assign = Stmt::Assign("x".into(), Expr::Number(2, Span::dummy()));
        assert!(c.check_stmt(&assign).is_ok());
    }

    #[test]
    fn test_array_repeat_variable_length_is_error() {
        let mut c = Checker::new();
        // [0; n] where n is a variable — must be rejected
        let expr = Expr::ArrayRepeat(
            Box::new(Expr::Number(0, Span::dummy())),
            Box::new(Expr::Ident("n".into(), Span::dummy())),
            Span::dummy(),
        );
        assert!(c.check_expr(&expr).is_err());
    }
}
