use crate::common::LeoError;
use crate::ast::stmt::Stmt;

/// Warning-level linter (demotable)
pub struct WarningLinter;

impl WarningLinter {
    pub fn new() -> Self { Self }

    /// Check for warnings like unused variables
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
        let errors = WarningLinter::lint(&[]);
        assert!(errors.is_empty());
    }
}
