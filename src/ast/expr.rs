use crate::common::span::Span;
use crate::common::types::LeoType;

#[derive(Debug, Clone)]
pub enum Expr {
    Ident(String, Span),
    Number(i64, Span),
    IntLiteral(u128, LeoType, Span),
    Float(f64, Span),
    FloatLiteral(f64, LeoType, Span),
    String(String, Span),
    Char(char, Span),
    Bool(bool, Span),
    Unit(Span),
    Tuple(Vec<Expr>, Span),
    Binary(BinOp, Box<Expr>, Box<Expr>, Span),
    Unary(UnOp, Box<Expr>, Span),
    /// Call(callee, args, type_args, span)
    Call(Box<Expr>, Vec<Expr>, Vec<String>, Span),
    Index(Box<Expr>, Box<Expr>, Span),
    Select(Box<Expr>, String, Span),
    Array(Vec<Expr>, Span),
    ArrayRepeat(Box<Expr>, Box<Expr>, Span),
    /// StructInit(name, fields, type_args, span)
    StructInit(String, Vec<(String, Expr)>, Vec<String>, Span),
    Lambda(Vec<(String, String)>, Box<Expr>, Span),
    If(Box<Expr>, Box<Expr>, Option<Box<Expr>>, Span),
    Block(Vec<Expr>, Span),
    Await(Box<Expr>, Span),
    Match(Box<Expr>, Vec<(Expr, Expr)>, Span),
}

#[derive(Debug, Clone)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    Shl,
    Shr,
}

#[derive(Debug, Clone)]
pub enum UnOp {
    Neg,
    Not,
    Ref,
    Deref,
    Minus,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::span::Pos;

    #[test]
    fn test_expr_ident() {
        let span = Span::new(Pos::new(1, 1, 0), Pos::new(1, 4, 3));
        let expr = Expr::Ident("x".to_string(), span);
        assert!(matches!(expr, Expr::Ident(_, _)));
    }

    #[test]
    fn test_expr_number() {
        let span = Span::dummy();
        let expr = Expr::Number(42, span);
        assert!(matches!(expr, Expr::Number(n, _) if n == 42));
    }

    #[test]
    fn test_expr_float() {
        let span = Span::dummy();
        let expr = Expr::Float(3.14, span);
        assert!(matches!(expr, Expr::Float(f, _) if (f - 3.14).abs() < 0.001));
    }

    #[test]
    fn test_expr_bool() {
        let span = Span::dummy();
        let expr = Expr::Bool(true, span);
        assert!(matches!(expr, Expr::Bool(b, _) if b == true));
    }

    #[test]
    fn test_expr_unit() {
        let span = Span::dummy();
        let expr = Expr::Unit(span);
        assert!(matches!(expr, Expr::Unit(_)));
    }

    #[test]
    fn test_expr_tuple() {
        let span = Span::dummy();
        let expr = Expr::Tuple(vec![Expr::Number(1, span), Expr::Bool(true, span)], span);
        assert!(matches!(expr, Expr::Tuple(elems, _) if elems.len() == 2));
    }

    #[test]
    fn test_expr_binary() {
        let span = Span::dummy();
        let left = Box::new(Expr::Number(1, span));
        let right = Box::new(Expr::Number(2, span));
        let expr = Expr::Binary(BinOp::Add, left, right, span);
        assert!(matches!(expr, Expr::Binary(BinOp::Add, _, _, _)));
    }

    #[test]
    fn test_expr_unary() {
        let span = Span::dummy();
        let expr_inner = Box::new(Expr::Number(1, span));
        let expr = Expr::Unary(UnOp::Neg, expr_inner, span);
        assert!(matches!(expr, Expr::Unary(UnOp::Neg, _, _)));
    }

    #[test]
    fn test_expr_if() {
        let span = Span::dummy();
        let cond = Box::new(Expr::Bool(true, span));
        let then = Box::new(Expr::Number(1, span));
        let else_: Option<Box<Expr>> = None;
        let expr = Expr::If(cond, then, else_, span);
        assert!(matches!(expr, Expr::If(_, _, _, _)));
    }

    #[test]
    fn test_expr_block() {
        let span = Span::dummy();
        let exprs = vec![Expr::Number(1, span), Expr::Number(2, span)];
        let expr = Expr::Block(exprs, span);
        assert!(matches!(expr, Expr::Block(_, _)));
    }

    #[test]
    fn test_expr_await() {
        let span = Span::dummy();
        let expr_inner = Box::new(Expr::Ident("task".to_string(), span));
        let expr = Expr::Await(expr_inner, span);
        assert!(matches!(expr, Expr::Await(_, _)));
    }

    #[test]
    fn test_expr_lambda() {
        let span = Span::dummy();
        let params = vec![("x".to_string(), "i32".to_string())];
        let body = Box::new(Expr::Number(1, span));
        let expr = Expr::Lambda(params, body, span);
        assert!(matches!(expr, Expr::Lambda(_, _, _)));
    }
}
