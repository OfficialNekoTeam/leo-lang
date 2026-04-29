use crate::codegen::Generator;
use crate::common::LeoResult;
use crate::lexer::Lexer;
use crate::lint::{SafetyLinter, SemanticLinter, StyleLinter, SyntaxLinter, WarningLinter};
use crate::parser::Parser;
use crate::sema::Checker;

/// Compilation pipeline orchestrating all stages
pub struct Pipeline {
    source: String,
    output: String,
}

impl Pipeline {
    /// Create pipeline with source code and output path
    pub fn new(source: &str, output: &str) -> Self {
        Self {
            source: source.to_string(),
            output: output.to_string(),
        }
    }

    /// Run full compilation pipeline
    pub fn compile(&self) -> LeoResult<String> {
        let tokens = self.stage_lexer()?;
        let stmts = self.stage_parser(&tokens)?;
        self.stage_sema(&stmts)?;
        self.stage_lint(&tokens, &stmts);
        let ir = self.stage_codegen(&stmts)?;
        Ok(ir)
    }

    /// Stage 1: Lexical analysis
    fn stage_lexer(&self) -> LeoResult<Vec<crate::lexer::token::TokenWithSpan>> {
        let mut lexer = Lexer::new(&self.source);
        lexer.tokenize()
    }

    /// Stage 2: Parsing
    fn stage_parser(
        &self,
        tokens: &[crate::lexer::token::TokenWithSpan],
    ) -> LeoResult<Vec<crate::ast::stmt::Stmt>> {
        let mut parser = Parser::new(tokens.to_vec());
        parser.parse()
    }

    /// Stage 3: Semantic analysis
    fn stage_sema(&self, stmts: &[crate::ast::stmt::Stmt]) -> LeoResult<()> {
        let mut checker = Checker::new();
        checker.check(stmts)
    }

    /// Stage 4: Lint checks (non-fatal)
    fn stage_lint(
        &self,
        tokens: &[crate::lexer::token::TokenWithSpan],
        stmts: &[crate::ast::stmt::Stmt],
    ) {
        let all: Vec<_> = SyntaxLinter::lint(tokens)
            .unwrap_or_default()
            .into_iter()
            .chain(SemanticLinter::lint(stmts))
            .chain(WarningLinter::lint(stmts))
            .chain(StyleLinter::lint_with_source(stmts, Some(&self.source)))
            .chain(SafetyLinter::lint(stmts))
            .collect();
        for err in &all {
            eprintln!("lint: {}", err);
        }
    }

    /// Stage 5: Code generation
    fn stage_codegen(&self, stmts: &[crate::ast::stmt::Stmt]) -> LeoResult<String> {
        let gen = Generator::new(&self.output);
        gen.generate(stmts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_empty() {
        let pipeline = Pipeline::new("", "");
        assert!(pipeline.compile().is_ok());
    }

    #[test]
    fn test_pipeline_number() {
        let pipeline = Pipeline::new("42", "");
        assert!(pipeline.compile().is_ok());
    }

    #[test]
    fn test_pipeline_let() {
        let pipeline = Pipeline::new("let x: i32 = 42", "");
        assert!(pipeline.compile().is_ok());
    }
}
