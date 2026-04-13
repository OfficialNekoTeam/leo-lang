use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::builder::Builder;
use inkwell::values::{FunctionValue, PointerValue};
use std::collections::HashMap;

/// Wrapper around LLVM module, builder, and variable/function tables
pub struct LlvmContext<'ctx> {
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    functions: HashMap<String, FunctionValue<'ctx>>,
    variables: HashMap<String, PointerValue<'ctx>>,
}

impl<'ctx> LlvmContext<'ctx> {
    /// Create new LLVM context with empty module
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        Self { module, builder, functions: HashMap::new(), variables: HashMap::new() }
    }

    /// Get reference to module
    pub fn module(&self) -> &Module<'ctx> { &self.module }

    /// Get mutable reference to module (needed for add_global etc.)
    pub fn module_mut(&mut self) -> &mut Module<'ctx> { &mut self.module }

    /// Get reference to builder
    pub fn builder(&self) -> &Builder<'ctx> { &self.builder }

    /// Register a function value by name
    pub fn register_function(&mut self, name: String, fv: FunctionValue<'ctx>) {
        self.functions.insert(name, fv);
    }

    /// Look up function by name
    pub fn get_function(&self, name: &str) -> Option<FunctionValue<'ctx>> {
        self.functions.get(name).copied()
    }

    /// Store a variable's stack pointer by name
    pub fn register_variable(&mut self, name: String, ptr: PointerValue<'ctx>) {
        self.variables.insert(name, ptr);
    }

    /// Look up variable stack pointer by name
    pub fn get_variable(&self, name: &str) -> Option<PointerValue<'ctx>> {
        self.variables.get(name).copied()
    }

    /// Clear all local variables (called between function bodies)
    pub fn clear_variables(&mut self) {
        self.variables.clear();
    }

    /// Write bitcode to file
    pub fn write_bitcode(&self, path: &str) -> Result<(), String> {
        self.module.write_bitcode_to_path(path.as_ref())
            .then_some(())
            .ok_or_else(|| format!("failed to write bitcode to {}", path))
    }

    /// Print module IR to string
    pub fn print_module(&self) -> String {
        self.module.print_to_string().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::context::Context;

    #[test]
    fn test_llvm_context_new() {
        let context = Context::create();
        let ctx = LlvmContext::new(&context, "test");
        let ir = ctx.print_module();
        assert!(ir.contains("test"));
    }
}
