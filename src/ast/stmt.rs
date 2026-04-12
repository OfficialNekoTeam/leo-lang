use crate::ast::expr::Expr;
use crate::common::span::Span;

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    Let(String, Option<String>, Option<Expr>),
    Assign(String, Expr),
    If(Expr, Vec<Stmt>, Option<Vec<Stmt>>, Span),
    While(Expr, Vec<Stmt>, Span),
    For(String, Expr, Vec<Stmt>, Span),
    Function(String, Vec<(String, String)>, Option<String>, Vec<Stmt>, Span),
    Return(Option<Expr>, Span),
    Import(String, Option<Vec<String>>, Span),
    Module(String, Vec<Stmt>, Span),
    Struct(String, Vec<(String, String)>, Span),
    Enum(String, Vec<(String, Vec<Expr>)>, Span),
}