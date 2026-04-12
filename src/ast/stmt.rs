use crate::ast::expr::Expr;
use crate::common::span::Span;

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    Let(String, Option<String>, Option<Expr>),
    Assign(String, Expr),
    MutAssign(String, Expr),
    If(Vec<(Expr, Vec<Stmt>)>, Option<Vec<Stmt>>, Span),
    While(Expr, Vec<Stmt>, Span),
    For(String, Expr, Vec<Stmt>, Span),
    Function(String, Vec<(String, String)>, Option<String>, Vec<Stmt>, Span),
    AsyncFunction(String, Vec<(String, String)>, Option<String>, Vec<Stmt>, Span),
    Return(Option<Expr>, Span),
    Break(Option<Expr>, Span),
    Continue(Span),
    Import(String, Option<Vec<String>>, Span),
    FromImport(String, Vec<String>, Span),
    Module(String, Vec<Stmt>, Span),
    Struct(String, Vec<(String, String)>, Span),
    Enum(String, Vec<(String, Vec<Expr>)>, Span),
    Trait(String, Vec<(String, Vec<Stmt>)>, Span),
    Impl(String, Option<String>, Vec<Stmt>, Span),
    Pub(Box<Stmt>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stmt_let() {
        let stmt = Stmt::Let("x".to_string(), Some("i32".to_string()), None);
        assert!(matches!(stmt, Stmt::Let(_, _, _)));
    }

    #[test]
    fn test_stmt_assign() {
        let expr = Expr::Number(1, Span::dummy());
        let stmt = Stmt::Assign("x".to_string(), expr);
        assert!(matches!(stmt, Stmt::Assign(_, _)));
    }

    #[test]
    fn test_stmt_if() {
        let cond = Expr::Bool(true, Span::dummy());
        let branches = vec![(cond, vec![])];
        let stmt = Stmt::If(branches, None, Span::dummy());
        assert!(matches!(stmt, Stmt::If(_, _, _)));
    }

    #[test]
    fn test_stmt_while() {
        let cond = Expr::Bool(true, Span::dummy());
        let stmt = Stmt::While(cond, vec![], Span::dummy());
        assert!(matches!(stmt, Stmt::While(_, _, _)));
    }

    #[test]
    fn test_stmt_for() {
        let expr = Expr::Ident("items".to_string(), Span::dummy());
        let stmt = Stmt::For("i".to_string(), expr, vec![], Span::dummy());
        assert!(matches!(stmt, Stmt::For(_, _, _, _)));
    }

    #[test]
    fn test_stmt_function() {
        let params = vec![("x".to_string(), "i32".to_string())];
        let stmt = Stmt::Function("foo".to_string(), params, Some("i32".to_string()), vec![], Span::dummy());
        assert!(matches!(stmt, Stmt::Function(_, _, _, _, _)));
    }

    #[test]
    fn test_stmt_return() {
        let expr = Some(Expr::Number(1, Span::dummy()));
        let stmt = Stmt::Return(expr, Span::dummy());
        assert!(matches!(stmt, Stmt::Return(_, _)));
    }

    #[test]
    fn test_stmt_import() {
        let stmt = Stmt::Import("foo".to_string(), None, Span::dummy());
        assert!(matches!(stmt, Stmt::Import(_, _, _)));
    }

    #[test]
    fn test_stmt_module() {
        let stmt = Stmt::Module("foo".to_string(), vec![], Span::dummy());
        assert!(matches!(stmt, Stmt::Module(_, _, _)));
    }

    #[test]
    fn test_stmt_struct() {
        let fields = vec![("x".to_string(), "i32".to_string())];
        let stmt = Stmt::Struct("Foo".to_string(), fields, Span::dummy());
        assert!(matches!(stmt, Stmt::Struct(_, _, _)));
    }

    #[test]
    fn test_stmt_enum() {
        let variants = vec![("A".to_string(), vec![])];
        let stmt = Stmt::Enum("E".to_string(), variants, Span::dummy());
        assert!(matches!(stmt, Stmt::Enum(_, _, _)));
    }

    #[test]
    fn test_stmt_pub() {
        let inner = Box::new(Stmt::Expr(Expr::Number(1, Span::dummy())));
        let stmt = Stmt::Pub(inner);
        assert!(matches!(stmt, Stmt::Pub(_)));
    }
}
