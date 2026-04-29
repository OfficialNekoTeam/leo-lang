use crate::ast::expr::Expr;
use crate::ast::stmt::Stmt;
use crate::common::{ErrorCode, LeoError, Span};
use crate::lint::common::warning_error;
use std::collections::{HashMap, HashSet};

const HEAP_RETURNING_BUILTINS: &[&str] = &[
    "vec_new",
    "str_concat",
    "str_slice",
    "char_to_str",
    "to_string",
    "file_read",
];

#[derive(Default)]
struct ScopeUsage {
    vars: HashMap<String, bool>,
    imports: HashMap<String, bool>,
    params: HashMap<String, bool>,
}

/// Warning-level linter (demotable).
pub struct WarningLinter;

struct WarningLintCtx {
    scopes: Vec<ScopeUsage>,
    functions: HashSet<String>,
    called_functions: HashSet<String>,
    errors: Vec<LeoError>,
}

impl WarningLinter {
    /// Check for warnings like unused variables and unreachable code.
    pub fn lint(stmts: &[Stmt]) -> Vec<LeoError> {
        let mut ctx = WarningLintCtx::new();
        ctx.collect_functions(stmts);
        ctx.check_stmts(stmts);
        ctx.finish_scope();
        ctx.finish_functions();
        ctx.errors
    }
}

impl WarningLintCtx {
    fn new() -> Self {
        Self {
            scopes: vec![ScopeUsage::default()],
            functions: HashSet::new(),
            called_functions: HashSet::new(),
            errors: Vec::new(),
        }
    }

