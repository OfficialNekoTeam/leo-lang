use crate::common::{ErrorCode, ErrorKind, LeoError, LeoResult};
use crate::ast::stmt::Stmt;
use crate::codegen::ir::IrBuilder;
use inkwell::context::Context;
use inkwell::targets::{InitializationConfig, Target, TargetMachine, CodeModel, RelocMode, FileType};
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

    /// Generate native code from AST statements
    pub fn generate(&self, stmts: &[Stmt]) -> LeoResult<String> {
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "leo_module");
        let mut builder = IrBuilder::new();
        builder.build(stmts, &mut ctx)?;
        let ir = ctx.print_module();

        if !self.output_path.is_empty() {
            self.emit_object(&ctx)?;
        }
        Ok(ir)
    }

    /// Emit native object file
    fn emit_object(&self, ctx: &LlvmContext) -> LeoResult<()> {
        Target::initialize_x86(&InitializationConfig::default());
        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple)
            .map_err(|e| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError,
                format!("target error: {:?}", e)))?;
        let machine = target.create_target_machine(
            &triple,
            "generic",
            "",
            inkwell::OptimizationLevel::Aggressive,
            RelocMode::Default,
            CodeModel::Default,
        ).ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError,
            "failed to create target machine".into()))?;

        let obj_path = self.output_path.clone() + ".o";
        machine.write_to_file(ctx.module(), FileType::Object, obj_path.as_ref())
            .map_err(|e| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError,
                format!("write object failed: {:?}", e)))?;

        std::process::Command::new("clang")
            .args([&obj_path, "-o", &self.output_path])
            .status()
            .map_err(|e| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError,
                format!("link failed: {}", e)))?;

        std::fs::remove_file(&obj_path).ok();
        Ok(())
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
