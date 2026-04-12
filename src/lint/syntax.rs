use crate::common::{LeoError, LeoResult};

pub struct SyntaxLinter;

impl SyntaxLinter {
    pub fn new() -> Self {
        Self
    }

    pub fn lint(&self, _source: &str) -> LeoResult<Vec<LeoError>> {
        Ok(vec![])
    }
}