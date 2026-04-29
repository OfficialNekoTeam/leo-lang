use crate::ast::stmt::Stmt;
use crate::common::{ErrorCode, LeoError, Pos, Span};
use crate::lint::common::style_error;

const MAX_FUNCTION_LINES: u32 = 50;
const MAX_LINE_LENGTH: usize = 120;
const MAX_NESTING_DEPTH: usize = 5;

/// Style-level linter (dismissible).
pub struct StyleLinter;

impl StyleLinter {
    /// Check naming conventions and simple structural style limits.
    pub fn lint(stmts: &[Stmt]) -> Vec<LeoError> {
        Self::lint_with_source(stmts, None)
    }

    /// Check style using both AST structure and optional source text.
    pub fn lint_with_source(stmts: &[Stmt], source: Option<&str>) -> Vec<LeoError> {
        let mut ctx = StyleLintCtx { errors: Vec::new() };
        if let Some(source) = source {
            ctx.check_source_text(source);
        }
        ctx.check_stmts(stmts, 0);
        ctx.errors
    }
}

struct StyleLintCtx {
    errors: Vec<LeoError>,
}

impl StyleLintCtx {
    fn check_source_text(&mut self, source: &str) {
        let mut offset = 0_u32;
        let mut blank_run = 0_u32;
        for (index, line) in source.lines().enumerate() {
            let line_no = (index + 1) as u32;
            let line_len = line.chars().count();
            if line_len > MAX_LINE_LENGTH {
                self.style_at(
                    format!("line exceeds {} characters", MAX_LINE_LENGTH),
                    Some(line_span(line_no, line_len as u32, offset)),
                );
            }

            if line.trim().is_empty() {
                blank_run += 1;
                if blank_run == 2 {
                    self.style_at(
                        "repeated blank lines".into(),
                        Some(line_span(line_no, 1, offset)),
                    );
                }
            } else {
                blank_run = 0;
            }

            offset = offset.saturating_add(line.len() as u32).saturating_add(1);
        }
    }

    fn check_stmts(&mut self, stmts: &[Stmt], depth: usize) {
        if depth > MAX_NESTING_DEPTH {
            self.style(format!("nesting depth exceeds {}", MAX_NESTING_DEPTH));
        }
        self.check_import_grouping(stmts);
        for stmt in stmts {
            self.check_stmt(stmt, depth);
        }
    }

