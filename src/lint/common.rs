use crate::ast::expr::Expr;
use crate::ast::stmt::Stmt;
use crate::common::{ErrorCode, ErrorKind, LeoError, Span};

/// Create a lint error with a stable kind/code/message shape.
pub(crate) fn lint_error(
    kind: ErrorKind,
    code: ErrorCode,
    message: impl Into<String>,
    span: Option<Span>,
) -> LeoError {
    let err = LeoError::new(kind, code, message.into());
    if let Some(span) = span {
        err.with_span(span)
    } else {
        err
    }
}

/// Create a semantic lint diagnostic.
pub(crate) fn semantic_error(
    code: ErrorCode,
    message: impl Into<String>,
    span: Option<Span>,
) -> LeoError {
    lint_error(ErrorKind::Semantic, code, message, span)
}

/// Create a warning lint diagnostic.
pub(crate) fn warning_error(
    code: ErrorCode,
    message: impl Into<String>,
    span: Option<Span>,
) -> LeoError {
    lint_error(ErrorKind::Warning, code, message, span)
}

/// Create a style lint diagnostic.
pub(crate) fn style_error(
    code: ErrorCode,
    message: impl Into<String>,
    span: Option<Span>,
) -> LeoError {
    lint_error(ErrorKind::Style, code, message, span)
}

/// Create a safety lint diagnostic.
pub(crate) fn safety_error(
    code: ErrorCode,
    message: impl Into<String>,
    span: Option<Span>,
) -> LeoError {
    lint_error(ErrorKind::Safety, code, message, span)
}

/// Visitor hooks for shared AST traversal.
pub(crate) trait AstVisitor {
    fn visit_stmt(&mut self, _stmt: &Stmt) {}

    fn visit_expr(&mut self, _expr: &Expr) {}
}

/// Walk a list of statements and every nested statement/expression below them.
pub(crate) fn walk_stmts<V: AstVisitor + ?Sized>(visitor: &mut V, stmts: &[Stmt]) {
    for stmt in stmts {
        walk_stmt(visitor, stmt);
    }
}

/// Walk one statement and every nested node below it.
pub(crate) fn walk_stmt<V: AstVisitor + ?Sized>(visitor: &mut V, stmt: &Stmt) {
    visitor.visit_stmt(stmt);
    match stmt {
        Stmt::Expr(expr) => walk_expr(visitor, expr),
        Stmt::Let(_, _, Some(expr)) | Stmt::Assign(_, expr) | Stmt::MutAssign(_, expr) => {
            walk_expr(visitor, expr)
        }
        Stmt::Let(_, _, None) => {}
        Stmt::FieldAssign(obj, _, expr) => {
            walk_expr(visitor, obj);
            walk_expr(visitor, expr);
        }
        Stmt::If(branches, els, _) => {
            for (cond, body) in branches {
                walk_expr(visitor, cond);
                walk_stmts(visitor, body);
            }
            if let Some(body) = els {
                walk_stmts(visitor, body);
            }
        }
        Stmt::While(cond, body, _) | Stmt::For(_, cond, body, _) => {
            walk_expr(visitor, cond);
            walk_stmts(visitor, body);
        }
        Stmt::Function(_, _, _, body, _, _) | Stmt::AsyncFunction(_, _, _, body, _, _) => {
            walk_stmts(visitor, body)
        }
        Stmt::Return(Some(expr), _) | Stmt::Break(Some(expr), _) => walk_expr(visitor, expr),
        Stmt::Return(None, _) | Stmt::Break(None, _) | Stmt::Continue(_) => {}
        Stmt::Import(_, _, _) | Stmt::FromImport(_, _, _) => {}
        Stmt::Module(_, body, _) => walk_stmts(visitor, body),
        Stmt::Struct(_, _, _, _) => {}
        Stmt::Enum(_, variants, _) => {
            for (_, payload) in variants {
                for expr in payload {
                    walk_expr(visitor, expr);
                }
            }
        }
        Stmt::Trait(_, methods, _) => {
            for (_, body) in methods {
                walk_stmts(visitor, body);
            }
        }
        Stmt::Impl(_, _, methods, _, _) => walk_stmts(visitor, methods),
        Stmt::Pub(inner) => walk_stmt(visitor, inner),
        Stmt::Const(_, _, expr, _) => walk_expr(visitor, expr),
    }
}

