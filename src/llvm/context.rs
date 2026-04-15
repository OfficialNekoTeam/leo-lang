use crate::common::types::LeoType;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::values::{FunctionValue, PointerValue};
use std::collections::HashMap;

pub struct LoopTarget<'ctx> {
    pub continue_block: BasicBlock<'ctx>,
    pub merge_block: BasicBlock<'ctx>,
}

pub struct EnumDef {
    pub variants: Vec<String>,
}

pub struct LlvmContext<'ctx> {
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    functions: HashMap<String, FunctionValue<'ctx>>,
    variables: HashMap<String, PointerValue<'ctx>>,
    current_fn: Option<FunctionValue<'ctx>>,
    enums: HashMap<String, EnumDef>,
    types: HashMap<String, LeoType>,
    fn_return_types: HashMap<String, LeoType>,
    fn_param_types: HashMap<String, Vec<LeoType>>,
    pub loop_stack: Vec<LoopTarget<'ctx>>,
}

impl<'ctx> LlvmContext<'ctx> {
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
            fn_return_types: HashMap::new(),
            fn_param_types: HashMap::new(),
            loop_stack: Vec::new(),
        }
    }

    pub fn module(&self) -> &Module<'ctx> {
        &self.module
    }

    pub fn module_mut(&mut self) -> &mut Module<'ctx> {
        &mut self.module
    }

    pub fn builder(&self) -> &Builder<'ctx> {
        &self.builder
    }

    pub fn register_function(&mut self, name: String, fv: FunctionValue<'ctx>) {
        self.functions.insert(name, fv);
    }

    pub fn get_function(&self, name: &str) -> Option<FunctionValue<'ctx>> {
        self.functions.get(name).copied()
    }

    pub fn register_variable(&mut self, name: String, ptr: PointerValue<'ctx>) {
        self.variables.insert(name, ptr);
    }

    pub fn get_variable(&self, name: &str) -> Option<PointerValue<'ctx>> {
        self.variables.get(name).copied()
    }

    pub fn clear_variables(&mut self) {
        self.variables.clear();
        self.current_fn = None;
        self.types.clear();
    }

    pub fn set_current_fn(&mut self, fv: FunctionValue<'ctx>) {
        self.current_fn = Some(fv);
    }

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

    pub fn register_fn_return_type(&mut self, name: String, ty: LeoType) {
        self.fn_return_types.insert(name, ty);
    }

    pub fn get_fn_return_type(&self, name: &str) -> Option<&LeoType> {
        self.fn_return_types.get(name)
    }

    pub fn register_fn_param_types(&mut self, name: String, types: Vec<LeoType>) {
        self.fn_param_types.insert(name, types);
    }

    pub fn get_fn_param_types(&self, name: &str) -> Option<&Vec<LeoType>> {
        self.fn_param_types.get(name)
    }

    pub fn is_string_var(&self, name: &str) -> bool {
        self.types.get(name).map(|t| t.is_string()).unwrap_or(false)
    }

    pub fn write_bitcode(&self, path: &str) -> Result<(), String> {
        self.module
            .write_bitcode_to_path(path.as_ref())
            .then_some(())
            .ok_or_else(|| format!("failed to write bitcode to {}", path))
    }

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