    fn check_import_grouping(&mut self, stmts: &[Stmt]) {
        let mut seen_non_import = false;
        let mut warned = false;
        for stmt in stmts {
            match stmt {
                Stmt::Import(_, _, _) | Stmt::FromImport(_, _, _) => {
                    if seen_non_import && !warned {
                        self.style("imports should be grouped before other statements".into());
                        warned = true;
                    }
                }
                Stmt::Pub(inner)
                    if matches!(
                        inner.as_ref(),
                        Stmt::Import(_, _, _) | Stmt::FromImport(_, _, _)
                    ) =>
                {
                    if seen_non_import && !warned {
                        self.style("imports should be grouped before other statements".into());
                        warned = true;
                    }
                }
                _ => seen_non_import = true,
            }
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt, depth: usize) {
        match stmt {
            Stmt::Let(name, _, _) | Stmt::Assign(name, _) | Stmt::MutAssign(name, _) => {
                self.check_snake(name, "variable");
            }
            Stmt::If(branches, els, _) => {
                for (_, body) in branches {
                    self.check_stmts(body, depth + 1);
                }
                if let Some(body) = els {
                    self.check_stmts(body, depth + 1);
                }
            }
            Stmt::While(_, body, _) | Stmt::For(_, _, body, _) => {
                self.check_stmts(body, depth + 1);
            }
            Stmt::Function(name, params, _, body, _, span)
            | Stmt::AsyncFunction(name, params, _, body, _, span) => {
                self.check_snake(name, "function");
                for (param, _) in params {
                    self.check_snake(param, "parameter");
                }
                if span.end.line.saturating_sub(span.start.line) > MAX_FUNCTION_LINES {
                    self.style_at(
                        format!("function '{}' exceeds {} lines", name, MAX_FUNCTION_LINES),
                        Some(*span),
                    );
                }
                self.check_stmts(body, depth + 1);
            }
            Stmt::Module(name, body, _) => {
                self.check_snake(name, "module");
                self.check_stmts(body, depth + 1);
            }
            Stmt::Struct(name, fields, _, _) => {
                self.check_pascal(name, "struct");
                for (field, _) in fields {
                    self.check_snake(field, "field");
                }
            }
            Stmt::Enum(name, variants, _) => {
                self.check_pascal(name, "enum");
                for (variant, _) in variants {
                    self.check_pascal(variant, "enum variant");
                }
            }
            Stmt::Trait(name, methods, _) => {
                self.check_pascal(name, "trait");
                for (_, body) in methods {
                    self.check_stmts(body, depth + 1);
                }
            }
            Stmt::Impl(name, _, methods, _, _) => {
                self.check_pascal(name, "impl target");
                for method in methods {
                    self.check_stmt(method, depth + 1);
                }
            }
            Stmt::Const(name, _, _, _) => self.check_screaming_snake(name, "const"),
            Stmt::Pub(inner) => self.check_stmt(inner, depth),
            Stmt::Expr(_)
            | Stmt::FieldAssign(_, _, _)
            | Stmt::Return(_, _)
            | Stmt::Break(_, _)
            | Stmt::Continue(_)
            | Stmt::Import(_, _, _)
            | Stmt::FromImport(_, _, _) => {}
        }
    }

    fn check_snake(&mut self, name: &str, kind: &str) {
        if !is_snake_case(name) {
            self.style(format!("{} '{}' should use snake_case", kind, name));
        }
    }

    fn check_pascal(&mut self, name: &str, kind: &str) {
        if !is_pascal_case(name) {
            self.style(format!("{} '{}' should use PascalCase", kind, name));
        }
    }

    fn check_screaming_snake(&mut self, name: &str, kind: &str) {
        if !is_screaming_snake_case(name) {
            self.style(format!(
                "{} '{}' should use SCREAMING_SNAKE_CASE",
                kind, name
            ));
        }
    }

    fn style(&mut self, message: String) {
        self.style_at(message, None);
    }

    fn style_at(&mut self, message: String, span: Option<Span>) {
        self.errors
            .push(style_error(ErrorCode::LintNamingConvention, message, span));
    }
}

fn is_snake_case(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('_')
        && !name.ends_with('_')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn is_pascal_case(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_alphanumeric())
}

fn is_screaming_snake_case(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('_')
        && !name.ends_with('_')
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn line_span(line: u32, len: u32, offset: u32) -> Span {
    Span::new(
        Pos::new(line, 1, offset),
        Pos::new(line, len.saturating_add(1), offset.saturating_add(len)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{Pos, Span};

    #[test]
    fn test_empty_program() {
        let errors = StyleLinter::lint(&[]);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_function_name_style() {
        let errors = StyleLinter::lint(&[Stmt::Function(
            "BadName".into(),
            vec![],
            None,
            vec![],
            vec![],
            Span::dummy(),
        )]);
        assert!(errors.iter().any(|e| e.message.contains("snake_case")));
    }

    #[test]
    fn test_struct_name_style() {
        let errors = StyleLinter::lint(&[Stmt::Struct(
            "bad_struct".into(),
            vec![],
            vec![],
            Span::dummy(),
        )]);
        assert!(errors.iter().any(|e| e.message.contains("PascalCase")));
    }

    #[test]
    fn test_const_name_style() {
        let errors = StyleLinter::lint(&[Stmt::Const(
            "bad_const".into(),
            "i64".into(),
            crate::ast::expr::Expr::Number(1, Span::dummy()),
            Span::dummy(),
        )]);
        assert!(errors
            .iter()
            .any(|e| e.message.contains("SCREAMING_SNAKE_CASE")));
    }

    #[test]
    fn test_function_length_style() {
        let span = Span::new(Pos::new(1, 1, 0), Pos::new(100, 1, 100));
        let errors = StyleLinter::lint(&[Stmt::Function(
            "long_fn".into(),
            vec![],
            None,
            vec![],
            vec![],
            span,
        )]);
        assert!(errors.iter().any(|e| e.message.contains("exceeds")));
    }

    #[test]
    fn test_import_grouping_style() {
        let errors = StyleLinter::lint(&[
            Stmt::Let("x".into(), None, None),
            Stmt::Import("math".into(), None, Span::dummy()),
        ]);
        assert!(errors
            .iter()
            .any(|e| e.message == "imports should be grouped before other statements"));
    }

    #[test]
    fn test_line_length_style() {
        let source = "a".repeat(121);
        let errors = StyleLinter::lint_with_source(&[], Some(&source));
        assert!(errors
            .iter()
            .any(|e| e.message == "line exceeds 120 characters"));
    }

    #[test]
    fn test_repeated_blank_lines_style() {
        let errors = StyleLinter::lint_with_source(&[], Some("let x = 1\n\n\nlet y = 2"));
        assert!(errors.iter().any(|e| e.message == "repeated blank lines"));
    }
}