    fn collect_functions(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt {
                Stmt::Function(name, _, _, _, _, _) | Stmt::AsyncFunction(name, _, _, _, _, _) => {
                    self.functions.insert(name.clone());
                }
                Stmt::Impl(struct_name, _, methods, _, _) => {
                    for method in methods {
                        if let Stmt::Function(name, _, _, _, _, _) = method {
                            self.functions.insert(format!("{}_{}", struct_name, name));
                        }
                    }
                }
                Stmt::Module(_, body, _) => self.collect_functions(body),
                Stmt::Pub(inner) => self.collect_functions(std::slice::from_ref(inner)),
                _ => {}
            }
        }
    }

    fn check_stmts(&mut self, stmts: &[Stmt]) {
        let mut unreachable = false;
        for stmt in stmts {
            if unreachable {
                self.warn("unreachable code".into());
            }
            self.check_stmt(stmt);
            if matches!(
                stmt,
                Stmt::Return(_, _) | Stmt::Break(_, _) | Stmt::Continue(_)
            ) {
                unreachable = true;
            }
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(expr) => {
                self.check_discarded_result(expr);
                self.check_expr(expr);
            }
            Stmt::Let(name, _, init) => {
                self.define_var(name);
                if let Some(init) = init {
                    self.check_expr(init);
                }
            }
            Stmt::Assign(name, expr) | Stmt::MutAssign(name, expr) => {
                self.mark_used(name);
                self.check_expr(expr);
            }
            Stmt::FieldAssign(obj, _, expr) => {
                self.check_expr(obj);
                self.check_expr(expr);
            }
            Stmt::If(branches, els, _) => {
                for (cond, body) in branches {
                    self.check_expr(cond);
                    if body.is_empty() {
                        self.warn("empty if branch".into());
                    }
                    self.with_scope(|ctx| ctx.check_stmts(body));
                }
                if let Some(body) = els {
                    if body.is_empty() {
                        self.warn("empty else branch".into());
                    }
                    self.with_scope(|ctx| ctx.check_stmts(body));
                }
            }
            Stmt::While(cond, body, _) => {
                self.check_expr(cond);
                if body.is_empty() {
                    self.warn("empty while body".into());
                }
                self.with_scope(|ctx| ctx.check_stmts(body));
            }
            Stmt::For(name, iter, body, _) => {
                self.check_expr(iter);
                if body.is_empty() {
                    self.warn("empty for body".into());
                }
                self.with_scope(|ctx| {
                    ctx.define_var(name);
                    ctx.check_stmts(body);
                });
            }
            Stmt::Function(_, params, _, body, _, _)
            | Stmt::AsyncFunction(_, params, _, body, _, _) => {
                if body.is_empty() {
                    self.warn("empty function body".into());
                }
                self.with_scope(|ctx| {
                    for (name, _) in params {
                        ctx.define_param(name);
                    }
                    ctx.check_stmts(body);
                });
            }
            Stmt::Return(expr, _) | Stmt::Break(expr, _) => {
                if let Some(expr) = expr {
                    self.check_expr(expr);
                }
            }
            Stmt::Continue(_) => {}
            Stmt::Import(name, items, _) => {
                if let Some(items) = items {
                    for item in items {
                        self.define_import(item);
                    }
                } else {
                    self.define_import(name);
                }
            }
            Stmt::FromImport(_, items, _) => {
                for item in items {
                    self.define_import(item);
                }
            }
            Stmt::Module(_, body, _) => self.with_scope(|ctx| ctx.check_stmts(body)),
            Stmt::Struct(_, _, _, _) | Stmt::Enum(_, _, _) | Stmt::Trait(_, _, _) => {}
            Stmt::Impl(_, _, methods, _, _) => {
                for method in methods {
                    self.check_stmt(method);
                }
            }
            Stmt::Pub(inner) => self.check_stmt(inner),
            Stmt::Const(name, _, expr, _) => {
                self.define_var(name);
                self.check_expr(expr);
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident(name, _) => self.mark_used(name),
            Expr::Binary(_, left, right, _) => {
                self.check_expr(left);
                self.check_expr(right);
            }
            Expr::Unary(_, inner, _) | Expr::Await(inner, _) => self.check_expr(inner),
            Expr::Call(callee, args, _, _) => {
                if let Expr::Ident(name, _) = callee.as_ref() {
                    self.called_functions.insert(name.clone());
                    self.mark_used(name);
                } else {
                    self.check_expr(callee);
                }
                for arg in args {
                    self.check_expr(arg);
                }
            }
            Expr::Index(obj, idx, _) => {
                self.check_expr(obj);
                self.check_expr(idx);
            }
            Expr::Select(obj, _, _) => self.check_expr(obj),
            Expr::Array(elems, _) => {
                for elem in elems {
                    self.check_expr(elem);
                }
            }
            Expr::Tuple(elems, _) => {
                for elem in elems {
                    self.check_expr(elem);
                }
            }
            Expr::ArrayRepeat(value, count, _) => {
                self.check_expr(value);
                self.check_expr(count);
            }
            Expr::StructInit(_, fields, _, _) => {
                for (_, value) in fields {
                    self.check_expr(value);
                }
            }
            Expr::Lambda(params, body, _) => self.with_scope(|ctx| {
                for (name, _) in params {
                    ctx.define_param(name);
                }
                ctx.check_expr(body);
            }),
            Expr::If(cond, then_expr, else_expr, _) => {
                self.check_expr(cond);
                self.check_expr(then_expr);
                if let Some(else_expr) = else_expr {
                    self.check_expr(else_expr);
                }
            }
            Expr::Block(exprs, _) => self.with_scope(|ctx| {
                for expr in exprs {
                    ctx.check_expr(expr);
                }
            }),
            Expr::Match(scrutinee, arms, _) => {
                self.check_expr(scrutinee);
                if !arms.iter().any(|(pattern, _)| is_wildcard_pattern(pattern)) {
                    self.warn("match expression has no default branch".into());
                }
                for (pattern, body) in arms {
                    self.check_pattern(pattern);
                    self.check_expr(body);
                }
            }
            Expr::Number(_, _)
            | Expr::IntLiteral(_, _, _)
            | Expr::Float(_, _)
            | Expr::FloatLiteral(_, _, _)
            | Expr::String(_, _)
            | Expr::Char(_, _)
            | Expr::Bool(_, _)
            | Expr::Unit(_) => {}
        }
    }

    fn check_pattern(&mut self, pattern: &Expr) {
        match pattern {
            Expr::Call(_, args, _, _) => {
                for arg in args {
                    if let Expr::Ident(name, _) = arg {
                        self.define_var(name);
                    } else {
                        self.check_pattern(arg);
                    }
                }
            }
            Expr::Ident(name, _) if name != "_" && !name.contains("::") => self.define_var(name),
            _ => {}
        }
    }

    fn define_var(&mut self, name: &str) {
        if !name.starts_with('_') {
            self.warn_if_shadowing(name);
            self.current().vars.insert(name.to_string(), false);
        }
    }

    fn define_import(&mut self, name: &str) {
        if !name.starts_with('_') {
            self.warn_if_shadowing(name);
            self.current().imports.insert(name.to_string(), false);
        }
    }

    fn define_param(&mut self, name: &str) {
        if !name.starts_with('_') {
            self.warn_if_shadowing(name);
            self.current().params.insert(name.to_string(), false);
        }
    }

    fn warn_if_shadowing(&mut self, name: &str) {
        if self.scopes.len() <= 1 {
            return;
        }
        if self.scopes[..self.scopes.len() - 1]
            .iter()
            .rev()
            .any(|scope| {
                scope.vars.contains_key(name)
                    || scope.params.contains_key(name)
                    || scope.imports.contains_key(name)
            })
        {
            self.warn(format!("variable shadows outer binding: {}", name));
        }
    }

    fn check_discarded_result(&mut self, expr: &Expr) {
        if let Expr::Call(callee, _, _, span) = expr {
            if let Expr::Ident(name, _) = callee.as_ref() {
                if HEAP_RETURNING_BUILTINS.contains(&name.as_str()) {
                    self.warn_at(
                        format!("result of heap-returning builtin '{}' is discarded", name),
                        Some(*span),
                    );
                }
            }
        }
    }

    fn mark_used(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(used) = scope.vars.get_mut(name) {
                *used = true;
                return;
            }
            if let Some(used) = scope.params.get_mut(name) {
                *used = true;
                return;
            }
            if let Some(used) = scope.imports.get_mut(name) {
                *used = true;
                return;
            }
        }
    }

    fn with_scope(&mut self, f: impl FnOnce(&mut Self)) {
        self.scopes.push(ScopeUsage::default());
        f(self);
        self.finish_scope();
    }

    fn finish_scope(&mut self) {
        let Some(scope) = self.scopes.pop() else {
            return;
        };
        for (name, used) in scope.vars {
            if !used {
                self.warn(format!("unused variable: {}", name));
            }
        }
        for (name, used) in scope.imports {
            if !used {
                self.errors.push(warning_error(
                    ErrorCode::LintUnusedImport,
                    format!("unused import: {}", name),
                    None,
                ));
            }
        }
        for (name, used) in scope.params {
            if !used {
                self.warn(format!("unused parameter: {}", name));
            }
        }
    }

    fn finish_functions(&mut self) {
        for name in self.functions.clone() {
            if name != "main" && !self.called_functions.contains(&name) {
                self.warn(format!("unused function: {}", name));
            }
        }
    }

    fn current(&mut self) -> &mut ScopeUsage {
        self.scopes.last_mut().expect("warning lint scope exists")
    }

    fn warn(&mut self, message: String) {
        self.warn_at(message, None);
    }

    fn warn_at(&mut self, message: String, span: Option<Span>) {
        self.errors
            .push(warning_error(ErrorCode::LintUnusedVariable, message, span));
    }
}

