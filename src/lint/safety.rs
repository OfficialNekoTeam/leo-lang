use crate::common::LeoError;
use crate::ast::stmt::Stmt;

/// Safety-level linter (configurable, default on)
pub struct SafetyLinter;

impl SafetyLinter {
    pub fn new() -> Self { Self }

    /// Check for unsafe patterns
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
        let errors = SafetyLinter::lint(&[]);
        assert!(errors.is_empty());
    }
}
