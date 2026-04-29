use crate::ast::expr::{BinOp, Expr, UnOp};
use crate::ast::stmt::Stmt;
use crate::common::{ErrorCode, LeoError};
use crate::lint::common::semantic_error;
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct SymbolInfo {
    ty: String,
    is_const: bool,
}

#[derive(Clone)]
struct StructInfo {
    fields: HashMap<String, String>,
}

#[derive(Clone)]
struct EnumInfo {
    variants: HashMap<String, usize>,
}

/// Semantic-level linter for non-fatal AST validation.
pub struct SemanticLinter;

struct SemanticLintCtx {
    scopes: Vec<HashMap<String, SymbolInfo>>,
    functions: HashMap<String, usize>,
    structs: HashMap<String, StructInfo>,
    enums: HashMap<String, EnumInfo>,
    traits: HashSet<String>,
    imports: HashSet<String>,
    loop_depth: usize,
    fn_depth: usize,
    errors: Vec<LeoError>,
}

impl SemanticLinter {
    /// Check semantic consistency that should be reported as lint output.
    pub fn lint(stmts: &[Stmt]) -> Vec<LeoError> {
        let mut ctx = SemanticLintCtx::new();
        ctx.collect_top_level(stmts);
        ctx.check_stmts(stmts);
        ctx.errors
    }
}

impl SemanticLintCtx {
    fn new() -> Self {
        let mut functions = HashMap::new();
        for (name, argc) in [
            ("println", 1),
            ("print", 1),
            ("panic", 1),
            ("assert", 1),
            ("str_len", 1),
            ("str_char_at", 2),
            ("str_slice", 3),
            ("str_concat", 2),
            ("vec_new", 0),
            ("vec_push", 2),
            ("vec_get", 2),
            ("vec_len", 1),
            ("file_read", 1),
            ("file_write", 2),
            ("char_to_str", 1),
            ("is_digit", 1),
            ("is_alpha", 1),
            ("is_alnum", 1),
            ("to_string", 1),
            ("free", 1),
        ] {
            functions.insert(name.to_string(), argc);
        }
        Self {
            scopes: vec![HashMap::new()],
            functions,
            structs: HashMap::new(),
            enums: HashMap::new(),
            traits: HashSet::new(),
            imports: HashSet::new(),
            loop_depth: 0,
            fn_depth: 0,
            errors: Vec::new(),
        }
    }

