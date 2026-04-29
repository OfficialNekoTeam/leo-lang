use crate::ast::expr::{BinOp, Expr};
use crate::ast::stmt::Stmt;
use crate::common::{ErrorCode, LeoError};
use crate::lint::common::{safety_error, walk_stmts, AstVisitor};

pub struct SafetyLinter;

impl SafetyLinter {
    pub fn lint(stmts: &[Stmt]) -> Vec<LeoError> {
        let mut visitor = SafetyVisitor { errors: Vec::new() };
        walk_stmts(&mut visitor, stmts);
        visitor.errors
    }
}

struct SafetyVisitor {
    errors: Vec<LeoError>,
}

impl AstVisitor for SafetyVisitor {
    fn visit_expr(&mut self, expr: &Expr) {
        if let Expr::Binary(op, left, right, span) = expr {
            if !matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) {
                return;
            }
            let (Expr::Number(l, _), Expr::Number(r, _)) = (left.as_ref(), right.as_ref()) else {
                return;
            };
            let overflow = match op {
                BinOp::Add => l.checked_add(*r).is_none(),
                BinOp::Sub => l.checked_sub(*r).is_none(),
                BinOp::Mul => l.checked_mul(*r).is_none(),
                _ => false,
            };
            if overflow {
                self.errors.push(safety_error(
                    ErrorCode::LintOverflowRisk,
                    format!("constant overflow in {:?} expression", op),
                    Some(*span),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Span;

    #[test]
    fn test_empty_program() {
        let errors = SafetyLinter::lint(&[]);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_nested_overflow_is_detected() {
        let expr = Expr::Binary(
            BinOp::Add,
            Box::new(Expr::Number(i64::MAX, Span::dummy())),
            Box::new(Expr::Number(1, Span::dummy())),
            Span::dummy(),
        );
        let errors = SafetyLinter::lint(&[Stmt::Pub(Box::new(Stmt::Module(
            "m".into(),
            vec![Stmt::Expr(expr)],
            Span::dummy(),
        )))]);
        assert!(errors.iter().any(|e| e.code == ErrorCode::LintOverflowRisk));
    }
}
