use crate::common::{ErrorCode, ErrorKind, LeoError, LeoResult};
use crate::ast::stmt::Stmt;
use crate::codegen::ir::IrBuilder;
use inkwell::context::Context;
use crate::llvm::context::LlvmContext;

/// Top-level code generator
pub struct Generator {
    output_path: String,
}

impl Generator {
    /// Create generator with output file path
    pub fn new(output_path: &str) -> Self {
        Self { output_path: output_path.to_string() }
    }

    /// Generate code from AST statements
    pub fn generate(&self, stmts: &[Stmt]) -> LeoResult<String> {
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "leo_module");
        let builder = IrBuilder::new();
        builder.build(stmts, &mut ctx)?;
        let ir = ctx.print_module();
        if !self.output_path.is_empty() {
            ctx.write_bitcode(&self.output_path)
                .map_err(|e| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, e))?;
        }
        Ok(ir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generator_empty() {
        let gen = Generator::new("");
        let result = gen.generate(&[]);
        assert!(result.is_ok());
    }
}