/// Walk one expression and every nested expression below it.
pub(crate) fn walk_expr<V: AstVisitor + ?Sized>(visitor: &mut V, expr: &Expr) {
    visitor.visit_expr(expr);
    match expr {
        Expr::Binary(_, left, right, _) => {
            walk_expr(visitor, left);
            walk_expr(visitor, right);
        }
        Expr::Unary(_, inner, _) | Expr::Await(inner, _) => walk_expr(visitor, inner),
        Expr::Call(callee, args, _, _) => {
            walk_expr(visitor, callee);
            for arg in args {
                walk_expr(visitor, arg);
            }
        }
        Expr::Index(obj, idx, _) => {
            walk_expr(visitor, obj);
            walk_expr(visitor, idx);
        }
        Expr::Select(obj, _, _) => walk_expr(visitor, obj),
        Expr::Array(elems, _) => {
            for elem in elems {
                walk_expr(visitor, elem);
            }
        }
        Expr::Tuple(elems, _) => {
            for elem in elems {
                walk_expr(visitor, elem);
            }
        }
        Expr::ArrayRepeat(value, count, _) => {
            walk_expr(visitor, value);
            walk_expr(visitor, count);
        }
        Expr::StructInit(_, fields, _, _) => {
            for (_, value) in fields {
                walk_expr(visitor, value);
            }
        }
        Expr::Lambda(_, body, _) => walk_expr(visitor, body),
        Expr::If(cond, then_expr, else_expr, _) => {
            walk_expr(visitor, cond);
            walk_expr(visitor, then_expr);
            if let Some(else_expr) = else_expr {
                walk_expr(visitor, else_expr);
            }
        }
        Expr::Block(exprs, _) => {
            for expr in exprs {
                walk_expr(visitor, expr);
            }
        }
        Expr::Match(scrutinee, arms, _) => {
            walk_expr(visitor, scrutinee);
            for (pattern, body) in arms {
                walk_expr(visitor, pattern);
                walk_expr(visitor, body);
            }
        }
        Expr::Ident(_, _)
        | Expr::Number(_, _)
        | Expr::IntLiteral(_, _, _)
        | Expr::Float(_, _)
        | Expr::FloatLiteral(_, _, _)
        | Expr::String(_, _)
        | Expr::Char(_, _)
        | Expr::Bool(_, _)
        | Expr::Unit(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::expr::Expr;
    use crate::common::Span;

    #[derive(Default)]
    struct CountingVisitor {
        stmts: usize,
        exprs: usize,
    }

    impl AstVisitor for CountingVisitor {
        fn visit_stmt(&mut self, _stmt: &Stmt) {
            self.stmts += 1;
        }

        fn visit_expr(&mut self, _expr: &Expr) {
            self.exprs += 1;
        }
    }

    #[test]
    fn test_walk_empty_program() {
        let mut visitor = CountingVisitor::default();
        walk_stmts(&mut visitor, &[]);
        assert_eq!(visitor.stmts, 0);
        assert_eq!(visitor.exprs, 0);
    }

    #[test]
    fn test_walk_wrappers_and_nested_nodes() {
        let stmts = vec![
            Stmt::Pub(Box::new(Stmt::Module(
                "m".into(),
                vec![Stmt::Function(
                    "f".into(),
                    vec![],
                    None,
                    vec![Stmt::Expr(Expr::Block(
                        vec![Expr::Number(1, Span::dummy())],
                        Span::dummy(),
                    ))],
                    vec![],
                    Span::dummy(),
                )],
                Span::dummy(),
            ))),
            Stmt::Impl(
                "Point".into(),
                None,
                vec![Stmt::Function(
                    "new".into(),
                    vec![],
                    None,
                    vec![Stmt::Return(
                        Some(Expr::Number(1, Span::dummy())),
                        Span::dummy(),
                    )],
                    vec![],
                    Span::dummy(),
                )],
                vec![],
                Span::dummy(),
            ),
        ];
        let mut visitor = CountingVisitor::default();
        walk_stmts(&mut visitor, &stmts);
        assert!(visitor.stmts >= 7);
        assert!(visitor.exprs >= 3);
    }
}
