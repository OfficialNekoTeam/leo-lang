use crate::common::span::Span;

#[derive(Debug, Clone)]
pub enum Expr {
    Ident(String, Span),
    Number(i64, Span),
    String(String, Span),
    Binary(BinOp, Box<Expr>, Box<Expr>, Span),
    Unary(UnOp, Box<Expr>, Span),
    Call(Box<Expr>, Vec<Expr>, Span),
    Index(Box<Expr>, Box<Expr>, Span),
    Select(Box<Expr>, String, Span),
    Lambda(Vec<(String, String)>, Box<Expr>, Span),
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
}

#[derive(Debug, Clone)]
pub enum UnOp {
    Neg,
    Not,
    Ref,
    Deref,
}