use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::values::{FunctionValue, PointerValue};
use std::collections::HashMap;

pub struct EnumDef {
    pub variants: Vec<String>,
}

/// Compile-time type tag for LLVM value tracking
#[derive(Debug, Clone, PartialEq)]
pub enum LeoType {
    Int,
    Float,
    Bool,
    Str,
    Char,
    Ptr,
}

pub struct LlvmContext<'ctx> {
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    functions: HashMap<String, FunctionValue<'ctx>>,
    variables: HashMap<String, PointerValue<'ctx>>,
    current_fn: Option<FunctionValue<'ctx>>,
    enums: HashMap<String, EnumDef>,
    types: HashMap<String, LeoType>,
}

impl<'ctx> LlvmContext<'ctx> {
    /// Create new LLVM context with empty module
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        Self {
            module,
            builder,
            functions: HashMap::new(),
            variables: HashMap::new(),
            current_fn: None,
            enums: HashMap::new(),
            types: HashMap::new(),
        }
    }

    /// Get reference to module
    pub fn module(&self) -> &Module<'ctx> {
        &self.module
    }

    /// Get mutable reference to module (needed for add_global etc.)
    pub fn module_mut(&mut self) -> &mut Module<'ctx> {
        &mut self.module
    }

    /// Get reference to builder
    pub fn builder(&self) -> &Builder<'ctx> {
        &self.builder
    }

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
        self.current_fn = None;
        self.types.clear();
    }

    /// Set the currently-being-compiled function (for return type queries)
    pub fn set_current_fn(&mut self, fv: FunctionValue<'ctx>) {
        self.current_fn = Some(fv);
    }

    /// Get the currently-being-compiled function
    pub fn current_fn(&self) -> Option<FunctionValue<'ctx>> {
        self.current_fn
    }

    pub fn register_enum(&mut self, name: String, variants: Vec<String>) {
        self.enums.insert(name, EnumDef { variants });
    }

    pub fn get_enum(&self, name: &str) -> Option<&EnumDef> {
        self.enums.get(name)
    }

    pub fn get_enum_variant_tag(&self, enum_name: &str, variant_name: &str) -> Option<u32> {
        self.enums.get(enum_name).and_then(|edef| {
            edef.variants
                .iter()
                .position(|v| v == variant_name)
                .map(|i| i as u32)
        })
    }

    pub fn register_type(&mut self, name: String, ty: LeoType) {
        self.types.insert(name, ty);
    }

    pub fn get_type(&self, name: &str) -> Option<&LeoType> {
        self.types.get(name)
    }

    pub fn clear_types(&mut self) {
        self.types.clear();
    }

    /// Write bitcode to file
    pub fn write_bitcode(&self, path: &str) -> Result<(), String> {
        self.module
            .write_bitcode_to_path(path.as_ref())
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