    fn collect_top_level(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt {
                Stmt::Function(name, params, _, _, _, _)
                | Stmt::AsyncFunction(name, params, _, _, _, _) => {
                    self.define_global(name, format!("fn({})", params.len()), false);
                    self.functions.insert(name.clone(), params.len());
                }
                Stmt::Struct(name, fields, _, _) => {
                    self.define_global(name, "struct".into(), true);
                    self.structs.insert(
                        name.clone(),
                        StructInfo {
                            fields: fields.iter().cloned().collect(),
                        },
                    );
                }
                Stmt::Enum(name, variants, _) => {
                    self.define_global(name, "enum".into(), true);
                    let mut variant_map = HashMap::new();
                    for (variant, payload) in variants {
                        variant_map.insert(variant.clone(), payload.len());
                        self.functions
                            .insert(format!("{}::{}", name, variant), payload.len());
                    }
                    self.enums.insert(
                        name.clone(),
                        EnumInfo {
                            variants: variant_map,
                        },
                    );
                }
                Stmt::Trait(name, _, _) => {
                    self.define_global(name, "trait".into(), true);
                    self.traits.insert(name.clone());
                }
                Stmt::Const(name, ty, _, _) => self.define_global(name, ty.clone(), true),
                Stmt::Pub(inner) => self.collect_top_level(std::slice::from_ref(inner)),
                _ => {}
            }
        }
    }

    fn check_stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.check_stmt(stmt);
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(expr) => {
                self.check_expr(expr);
            }
            Stmt::Let(name, ty, init) => {
                if let Some(expr) = init {
                    let inferred = self.check_expr(expr);
                    if let (Some(expected), Some(actual)) = (ty.as_ref(), inferred) {
                        self.check_type_match(expected, &actual, "let binding type mismatch");
                    }
                }
                self.define_local(name, ty.clone().unwrap_or_else(|| "unknown".into()), false);
            }
            Stmt::Assign(name, expr) | Stmt::MutAssign(name, expr) => {
                let actual = self.check_expr(expr);
                if let Some(symbol) = self.resolve(name).cloned() {
                    if symbol.is_const {
                        self.push_error(
                            ErrorCode::SemaTypeMismatch,
                            format!("cannot assign to constant: {}", name),
                        );
                    }
                    if let Some(actual) = actual {
                        self.check_type_match(&symbol.ty, &actual, "assignment type mismatch");
                    }
                } else {
                    self.push_error(
                        ErrorCode::SemaUndefinedVariable,
                        format!("undefined variable: {}", name),
                    );
                }
            }
            Stmt::FieldAssign(obj, _, expr) => {
                self.check_expr(obj);
                self.check_expr(expr);
            }
            Stmt::If(branches, els, _) => {
                for (cond, body) in branches {
                    self.check_expr(cond);
                    self.with_scope(|ctx| ctx.check_stmts(body));
                }
                if let Some(body) = els {
                    self.with_scope(|ctx| ctx.check_stmts(body));
                }
            }
            Stmt::While(cond, body, _) => {
                self.check_expr(cond);
                self.loop_depth += 1;
                self.with_scope(|ctx| ctx.check_stmts(body));
                self.loop_depth -= 1;
            }
            Stmt::For(name, iter, body, _) => {
                self.check_expr(iter);
                self.loop_depth += 1;
                self.with_scope(|ctx| {
                    ctx.define_local(name, "unknown".into(), false);
                    ctx.check_stmts(body);
                });
                self.loop_depth -= 1;
            }
            Stmt::Function(_, params, ret, body, _, _)
            | Stmt::AsyncFunction(_, params, ret, body, _, _) => {
                self.check_duplicate_params(params);
                self.fn_depth += 1;
                self.with_scope(|ctx| {
                    for (name, ty) in params {
                        ctx.define_local(name, ty.clone(), false);
                    }
                    ctx.check_stmts(body);
                });
                self.fn_depth -= 1;
                if let Some(ret_ty) = ret {
                    self.check_return_statements(body, ret_ty);
                }
            }
            Stmt::Return(expr, _) => {
                if self.fn_depth == 0 {
                    self.push_error(
                        ErrorCode::ParserInvalidSyntax,
                        "return outside function".into(),
                    );
                }
                if let Some(expr) = expr {
                    self.check_expr(expr);
                }
            }
            Stmt::Break(expr, _) => {
                if self.loop_depth == 0 {
                    self.push_error(ErrorCode::ParserInvalidSyntax, "break outside loop".into());
                }
                if let Some(expr) = expr {
                    self.check_expr(expr);
                }
            }
            Stmt::Continue(_) => {
                if self.loop_depth == 0 {
                    self.push_error(
                        ErrorCode::ParserInvalidSyntax,
                        "continue outside loop".into(),
                    );
                }
            }
            Stmt::Import(name, items, _) => self.check_import(name, items.as_deref()),
            Stmt::FromImport(module, items, _) => self.check_from_import(module, items),
            Stmt::Module(_, body, _) => self.with_scope(|ctx| ctx.check_stmts(body)),
            Stmt::Struct(_, fields, _, _) => self.check_duplicate_named_items(fields, "field"),
            Stmt::Enum(_, variants, _) => self.check_duplicate_variants(variants),
            Stmt::Trait(_, methods, _) => {
                let mut seen = HashSet::new();
                for (name, body) in methods {
                    if !seen.insert(name) {
                        self.push_error(
                            ErrorCode::SemaDuplicateDefinition,
                            format!("duplicate trait item: {}", name),
                        );
                    }
                    self.check_stmts(body);
                }
            }
            Stmt::Impl(name, trait_name, methods, _, _) => {
                self.check_impl_target(name, trait_name.as_deref());
                let mut seen = HashSet::new();
                for method in methods {
                    if let Stmt::Function(name, _, _, _, _, _) = method {
                        if !seen.insert(name) {
                            self.push_error(
                                ErrorCode::SemaDuplicateDefinition,
                                format!("duplicate method: {}", name),
                            );
                        }
                    }
                    self.check_stmt(method);
                }
            }
            Stmt::Pub(inner) => self.check_stmt(inner),
            Stmt::Const(name, ty, expr, _) => {
                let actual = self.check_expr(expr);
                if let Some(actual) = actual {
                    self.check_type_match(ty, &actual, "const type mismatch");
                }
                if self.fn_depth > 0
                    || self.scopes.len() > 1
                    || self.resolve_current(name).is_none()
                {
                    self.define_local(name, ty.clone(), true);
                }
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Ident(name, span) => self.resolve_ident(name, *span),
            Expr::Number(_, _) => Some("i64".into()),
            Expr::IntLiteral(_, ty, _) => Some(ty.to_string()),
            Expr::Float(_, _) => Some("f64".into()),
            Expr::FloatLiteral(_, ty, _) => Some(ty.to_string()),
            Expr::String(_, _) => Some("str".into()),
            Expr::Char(_, _) => Some("char".into()),
            Expr::Bool(_, _) => Some("bool".into()),
            Expr::Unit(_) => Some("unit".into()),
            Expr::Binary(op, left, right, _) => self.check_binary(op, left, right),
            Expr::Unary(op, inner, _) => {
                let inner_ty = self.check_expr(inner);
                match op {
                    UnOp::Not => Some("bool".into()),
                    _ => inner_ty,
                }
            }
            Expr::Call(callee, args, _, _) => self.check_call(callee, args),
            Expr::Index(obj, idx, _) => {
                self.check_expr(obj);
                self.check_expr(idx);
                Some("unknown".into())
            }
            Expr::Select(obj, _, _) => {
                self.check_expr(obj);
                Some("unknown".into())
            }
            Expr::Array(elements, _) => {
                for elem in elements {
                    self.check_expr(elem);
                }
                Some("array".into())
            }
            Expr::Tuple(elements, _) => {
                let types = elements
                    .iter()
                    .map(|elem| self.check_expr(elem).unwrap_or_else(|| "unknown".into()))
                    .collect::<Vec<_>>();
                Some(format!("({})", types.join(", ")))
            }
            Expr::ArrayRepeat(value, count, _) => {
                self.check_expr(value);
                self.check_expr(count);
                Some("array".into())
            }
            Expr::StructInit(name, fields, _, _) => {
                self.check_struct_init(name, fields);
                for (_, value) in fields {
                    self.check_expr(value);
                }
                Some(name.clone())
            }
            Expr::Lambda(params, body, _) => {
                self.check_duplicate_params(params);
                self.with_scope(|ctx| {
                    for (name, ty) in params {
                        ctx.define_local(name, ty.clone(), false);
                    }
                    ctx.check_expr(body);
                });
                Some("fn".into())
            }
            Expr::If(cond, then_expr, else_expr, _) => {
                self.check_expr(cond);
                let then_ty = self.check_expr(then_expr);
                if let Some(else_expr) = else_expr {
                    let else_ty = self.check_expr(else_expr);
                    if let (Some(t), Some(e)) = (&then_ty, &else_ty) {
                        self.check_type_match(t, e, "if branch type mismatch");
                    }
                }
                then_ty
            }
            Expr::Block(exprs, _) => {
                let mut ty = Some("unit".to_string());
                self.with_scope(|ctx| {
                    for expr in exprs {
                        ty = ctx.check_expr(expr);
                    }
                });
                ty
            }
            Expr::Await(inner, _) => self.check_expr(inner),
            Expr::Match(scrutinee, arms, _) => {
                self.check_expr(scrutinee);
                let mut result_ty: Option<String> = None;
                for (pattern, body) in arms {
                    self.with_scope(|ctx| {
                        ctx.bind_pattern(pattern);
                        let body_ty = ctx.check_expr(body);
                        if let Some(body_ty) = body_ty {
                            if let Some(prev) = &result_ty {
                                ctx.check_type_match(prev, &body_ty, "match arm type mismatch");
                            } else {
                                result_ty = Some(body_ty);
                            }
                        }
                    });
                }
                result_ty.or_else(|| Some("unknown".into()))
            }
        }
    }

    fn check_call(&mut self, callee: &Expr, args: &[Expr]) -> Option<String> {
        if let Expr::Ident(name, span) = callee {
            if let Some(expected) = self.enum_variant_payload_len(name) {
                if expected != args.len() {
                    self.errors.push(semantic_error(
                        ErrorCode::SemaTypeMismatch,
                        format!(
                            "enum variant '{}' expects {} payload values, got {}",
                            name,
                            expected,
                            args.len()
                        ),
                        Some(*span),
                    ));
                }
            } else if name.contains("::") {
                self.errors.push(semantic_error(
                    ErrorCode::SemaUndefinedFunction,
                    format!("undefined enum variant: {}", name),
                    Some(*span),
                ));
            } else if let Some(expected) = self.functions.get(name).copied() {
                if expected != args.len() {
                    self.errors.push(semantic_error(
                        ErrorCode::SemaTypeMismatch,
                        format!(
                            "function '{}' expects {} arguments, got {}",
                            name,
                            expected,
                            args.len()
                        ),
                        Some(*span),
                    ));
                }
            } else if self.resolve(name).is_none() {
                self.errors.push(semantic_error(
                    ErrorCode::SemaUndefinedFunction,
                    format!("undefined function: {}", name),
                    Some(*span),
                ));
            }
        } else {
            self.check_expr(callee);
        }
        for arg in args {
            self.check_expr(arg);
        }
        Some("unknown".into())
    }

    fn check_binary(&mut self, op: &BinOp, left: &Expr, right: &Expr) -> Option<String> {
        let left_ty = self.check_expr(left);
        let right_ty = self.check_expr(right);
        if let (Some(left_ty), Some(right_ty)) = (&left_ty, &right_ty) {
            if !self.types_compatible(left_ty, right_ty) {
                self.push_error(
                    ErrorCode::SemaTypeMismatch,
                    format!("type mismatch: {} vs {}", left_ty, right_ty),
                );
            }
        }
        match op {
            BinOp::Eq
            | BinOp::Ne
            | BinOp::Lt
            | BinOp::Le
            | BinOp::Gt
            | BinOp::Ge
            | BinOp::And
            | BinOp::Or => Some("bool".into()),
            _ => left_ty.or(right_ty),
        }
    }

    fn check_return_statements(&mut self, body: &[Stmt], ret_ty: &str) {
        for stmt in body {
            match stmt {
                Stmt::Return(Some(expr), _) => {
                    if let Some(actual) = self.check_expr(expr) {
                        self.check_type_match(ret_ty, &actual, "return type mismatch");
                    }
                }
                Stmt::If(branches, els, _) => {
                    for (_, body) in branches {
                        self.check_return_statements(body, ret_ty);
                    }
                    if let Some(body) = els {
                        self.check_return_statements(body, ret_ty);
                    }
                }
                Stmt::While(_, body, _) | Stmt::For(_, _, body, _) | Stmt::Module(_, body, _) => {
                    self.check_return_statements(body, ret_ty);
                }
                Stmt::Pub(inner) => {
                    self.check_return_statements(std::slice::from_ref(inner), ret_ty)
                }
                _ => {}
            }
        }
    }

    fn bind_pattern(&mut self, pattern: &Expr) {
        match pattern {
            Expr::Ident(name, _) if name != "_" && !name.contains("::") => {
                self.define_local(name, "unknown".into(), false);
            }
            Expr::Call(callee, args, _, _) => {
                self.check_expr(callee);
                for arg in args {
                    self.bind_pattern(arg);
                }
            }
            _ => {
                self.check_expr(pattern);
            }
        }
    }

    fn check_import(&mut self, module: &str, items: Option<&[String]>) {
        if let Some(items) = items {
            let mut seen_items = HashSet::new();
            for item in items {
                if !seen_items.insert(item) {
                    self.push_error(
                        ErrorCode::SemaDuplicateDefinition,
                        format!("duplicate import item: {}::{}", module, item),
                    );
                }
                self.define_import_key(&format!("{}::{}", module, item));
            }
        } else {
            self.define_import_key(module);
        }
    }

    fn check_from_import(&mut self, module: &str, items: &[String]) {
        let mut seen_items = HashSet::new();
        for item in items {
            if !seen_items.insert(item) {
                self.push_error(
                    ErrorCode::SemaDuplicateDefinition,
                    format!("duplicate import item: {}::{}", module, item),
                );
            }
            self.define_import_key(&format!("{}::{}", module, item));
        }
    }

    fn define_import_key(&mut self, key: &str) {
        if !self.imports.insert(key.to_string()) {
            self.push_error(
                ErrorCode::SemaDuplicateDefinition,
                format!("duplicate import: {}", key),
            );
        }
    }

    fn check_impl_target(&mut self, name: &str, trait_name: Option<&str>) {
        if !self.structs.contains_key(name) {
            self.push_error(
                ErrorCode::SemaUndefinedVariable,
                format!("impl target is not a known struct: {}", name),
            );
        }
        if let Some(trait_name) = trait_name {
            if !self.traits.contains(trait_name) {
                self.push_error(
                    ErrorCode::SemaUndefinedVariable,
                    format!("impl references unknown trait: {}", trait_name),
                );
            }
        }
    }

    fn resolve_ident(&mut self, name: &str, span: crate::common::Span) -> Option<String> {
        if name == "_" {
            return Some("unknown".into());
        }
        if let Some(symbol) = self.resolve(name) {
            Some(symbol.ty.clone())
        } else if name.contains("::") {
            if self.enum_variant_payload_len(name).is_some() {
                Some("fn".into())
            } else {
                self.errors.push(semantic_error(
                    ErrorCode::SemaUndefinedFunction,
                    format!("undefined enum variant: {}", name),
                    Some(span),
                ));
                None
            }
        } else if self.functions.contains_key(name) {
            Some("fn".into())
        } else {
            self.errors.push(semantic_error(
                ErrorCode::SemaUndefinedVariable,
                format!("undefined variable: {}", name),
                Some(span),
            ));
            None
        }
    }

    fn check_duplicate_params(&mut self, params: &[(String, String)]) {
        self.check_duplicate_named_items(params, "parameter");
    }

    fn check_duplicate_named_items(&mut self, items: &[(String, String)], kind: &str) {
        let mut seen = HashSet::new();
        for (name, _) in items {
            if !seen.insert(name) {
                self.push_error(
                    ErrorCode::SemaDuplicateDefinition,
                    format!("duplicate {}: {}", kind, name),
                );
            }
        }
    }

    fn check_duplicate_variants(&mut self, variants: &[(String, Vec<Expr>)]) {
        let mut seen = HashSet::new();
        for (name, _) in variants {
            if !seen.insert(name) {
                self.push_error(
                    ErrorCode::SemaDuplicateDefinition,
                    format!("duplicate enum variant: {}", name),
                );
            }
        }
    }

    fn check_struct_init(&mut self, name: &str, fields: &[(String, Expr)]) {
        let mut seen = HashSet::new();
        for (name, _) in fields {
            if !seen.insert(name) {
                self.push_error(
                    ErrorCode::SemaDuplicateDefinition,
                    format!("duplicate field: {}", name),
                );
            }
        }

        let Some(info) = self.structs.get(name).cloned() else {
            self.push_error(
                ErrorCode::SemaUndefinedVariable,
                format!("undefined struct: {}", name),
            );
            return;
        };

        for expected in info.fields.keys() {
            if !fields.iter().any(|(field, _)| field == expected) {
                self.push_error(
                    ErrorCode::SemaTypeMismatch,
                    format!("struct '{}' missing field: {}", name, expected),
                );
            }
        }

        for (field, value) in fields {
            match info.fields.get(field) {
                Some(expected_ty) => {
                    if let Some(actual_ty) = self.infer_expr_type(value) {
                        self.check_type_match(
                            expected_ty,
                            &actual_ty,
                            "struct field type mismatch",
                        );
                    }
                }
                None => self.push_error(
                    ErrorCode::SemaTypeMismatch,
                    format!("struct '{}' has no field: {}", name, field),
                ),
            }
        }
    }

    fn define_global(&mut self, name: &str, ty: String, is_const: bool) {
        self.define_in_scope(0, name, ty, is_const);
    }

    fn define_local(&mut self, name: &str, ty: String, is_const: bool) {
        let index = self.scopes.len().saturating_sub(1);
        self.define_in_scope(index, name, ty, is_const);
    }

    fn define_in_scope(&mut self, index: usize, name: &str, ty: String, is_const: bool) {
        if self.scopes[index].contains_key(name) {
            self.push_error(
                ErrorCode::SemaDuplicateDefinition,
                format!("duplicate definition: {}", name),
            );
            return;
        }
        self.scopes[index].insert(name.to_string(), SymbolInfo { ty, is_const });
    }

    fn resolve(&self, name: &str) -> Option<&SymbolInfo> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    fn resolve_current(&self, name: &str) -> Option<&SymbolInfo> {
        self.scopes.last().and_then(|scope| scope.get(name))
    }

    fn with_scope(&mut self, f: impl FnOnce(&mut Self)) {
        self.scopes.push(HashMap::new());
        f(self);
        self.scopes.pop();
    }

    fn check_type_match(&mut self, expected: &str, actual: &str, context: &str) {
        if !self.types_compatible(expected, actual) {
            self.push_error(
                ErrorCode::SemaTypeMismatch,
                format!("{}: expected {}, got {}", context, expected, actual),
            );
        }
    }

    fn types_compatible(&self, expected: &str, actual: &str) -> bool {
        expected == actual || expected == "unknown" || actual == "unknown"
    }

    fn enum_variant_payload_len(&self, qualified: &str) -> Option<usize> {
        let (enum_name, variant_name) = qualified.split_once("::")?;
        self.enums
            .get(enum_name)
            .and_then(|info| info.variants.get(variant_name).copied())
    }

    fn infer_expr_type(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Number(_, _) => Some("i64".into()),
            Expr::IntLiteral(_, ty, _) => Some(ty.to_string()),
            Expr::Float(_, _) => Some("f64".into()),
            Expr::FloatLiteral(_, ty, _) => Some(ty.to_string()),
            Expr::String(_, _) => Some("str".into()),
            Expr::Char(_, _) => Some("char".into()),
            Expr::Bool(_, _) => Some("bool".into()),
            Expr::Unit(_) => Some("unit".into()),
            Expr::Tuple(elements, _) => {
                let types = elements
                    .iter()
                    .map(|elem| {
                        self.infer_expr_type(elem)
                            .unwrap_or_else(|| "unknown".into())
                    })
                    .collect::<Vec<_>>();
                Some(format!("({})", types.join(", ")))
            }
            Expr::Ident(name, _) => self.resolve(name).map(|symbol| symbol.ty.clone()),
            Expr::StructInit(name, _, _, _) => Some(name.clone()),
            _ => Some("unknown".into()),
        }
    }

    fn push_error(&mut self, code: ErrorCode, message: String) {
        self.errors.push(semantic_error(code, message, None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Span;

    fn ident(name: &str) -> Expr {
        Expr::Ident(name.to_string(), Span::dummy())
    }

    #[test]
    fn test_empty_program() {
        let errors = SemanticLinter::lint(&[]);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_undefined_variable() {
        let errors = SemanticLinter::lint(&[Stmt::Expr(ident("missing"))]);
        assert!(errors
            .iter()
            .any(|e| e.code == ErrorCode::SemaUndefinedVariable));
    }

    #[test]
    fn test_duplicate_definition() {
        let stmts = vec![
            Stmt::Let("x".into(), Some("i64".into()), None),
            Stmt::Let("x".into(), Some("i64".into()), None),
        ];
        let errors = SemanticLinter::lint(&stmts);
        assert!(errors
            .iter()
            .any(|e| e.code == ErrorCode::SemaDuplicateDefinition));
    }

    #[test]
    fn test_duplicate_parameter() {
        let stmt = Stmt::Function(
            "f".into(),
            vec![("x".into(), "i64".into()), ("x".into(), "i64".into())],
            None,
            vec![],
            vec![],
            Span::dummy(),
        );
        let errors = SemanticLinter::lint(&[stmt]);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("duplicate parameter")));
    }

    #[test]
    fn test_const_assignment() {
        let stmts = vec![
            Stmt::Const(
                "X".into(),
                "i64".into(),
                Expr::Number(1, Span::dummy()),
                Span::dummy(),
            ),
            Stmt::Assign("X".into(), Expr::Number(2, Span::dummy())),
        ];
        let errors = SemanticLinter::lint(&stmts);
        assert!(errors.iter().any(|e| e.message.contains("constant")));
    }

    #[test]
    fn test_break_outside_loop() {
        let errors = SemanticLinter::lint(&[Stmt::Break(None, Span::dummy())]);
        assert!(errors.iter().any(|e| e.message == "break outside loop"));
    }

    #[test]
    fn test_return_outside_function() {
        let errors = SemanticLinter::lint(&[Stmt::Return(None, Span::dummy())]);
        assert!(errors
            .iter()
            .any(|e| e.message == "return outside function"));
    }

    #[test]
    fn test_call_arg_count() {
        let stmt = Stmt::Expr(Expr::Call(
            Box::new(ident("println")),
            vec![],
            vec![],
            Span::dummy(),
        ));
        let errors = SemanticLinter::lint(&[stmt]);
        assert!(errors.iter().any(|e| e.message.contains("expects 1")));
    }

    #[test]
    fn test_assignment_type_mismatch() {
        let stmts = vec![
            Stmt::Let("x".into(), Some("i64".into()), None),
            Stmt::Assign("x".into(), Expr::Bool(true, Span::dummy())),
        ];
        let errors = SemanticLinter::lint(&stmts);
        assert!(errors.iter().any(|e| e.code == ErrorCode::SemaTypeMismatch));
    }

    #[test]
    fn test_match_arm_type_mismatch() {
        let stmt = Stmt::Expr(Expr::Match(
            Box::new(Expr::Number(1, Span::dummy())),
            vec![
                (
                    Expr::Number(1, Span::dummy()),
                    Expr::Number(2, Span::dummy()),
                ),
                (
                    Expr::Ident("_".into(), Span::dummy()),
                    Expr::Bool(true, Span::dummy()),
                ),
            ],
            Span::dummy(),
        ));
        let errors = SemanticLinter::lint(&[stmt]);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("match arm type mismatch")));
    }

    #[test]
    fn test_struct_init_missing_field() {
        let stmts = vec![
            Stmt::Struct(
                "Point".into(),
                vec![("x".into(), "i64".into()), ("y".into(), "i64".into())],
                vec![],
                Span::dummy(),
            ),
            Stmt::Expr(Expr::StructInit(
                "Point".into(),
                vec![("x".into(), Expr::Number(1, Span::dummy()))],
                vec![],
                Span::dummy(),
            )),
        ];
        let errors = SemanticLinter::lint(&stmts);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("missing field: y")));
    }

    #[test]
    fn test_struct_init_extra_field() {
        let stmts = vec![
            Stmt::Struct(
                "Point".into(),
                vec![("x".into(), "i64".into())],
                vec![],
                Span::dummy(),
            ),
            Stmt::Expr(Expr::StructInit(
                "Point".into(),
                vec![
                    ("x".into(), Expr::Number(1, Span::dummy())),
                    ("z".into(), Expr::Number(2, Span::dummy())),
                ],
                vec![],
                Span::dummy(),
            )),
        ];
        let errors = SemanticLinter::lint(&stmts);
        assert!(errors.iter().any(|e| e.message.contains("has no field: z")));
    }

    #[test]
    fn test_struct_init_field_type_mismatch() {
        let stmts = vec![
            Stmt::Struct(
                "Point".into(),
                vec![("x".into(), "i64".into())],
                vec![],
                Span::dummy(),
            ),
            Stmt::Expr(Expr::StructInit(
                "Point".into(),
                vec![("x".into(), Expr::Bool(true, Span::dummy()))],
                vec![],
                Span::dummy(),
            )),
        ];
        let errors = SemanticLinter::lint(&stmts);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("struct field type mismatch")));
    }

    #[test]
    fn test_enum_variant_payload_count() {
        let stmts = vec![
            Stmt::Enum(
                "Token".into(),
                vec![("Number".into(), vec![ident("i64")])],
                Span::dummy(),
            ),
            Stmt::Expr(Expr::Call(
                Box::new(ident("Token::Number")),
                vec![],
                vec![],
                Span::dummy(),
            )),
        ];
        let errors = SemanticLinter::lint(&stmts);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("expects 1 payload")));
    }

    #[test]
    fn test_undefined_enum_variant() {
        let stmts = vec![
            Stmt::Enum("Token".into(), vec![("Eof".into(), vec![])], Span::dummy()),
            Stmt::Expr(ident("Token::Number")),
        ];
        let errors = SemanticLinter::lint(&stmts);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("undefined enum variant")));
    }

    #[test]
    fn test_duplicate_import() {
        let stmts = vec![
            Stmt::Import("math".into(), None, Span::dummy()),
            Stmt::Import("math".into(), None, Span::dummy()),
        ];
        let errors = SemanticLinter::lint(&stmts);
        assert!(errors.iter().any(|e| e.message == "duplicate import: math"));
    }

    #[test]
    fn test_duplicate_from_import_item() {
        let stmts = vec![Stmt::FromImport(
            "math".into(),
            vec!["sqrt".into(), "sqrt".into()],
            Span::dummy(),
        )];
        let errors = SemanticLinter::lint(&stmts);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("duplicate import item: math::sqrt")));
    }

    #[test]
    fn test_impl_unknown_target_and_trait() {
        let stmts = vec![Stmt::Impl(
            "Point".into(),
            Some("Display".into()),
            vec![],
            vec![],
            Span::dummy(),
        )];
        let errors = SemanticLinter::lint(&stmts);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("impl target is not a known struct")));
        assert!(errors
            .iter()
            .any(|e| e.message.contains("impl references unknown trait")));
    }
}