fn is_wildcard_pattern(pattern: &Expr) -> bool {
    matches!(pattern, Expr::Ident(name, _) if name == "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Span;

    #[test]
    fn test_empty_program() {
        let errors = WarningLinter::lint(&[]);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_unused_variable() {
        let errors = WarningLinter::lint(&[Stmt::Let("x".into(), None, None)]);
        assert!(errors.iter().any(|e| e.message == "unused variable: x"));
    }

    #[test]
    fn test_used_variable() {
        let errors = WarningLinter::lint(&[
            Stmt::Let("x".into(), None, None),
            Stmt::Expr(Expr::Ident("x".into(), Span::dummy())),
        ]);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_unused_import() {
        let errors = WarningLinter::lint(&[Stmt::Import("math".into(), None, Span::dummy())]);
        assert!(errors.iter().any(|e| e.message == "unused import: math"));
    }

    #[test]
    fn test_unused_parameter() {
        let errors = WarningLinter::lint(&[Stmt::Function(
            "main".into(),
            vec![("x".into(), "i64".into())],
            None,
            vec![],
            vec![],
            Span::dummy(),
        )]);
        assert!(errors.iter().any(|e| e.message == "unused parameter: x"));
    }

    #[test]
    fn test_unreachable_code() {
        let errors = WarningLinter::lint(&[
            Stmt::Return(None, Span::dummy()),
            Stmt::Expr(Expr::Number(1, Span::dummy())),
        ]);
        assert!(errors.iter().any(|e| e.message == "unreachable code"));
    }

    #[test]
    fn test_unused_function() {
        let errors = WarningLinter::lint(&[Stmt::Function(
            "helper".into(),
            vec![],
            None,
            vec![],
            vec![],
            Span::dummy(),
        )]);
        assert!(errors
            .iter()
            .any(|e| e.message == "unused function: helper"));
    }

    #[test]
    fn test_shadowed_variable_warning() {
        let errors = WarningLinter::lint(&[
            Stmt::Let("x".into(), None, None),
            Stmt::If(
                vec![(
                    Expr::Bool(true, Span::dummy()),
                    vec![Stmt::Let("x".into(), None, None)],
                )],
                None,
                Span::dummy(),
            ),
        ]);
        assert!(errors
            .iter()
            .any(|e| e.message == "variable shadows outer binding: x"));
    }

    #[test]
    fn test_match_without_default_warning() {
        let errors = WarningLinter::lint(&[Stmt::Expr(Expr::Match(
            Box::new(Expr::Number(1, Span::dummy())),
            vec![(
                Expr::Number(1, Span::dummy()),
                Expr::Number(2, Span::dummy()),
            )],
            Span::dummy(),
        ))]);
        assert!(errors
            .iter()
            .any(|e| e.message == "match expression has no default branch"));
    }

    #[test]
    fn test_discarded_heap_builtin_warning() {
        let errors = WarningLinter::lint(&[Stmt::Expr(Expr::Call(
            Box::new(Expr::Ident("vec_new".into(), Span::dummy())),
            vec![],
            vec![],
            Span::dummy(),
        ))]);
        assert!(errors
            .iter()
            .any(|e| { e.message == "result of heap-returning builtin 'vec_new' is discarded" }));
    }
}
