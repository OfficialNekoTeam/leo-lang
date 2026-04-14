use crate::ast::expr::{BinOp, Expr};
use crate::ast::stmt::Stmt;
use crate::common::{ErrorCode, ErrorKind, LeoError};

pub struct SafetyLinter;

impl SafetyLinter {
    pub fn new() -> Self {
        Self
    }

    pub fn lint(stmts: &[Stmt]) -> Vec<LeoError> {
        let mut errors = Vec::new();
        for stmt in stmts {
            Self::lint_stmt(stmt, &mut errors);
        }
        errors
    }

    fn lint_stmt(stmt: &Stmt, errors: &mut Vec<LeoError>) {
        match stmt {
            Stmt::Let(_, _, Some(init)) | Stmt::Assign(_, init) | Stmt::FieldAssign(_, _, init) => {
                Self::lint_expr(init, errors);
            }
            Stmt::Expr(e) => Self::lint_expr(e, errors),
            Stmt::Return(Some(e), _) => Self::lint_expr(e, errors),
            Stmt::If(branches, els, _) => {
                for (cond, body) in branches {
                    Self::lint_expr(cond, errors);
                    for s in body {
                        Self::lint_stmt(s, errors);
                    }
                }
                if let Some(els) = els {
                    for s in els {
                        Self::lint_stmt(s, errors);
                    }
                }
            }
            Stmt::While(cond, body, _) | Stmt::For(_, cond, body, _) => {
                Self::lint_expr(cond, errors);
                for s in body {
                    Self::lint_stmt(s, errors);
                }
            }
            Stmt::Function(_, _, _, body, _) | Stmt::AsyncFunction(_, _, _, body, _) => {
                for s in body {
                    Self::lint_stmt(s, errors);
                }
            }
            Stmt::Impl(_, _, methods, _) => {
                for m in methods {
                    Self::lint_stmt(m, errors);
                }
            }
            Stmt::Module(_, body, _) => {
                for s in body {
                    Self::lint_stmt(s, errors);
                }
            }
            Stmt::Pub(inner) => Self::lint_stmt(inner, errors),
            _ => {}
        }
    }

    fn lint_expr(expr: &Expr, errors: &mut Vec<LeoError>) {
        match expr {
            Expr::Binary(op, left, right, _) => {
                if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) {
                    if let (Expr::Number(l, _), Expr::Number(r, _)) =
                        (left.as_ref(), right.as_ref())
                    {
                        let overflow = match op {
                            BinOp::Add => l.checked_add(*r).is_none(),
                            BinOp::Sub => l.checked_sub(*r).is_none(),
                            BinOp::Mul => l.checked_mul(*r).is_none(),
                            _ => false,
                        };
                        if overflow {
                            errors.push(LeoError::new(
                                ErrorKind::Safety,
                                ErrorCode::LintOverflowRisk,
                                format!("constant overflow in {:?} expression", op),
                            ));
                        }
                    }
                }
                Self::lint_expr(left, errors);
                Self::lint_expr(right, errors);
            }
            Expr::Call(callee, args, _) => {
                Self::lint_expr(callee, errors);
                for a in args {
                    Self::lint_expr(a, errors);
                }
            }
            Expr::Index(obj, idx, _) => {
                Self::lint_expr(obj, errors);
                Self::lint_expr(idx, errors);
            }
            Expr::Select(obj, _, _) => {
                Self::lint_expr(obj, errors);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_program() {
        let errors = SafetyLinter::lint(&[]);
        assert!(errors.is_empty());
    }
}
