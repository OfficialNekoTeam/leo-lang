use crate::common::span::Span;
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrorKind {
    Syntax,
    Semantic,
    Warning,
    Style,
    Safety,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ErrorCode {
    LexerUnexpectedChar,
    LexerUnterminatedString,
    LexerInvalidNumber,
    ParserUnexpectedToken,
    ParserMissingToken,
    ParserInvalidSyntax,
    SemaUndefinedVariable,
    SemaUndefinedFunction,
    SemaTypeMismatch,
    SemaDuplicateDefinition,
    SemaInvalidImport,
    CodegenLLVMError,
    IoError,
    LintUnusedVariable,
    LintUnusedImport,
    LintNamingConvention,
    LintDataRace,
    LintUninitMemory,
    LintOverflowRisk,
}

pub struct LeoError {
    pub kind: ErrorKind,
    pub code: ErrorCode,
    pub message: String,
    pub span: Option<Span>,
    pub hint: Option<String>,
}

impl LeoError {
    pub fn new(kind: ErrorKind, code: ErrorCode, message: String) -> Self {
        Self {
            kind,
            code,
            message,
            span: None,
            hint: None,
        }
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_hint(mut self, hint: String) -> Self {
        self.hint = Some(hint);
        self
    }
}

impl fmt::Display for LeoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(span) = self.span {
            write!(f, "{}: {}", span, self.message)?;
        } else {
            write!(f, "{}", self.message)?;
        }
        if let Some(hint) = &self.hint {
            write!(f, "\n  help: {}", hint)?;
        }
        Ok(())
    }
}

impl fmt::Debug for LeoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "LeoError({:?}, {:?}): {}",
            self.kind, self.code, self.message
        )
    }
}

impl std::error::Error for LeoError {}

pub type LeoResult<T> = Result<T, LeoError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_new() {
        let err = LeoError::new(
            ErrorKind::Syntax,
            ErrorCode::LexerUnexpectedChar,
            "unexpected character".to_string(),
        );
        assert_eq!(err.kind, ErrorKind::Syntax);
        assert_eq!(err.code, ErrorCode::LexerUnexpectedChar);
        assert_eq!(err.span, None);
    }

    #[test]
    fn test_error_with_span() {
        let span = Span::new(
            crate::common::span::Pos::new(1, 1, 0),
            crate::common::span::Pos::new(1, 5, 4),
        );
        let err = LeoError::new(
            ErrorKind::Syntax,
            ErrorCode::LexerUnexpectedChar,
            "unexpected character".to_string(),
        )
        .with_span(span);
        assert_eq!(err.span, Some(span));
    }

    #[test]
    fn test_error_with_hint() {
        let err = LeoError::new(
            ErrorKind::Syntax,
            ErrorCode::LexerUnexpectedChar,
            "unexpected character".to_string(),
        )
        .with_hint("did you mean 'x'?".to_string());
        assert_eq!(err.hint, Some("did you mean 'x'?".to_string()));
    }

    #[test]
    fn test_error_display() {
        let err = LeoError::new(
            ErrorKind::Syntax,
            ErrorCode::LexerUnexpectedChar,
            "unexpected character".to_string(),
        );
        let msg = err.to_string();
        assert!(msg.contains("unexpected character"));
    }
}
