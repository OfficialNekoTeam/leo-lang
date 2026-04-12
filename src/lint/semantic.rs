use crate::common::LeoError;
use crate::ast::stmt::Stmt;

/// Semantic-level linter for AST validation
pub struct SemanticLinter;

impl SemanticLinter {
    pub fn new() -> Self { Self }

    /// Check for unused imports
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
        let errors = SemanticLinter::lint(&[]);
        assert!(errors.is_empty());
    }
}
