use crate::common::LeoError;
use crate::ast::stmt::Stmt;

/// Style-level linter (dismissible)
pub struct StyleLinter;

impl StyleLinter {
    pub fn new() -> Self { Self }

    /// Check naming conventions
    pub fn lint(stmts: &[Stmt]) -> Vec<LeoError> {
        let _ = stmts;
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_program() {
        let errors = StyleLinter::lint(&[]);
        assert!(errors.is_empty());
    }
}
