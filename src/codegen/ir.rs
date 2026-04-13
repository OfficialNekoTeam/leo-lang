use crate::common::{ErrorCode, ErrorKind, LeoError, LeoResult};
use crate::ast::expr::{BinOp, Expr, UnOp};
use crate::ast::stmt::Stmt;
use crate::llvm::context::LlvmContext;
use inkwell::types::BasicTypeEnum;
use inkwell::values::BasicValueEnum;
use inkwell::IntPredicate;
use inkwell::AddressSpace;
use inkwell::attributes::AttributeLoc;
use std::collections::{HashMap, HashSet};

/// IR builder that walks AST and emits LLVM IR
pub struct IrBuilder {
    array_sizes: HashMap<String, u32>,
    string_vars: HashSet<String>,
}

impl IrBuilder {
    pub fn new() -> Self { Self { array_sizes: HashMap::new(), string_vars: HashSet::new() } }

    /// Build LLVM IR from statements
    pub fn build(&mut self, stmts: &[Stmt], ctx: &mut LlvmContext) -> LeoResult<()> {
        self.array_sizes.clear();
        self.string_vars.clear();
        self.declare_c_runtime(ctx);
        for stmt in stmts {
            self.build_stmt(stmt, ctx)?;
        }
        Ok(())
    }

    /// Declare external C runtime functions (puts, printf)
    fn declare_c_runtime(&mut self, ctx: &mut LlvmContext) {
        let i8_ptr = ctx.module().get_context().i8_type().ptr_type(AddressSpace::default());
        let i64_type = ctx.module().get_context().i64_type();
        let i32_type = ctx.module().get_context().i32_type();
        ctx.module_mut().add_function("puts", i8_ptr.fn_type(&[], false), None);
        ctx.module_mut().add_function("printf", i32_type.fn_type(&[i8_ptr.into(), i64_type.into()], true), None);
        ctx.module_mut().add_function("strlen", i64_type.fn_type(&[i8_ptr.into()], false), None);
        ctx.module_mut().add_function("malloc", i8_ptr.fn_type(&[i64_type.into()], false), None);
        ctx.module_mut().add_function("memcpy", i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_type.into()], false), None);
        ctx.module_mut().add_function("strcpy", i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false), None);
        ctx.module_mut().add_function("strcat", i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false), None);
    }

    /// Build top-level statement (function definitions, struct declarations)
    fn build_stmt(&mut self, stmt: &Stmt, ctx: &mut LlvmContext) -> LeoResult<()> {
        match stmt {
            Stmt::Function(name, params, ret, body, _) |
            Stmt::AsyncFunction(name, params, ret, body, _) => {
                self.build_fn(name, params, ret, body, ctx)?;
            }
            Stmt::Const(name, ty, expr, _span) => {
                self.build_const(name, ty, expr, ctx)?;
            }
            Stmt::Struct(name, fields, _) => {
                let context = ctx.module().get_context();
                let struct_type = context.opaque_struct_type(name);
                let field_types: Vec<BasicTypeEnum> = fields.iter()
                    .map(|(_, ty)| Self::llvm_type(ty, ctx))
                    .collect();
                struct_type.set_body(&field_types, false);
            }
            Stmt::Enum(name, variants, _) => {
                let context = ctx.module().get_context();
                let i32_type = context.i32_type();
                let max_payload: u32 = variants.iter().map(|(_, payload)| {
                    if payload.is_empty() { 0 } else { payload.len() as u32 * 8 }
                }).max().unwrap_or(0);
                let payload_type = if max_payload > 0 {
                    context.i8_type().array_type(max_payload).into()
                } else {
                    context.i8_type().array_type(1).into()
                };
                let enum_struct = context.opaque_struct_type(name);
                enum_struct.set_body(&[i32_type.into(), payload_type], false);
                let variant_names: Vec<String> = variants.iter().map(|(n, _)| n.clone()).collect();
                ctx.register_enum(name.clone(), variant_names);
            }
            _ => {}
        }
        Ok(())
    }

    /// Build LLVM function with params, return type, and body
    fn build_fn(&mut self, name: &str, params: &[(String, String)], ret: &Option<String>, body: &[Stmt], ctx: &mut LlvmContext) -> LeoResult<()> {
        ctx.clear_variables();
        let context = ctx.module().get_context();
        let is_main = name == "main";

        let param_types: Vec<BasicTypeEnum> = params.iter()
            .map(|(_, ty)| Self::llvm_type(ty, ctx))
            .collect();
        let param_meta: Vec<_> = param_types.iter().map(|t| (*t).into()).collect();

        let fn_type = if is_main {
            context.i32_type().fn_type(&param_meta, false)
        } else {
            match ret.as_deref() {
                Some("i32") => context.i32_type().fn_type(&param_meta, false),
                Some("bool") => context.bool_type().fn_type(&param_meta, false),
                _ => context.i64_type().fn_type(&param_meta, false),
            }
        };

        let function = ctx.module_mut().add_function(name, fn_type, None);

        // Auto-inline small functions (≤3 body statements, not main)
        if !is_main && body.len() <= 3 {
            let always_inline = context.create_enum_attribute(inkwell::attributes::Attribute::get_named_enum_kind_id("alwaysinline"), 0);
            function.add_attribute(AttributeLoc::Function, always_inline);
        }

        let entry = context.append_basic_block(function, "entry");
        ctx.builder().position_at_end(entry);
        ctx.register_function(name.to_string(), function);
        ctx.set_current_fn(function);

        // Alloca + store each parameter as a local variable
        for (i, (pname, _pty)) in params.iter().enumerate() {
            let ptr = ctx.builder().build_alloca(param_types[i], pname)
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("alloca param {} failed", pname)))?;
            let param_val = function.get_nth_param(i as u32)
                .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("param {} not found", pname)))?;
            ctx.builder().build_store(ptr, param_val)
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("store param {} failed", pname)))?;
            ctx.register_variable(pname.clone(), ptr);
        }

        for stmt in body {
            self.build_body_stmt(stmt, ctx)?;
        }

        // Default return (fallback if no explicit return)
        if !Self::block_is_terminated(ctx) {
            if is_main {
                let zero = context.i32_type().const_int(0, false);
                ctx.builder().build_return(Some(&zero))
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "return failed".into()))?;
            } else {
                let zero = context.i64_type().const_int(0, false);
                ctx.builder().build_return(Some(&zero))
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "return failed".into()))?;
            }
        }
        Ok(())
    }

    /// Build statement inside function body (let, assign, while, if, expr, return)
    fn build_body_stmt(&mut self, stmt: &Stmt, ctx: &mut LlvmContext) -> LeoResult<()> {
        match stmt {
            Stmt::Let(name, ty, init) => {
                self.build_let(name, ty, init, ctx)?;
            }
            Stmt::Assign(name, expr) => {
                self.build_assign(name, expr, ctx)?;
            }
            Stmt::While(cond, body, _span) => {
                self.build_while(cond, body, ctx)?;
            }
            Stmt::If(branches, else_body, _span) => {
                self.build_if(branches, else_body, ctx)?;
            }
            Stmt::Return(Some(expr), _) => {
                let val = self.eval_expr_to_value(expr, ctx)?;
                self.build_return_with(val, ctx)?;
            }
            Stmt::Return(None, _) => {
                let context = ctx.module().get_context();
                let zero = context.i32_type().const_int(0, false);
                ctx.builder().build_return(Some(&zero))
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "return failed".into()))?;
            }
            Stmt::Expr(expr) => {
                self.eval_and_emit(expr, ctx)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Build const: treated as a global constant (evaluated at compile time)
    fn build_const(&mut self, name: &str, ty: &str, expr: &Expr, ctx: &mut LlvmContext) -> LeoResult<()> {
        let context = ctx.module().get_context();
        let llvm_type = Self::llvm_type(ty, ctx);
        match expr {
            Expr::Number(n, _) => {
                let gv = ctx.module_mut().add_global(llvm_type, Some(AddressSpace::default()), name);
                gv.set_initializer(&context.i64_type().const_int(*n as u64, false));
                gv.set_constant(true);
                gv.set_linkage(inkwell::module::Linkage::Private);
            }
            Expr::Bool(b, _) => {
                let gv = ctx.module_mut().add_global(llvm_type, Some(AddressSpace::default()), name);
                gv.set_initializer(&context.i64_type().const_int(*b as u64, false));
                gv.set_constant(true);
                gv.set_linkage(inkwell::module::Linkage::Private);
            }
            _ => {
                // Fallback: evaluate and store
                let val = self.eval_int(expr, ctx)?;
                let gv = ctx.module_mut().add_global(llvm_type, Some(AddressSpace::default()), name);
                gv.set_initializer(&val);
                gv.set_constant(true);
                gv.set_linkage(inkwell::module::Linkage::Private);
            }
        }
        Ok(())
    }
    fn build_let(&mut self, name: &str, ty: &Option<String>, init: &Option<Expr>, ctx: &mut LlvmContext) -> LeoResult<()> {
        let type_str = ty.as_deref().unwrap_or("i64");
        let llvm_type = Self::llvm_type(type_str, ctx);
        let ptr = ctx.builder().build_alloca(llvm_type, name)
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("alloca failed for {}", name)))?;
        ctx.register_variable(name.to_string(), ptr);

        if let Some(expr) = init {
            match expr {
                Expr::Array(elems, _) => {
                    self.array_sizes.insert(name.to_string(), elems.len() as u32);
                }
                Expr::ArrayRepeat(_, count, _) => {
                    if let Expr::Number(n, _) = count.as_ref() {
                        self.array_sizes.insert(name.to_string(), *n as u32);
                    }
                }
                Expr::String(s, _) => {
                    self.string_vars.insert(name.to_string());
                    self.array_sizes.insert(name.to_string(), s.len() as u32);
                }
                _ => {}
            }
            let val = self.eval_expr_to_value(expr, ctx)?;
            ctx.builder().build_store(ptr, val)
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("store failed for {}", name)))?;
        }
        Ok(())
    }

    /// Build assignment: load value, store into existing variable
    fn build_assign(&mut self, name: &str, expr: &Expr, ctx: &mut LlvmContext) -> LeoResult<()> {
        let ptr = ctx.get_variable(name)
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("undefined variable: {}", name)))?;
        let val = self.eval_expr_to_value(expr, ctx)?;
        ctx.builder().build_store(ptr, val)
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("store failed for {}", name)))?;
        Ok(())
    }

    /// Build while loop: condition block → body block → merge block
    fn build_while(&mut self, cond: &Expr, body: &[Stmt], ctx: &mut LlvmContext) -> LeoResult<()> {
        let function = ctx.builder().get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "no function".into()))?;
        let context = ctx.module().get_context();

        let cond_block = context.append_basic_block(function, "while.cond");
        let body_block = context.append_basic_block(function, "while.body");
        let merge_block = context.append_basic_block(function, "while.merge");

        ctx.builder().build_unconditional_branch(cond_block)
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "branch failed".into()))?;

        ctx.builder().position_at_end(cond_block);
        let cond_val = self.eval_int(cond, ctx)?;
        let zero = context.i64_type().const_int(0, false);
        let cmp = ctx.builder().build_int_compare(IntPredicate::NE, cond_val, zero, "while.test")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "compare failed".into()))?;
        ctx.builder().build_conditional_branch(cmp, body_block, merge_block)
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "cond branch failed".into()))?;

        ctx.builder().position_at_end(body_block);
        for stmt in body {
            self.build_body_stmt(stmt, ctx)?;
        }
        self.emit_branch(cond_block, ctx)?;

        ctx.builder().position_at_end(merge_block);
        Ok(())
    }

    /// Build if/else-if/else chain using LLVM conditional branches
    fn build_if(&mut self, branches: &[(Expr, Vec<Stmt>)], else_body: &Option<Vec<Stmt>>, ctx: &mut LlvmContext) -> LeoResult<()> {
        let function = ctx.builder().get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "no function".into()))?;
        let context = ctx.module().get_context();

        let merge_block = context.append_basic_block(function, "if.merge");

        // Pre-create all condition and then blocks to avoid duplicates
        let cond_blocks: Vec<_> = (0..branches.len())
            .map(|i| context.append_basic_block(function, &format!("if.cond.{}", i)))
            .collect();
        let then_blocks: Vec<_> = (0..branches.len())
            .map(|i| context.append_basic_block(function, &format!("if.then.{}", i)))
            .collect();

        let else_block = if else_body.is_some() {
            Some(context.append_basic_block(function, "if.else"))
        } else {
            None
        };

        // Branch from current position to first condition
        ctx.builder().build_unconditional_branch(cond_blocks[0])
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "branch failed".into()))?;

        for (i, (cond, body)) in branches.iter().enumerate() {
            // Evaluate condition in cond_block
            ctx.builder().position_at_end(cond_blocks[i]);
            let cond_val = self.eval_int(cond, ctx)?;
            let zero = context.i64_type().const_int(0, false);
            let cmp = ctx.builder().build_int_compare(IntPredicate::NE, cond_val, zero, "if.test")
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "compare failed".into()))?;

            let false_block = if i + 1 < branches.len() {
                cond_blocks[i + 1]
            } else {
                else_block.unwrap_or(merge_block)
            };
            ctx.builder().build_conditional_branch(cmp, then_blocks[i], false_block)
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "cond branch failed".into()))?;

            // Build then body
            ctx.builder().position_at_end(then_blocks[i]);
            for stmt in body {
                self.build_body_stmt(stmt, ctx)?;
            }
            self.emit_branch(merge_block, ctx)?;
        }

        // Build else body
        if let (Some(else_stmts), Some(eb)) = (else_body, else_block) {
            ctx.builder().position_at_end(eb);
            for stmt in else_stmts {
                self.build_body_stmt(stmt, ctx)?;
            }
            self.emit_branch(merge_block, ctx)?;
        }

        ctx.builder().position_at_end(merge_block);
        Ok(())
    }

    /// Check if the current basic block already has a terminator
    fn block_is_terminated(ctx: &LlvmContext) -> bool {
        ctx.builder().get_insert_block()
            .map_or(true, |bb| bb.get_terminator().is_some())
    }

    /// Emit unconditional branch only if block has no terminator yet
    fn emit_branch(&mut self, target: inkwell::basic_block::BasicBlock, ctx: &mut LlvmContext) -> LeoResult<()> {
        if Self::block_is_terminated(ctx) { return Ok(()); }
        ctx.builder().build_unconditional_branch(target)
            .map(|_| ())
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "branch failed".into()))
    }
     fn eval_and_emit(&mut self, expr: &Expr, ctx: &mut LlvmContext) -> LeoResult<()> {
         match expr {
             Expr::String(s, _) => self.emit_puts(s, ctx),
             Expr::Call(_, _, _) => { let _ = self.eval_int(expr, ctx)?; Ok(()) }
             Expr::Ident(name, _) => {
                 let val = self.load_ident(name, ctx)?;
                 self.emit_print_int(val, ctx);
                 Ok(())
             }
             _ => {
                 let val = self.eval_int(expr, ctx)?;
                 self.emit_print_int(val, ctx);
                 Ok(())
             }
         }
     }

    /// Load identifier value: try local variable, then global constant
    fn load_ident<'a>(&mut self, name: &str, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        if let Some(ptr) = ctx.get_variable(name) {
            ctx.builder().build_load(ptr, name)
                .map(|v| v.into_int_value())
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("load failed for {}", name)))
        } else if let Some(gv) = ctx.module().get_global(name) {
            let val = ctx.builder().build_load(gv.as_pointer_value(), name)
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("load const {} failed", name)))?;
            Ok(val.into_int_value())
        } else {
            Err(LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("undefined variable: {}", name)))
        }
    }

    /// Evaluate expression to an LLVM IntValue (handles ident load, literals, binary, unary)
    fn eval_expr_to_value<'a>(&mut self, expr: &Expr, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        match expr {
            Expr::Ident(name, _) => self.load_ident(name, ctx),
            _ => self.eval_int(expr, ctx),
        }
    }

    /// Evaluate integer expression (number, bool, binary, unary, call)
    fn eval_int<'a>(&mut self, expr: &Expr, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        match expr {
            Expr::Number(n, _) => Ok(ctx.module().get_context().i64_type().const_int(*n as u64, false)),
            Expr::Bool(b, _) => Ok(ctx.module().get_context().i64_type().const_int(*b as u64, false)),
            Expr::Char(c, _) => Ok(ctx.module().get_context().i64_type().const_int(*c as u64, false)),
            Expr::Ident(name, _) => self.load_ident(name, ctx),
            Expr::Binary(op, left, right, _) => {
                if let (Expr::Number(l, _), Expr::Number(r, _)) = (left.as_ref(), right.as_ref()) {
                    if let Some(folded) = Self::fold_constants(op, *l, *r) {
                        return Ok(ctx.module().get_context().i64_type().const_int(folded as u64, false));
                    }
                }
                let lv = self.eval_int(left, ctx)?;
                let rv = self.eval_int(right, ctx)?;
                self.emit_binop(op, lv, rv, ctx)
            }
            Expr::Unary(op, e, _) => {
                let val = self.eval_int(e, ctx)?;
                self.emit_unop(op, val, ctx)
            }
            Expr::Call(callee, args, _) => self.eval_call(callee, args, ctx),
            Expr::Index(obj, idx, _) => self.eval_index(obj, idx, ctx),
            Expr::Select(obj, field, _) => self.eval_select(obj, field, ctx),
            Expr::Array(_, _) | Expr::ArrayRepeat(_, _, _) => self.eval_array_alloc(expr, ctx),
            Expr::StructInit(_, _, _) => self.eval_struct_init(expr, ctx),
            Expr::Match(scrutinee, arms, _) => self.eval_match(scrutinee, arms, ctx),
            _ => Ok(ctx.module().get_context().i64_type().const_int(0, false)),
        }
    }

    /// Evaluate function call: check builtins first, then user functions
    fn eval_call<'a>(&mut self, callee: &Expr, args: &[Expr], ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        match callee {
            Expr::Ident(name, _) => {
                let func_name = name.clone();
                if func_name.contains("::") {
                    return self.eval_enum_constructor(&func_name, args, ctx);
                }
                match func_name.as_str() {
                    "println" => return self.builtin_println(args, ctx),
                    "print" => return self.builtin_print(args, ctx),
                    "panic" => return self.builtin_panic(args, ctx),
                    "assert" => return self.builtin_assert(args, ctx),
                    "str_len" => return self.builtin_str_len(args, ctx),
                    "str_char_at" => return self.builtin_str_char_at(args, ctx),
                    "str_slice" => return self.builtin_str_slice(args, ctx),
                    "str_concat" => return self.builtin_str_concat(args, ctx),
                    _ => {}
                }
                let func = ctx.get_function(&func_name)
                    .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("undefined function: {}", func_name)))?;
                let mut arg_values: Vec<_> = Vec::new();
                for arg in args {
                    let val = self.eval_int(arg, ctx)?;
                    arg_values.push(BasicValueEnum::from(val).into());
                }
                let call_site = ctx.builder().build_call(func, &arg_values, "call")
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("call {} failed", func_name)))?;
                let ret = call_site.try_as_basic_value()
                    .left()
                    .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("{} returned void", func_name)))?;
                Ok(ret.into_int_value())
            }
            Expr::Select(obj, method, _) => self.eval_method_call(obj, method, args, ctx),
            _ => Err(LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "only direct function calls supported".into())),
        }
    }

    fn eval_method_call<'a>(&mut self, obj: &Expr, method: &str, _args: &[Expr], ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        match method {
            "len" => {
                match obj {
                    Expr::Ident(name, _) => {
                        if let Some(size) = self.array_sizes.get(name).copied() {
                            return Ok(i64_type.const_int(size as u64, false));
                        }
                        Err(LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError,
                            format!("{} has no known length", name)))
                    }
                    Expr::String(s, _) => {
                        Ok(i64_type.const_int(s.len() as u64, false))
                    }
                    Expr::Array(elems, _) => {
                        Ok(i64_type.const_int(elems.len() as u64, false))
                    }
                    _ => Err(LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError,
                        ".len() only supported on arrays and strings".into())),
                }
            }
            _ => Err(LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError,
                format!("unknown method: .{}", method))),
        }
    }

    /// Builtin println(x): prints any basic type followed by newline
    fn builtin_println<'a>(&mut self, args: &[Expr], ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.is_empty() {
            self.emit_puts("", ctx)?;
        } else {
            self.builtin_print_value(&args[0], ctx, true)?;
        }
        Ok(ctx.module().get_context().i64_type().const_int(0, false))
    }

    /// Builtin print(x): prints without newline
    fn builtin_print<'a>(&mut self, args: &[Expr], ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        if !args.is_empty() {
            self.builtin_print_value(&args[0], ctx, false)?;
        }
        Ok(ctx.module().get_context().i64_type().const_int(0, false))
    }

    /// Print a single value (integer or string), with or without newline
    fn builtin_print_value(&mut self, expr: &Expr, ctx: &mut LlvmContext, newline: bool) -> LeoResult<()> {
        match expr {
            Expr::String(s, _) => {
                if newline { self.emit_puts(s, ctx)? } else { self.emit_print_str(s, ctx)? }
            }
            _ => {
                let val = self.eval_int(expr, ctx)?;
                if newline { self.emit_print_int(val, ctx) } else { self.emit_print_int_no_newline(val, ctx) }
            }
        }
        Ok(())
    }

    /// Builtin panic(msg): print error and call abort()
    fn builtin_panic<'a>(&mut self, args: &[Expr], ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        let msg = match args.first() {
            Some(Expr::String(s, _)) => s.clone(),
            _ => "panic".to_string(),
        };
        self.emit_puts(&format!("PANIC: {}", msg), ctx)?;
        self.emit_abort(ctx);
        Ok(ctx.module().get_context().i64_type().const_int(1, false))
    }

    /// Builtin assert(cond, msg): panic if condition is false
    fn builtin_assert<'a>(&mut self, args: &[Expr], ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.is_empty() { return Ok(ctx.module().get_context().i64_type().const_int(0, false)); }
        let cond_val = self.eval_int(&args[0], ctx)?;

        let function = ctx.builder().get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "no function".into()))?;
        let context = ctx.module().get_context();

        let pass_block = context.append_basic_block(function, "assert.pass");
        let fail_block = context.append_basic_block(function, "assert.fail");
        let zero = context.i64_type().const_int(0, false);
        let cmp = ctx.builder().build_int_compare(IntPredicate::EQ, cond_val, zero, "assert.check")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "assert compare failed".into()))?;
        ctx.builder().build_conditional_branch(cmp, fail_block, pass_block)
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "assert branch failed".into()))?;

        ctx.builder().position_at_end(fail_block);
        let msg = match args.get(1) {
            Some(Expr::String(s, _)) => format!("Assertion failed: {}", s),
            _ => "Assertion failed".to_string(),
        };
        self.emit_puts(&msg, ctx)?;
        self.emit_abort(ctx);

        ctx.builder().position_at_end(pass_block);
        Ok(context.i64_type().const_int(0, false))
    }

    /// Emit abort() call (for panic/assert)
    fn emit_abort(&mut self, ctx: &mut LlvmContext) {
        let abort_fn = ctx.module().get_function("abort")
            .unwrap_or_else(|| {
                let void_type = ctx.module().get_context().void_type();
                ctx.module_mut().add_function("abort", void_type.fn_type(&[], false), None)
            });
        ctx.builder().build_call(abort_fn, &[], "abort").ok();
    }

    /// Builtin str_len(s): returns string length as i64
    fn builtin_str_len<'a>(&mut self, args: &[Expr], ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.is_empty() { return Ok(ctx.module().get_context().i64_type().const_int(0, false)); }
        let str_ptr = self.eval_string_arg(&args[0], ctx)?;
        let strlen_fn = ctx.module().get_function("strlen")
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "strlen not declared".into()))?;
        let result = ctx.builder().build_call(strlen_fn, &[str_ptr.into()], "strlen_call")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "strlen call failed".into()))?;
        Ok(result.try_as_basic_value().left().ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "strlen returned void".into()))?.into_int_value())
    }

    /// Builtin str_char_at(s, i): returns ASCII code of char at index as i64
    fn builtin_str_char_at<'a>(&mut self, args: &[Expr], ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.len() < 2 { return Ok(ctx.module().get_context().i64_type().const_int(0, false)); }
        let str_ptr = self.eval_string_arg(&args[0], ctx)?;
        let idx = self.eval_int(&args[1], ctx)?;
        let context = ctx.module().get_context();
        let i8_type = context.i8_type();
        let i8_ptr = i8_type.ptr_type(AddressSpace::default());
        // GEP: get pointer to s[idx]
        let casted_ptr = ctx.builder().build_pointer_cast(str_ptr, i8_ptr, "str_to_i8ptr")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "ptr cast failed".into()))?;
        let offset_ptr = unsafe {
            ctx.builder().build_in_bounds_gep(casted_ptr, &[idx], "char_ptr")
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep failed".into()))?
        };
        let char_val = ctx.builder().build_load(offset_ptr, "char_val")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "load char failed".into()))?;
        let extended = ctx.builder().build_int_z_extend(char_val.into_int_value(), context.i64_type(), "char_ext")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "zext failed".into()))?;
        Ok(extended)
    }

    /// Builtin str_slice(s, start, end): returns new substring (allocated)
    fn builtin_str_slice<'a>(&mut self, args: &[Expr], ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.len() < 3 { return Ok(ctx.module().get_context().i64_type().const_int(0, false)); }
        let str_ptr = self.eval_string_arg(&args[0], ctx)?;
        let start = self.eval_int(&args[1], ctx)?;
        let end = self.eval_int(&args[2], ctx)?;
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i8_ptr_type = context.i8_type().ptr_type(AddressSpace::default());

        // len = end - start + 1 (for null terminator)
        let one = i64_type.const_int(1, false);
        let len = ctx.builder().build_int_sub(end, start, "slice_len")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "sub failed".into()))?;
        let alloc_size = ctx.builder().build_int_add(len, one, "alloc_size")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "add failed".into()))?;

        // dest = malloc(alloc_size)
        let malloc_fn = ctx.module().get_function("malloc")
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc not declared".into()))?;
        let dest = ctx.builder().build_call(malloc_fn, &[alloc_size.into()], "malloc_dest")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc failed".into()))?;
        let dest_ptr = dest.try_as_basic_value().left().ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc void".into()))?;
        let dest_i8 = ctx.builder().build_pointer_cast(dest_ptr.into_pointer_value(), i8_ptr_type, "dest_i8")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "ptr cast failed".into()))?;

        // src = str_ptr + start (GEP)
        let src_i8 = ctx.builder().build_pointer_cast(str_ptr, i8_ptr_type, "src_i8")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "ptr cast failed".into()))?;
        let src_offset = unsafe {
            ctx.builder().build_in_bounds_gep(src_i8, &[start], "src_offset")
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep failed".into()))?
        };

        // memcpy(dest, src, len)
        let memcpy_fn = ctx.module().get_function("memcpy")
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "memcpy not declared".into()))?;
        ctx.builder().build_call(memcpy_fn, &[dest_i8.into(), src_offset.into(), len.into()], "memcpy_call")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "memcpy failed".into()))?;

        // null terminate: dest[len] = 0
        let null_pos = unsafe {
            ctx.builder().build_in_bounds_gep(dest_i8, &[len], "null_pos")
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep failed".into()))?
        };
        ctx.builder().build_store(null_pos, context.i8_type().const_int(0, false))
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "store null failed".into()))?;

        // Return dest as i64 (pointer cast)
        let dest_as_i64 = ctx.builder().build_ptr_to_int(dest_i8, i64_type, "ptr_to_int")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "ptr_to_int failed".into()))?;
        Ok(dest_as_i64)
    }

    /// Builtin str_concat(a, b): returns new concatenated string (allocated)
    fn builtin_str_concat<'a>(&mut self, args: &[Expr], ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.len() < 2 { return Ok(ctx.module().get_context().i64_type().const_int(0, false)); }
        let a_ptr = self.eval_string_arg(&args[0], ctx)?;
        let b_ptr = self.eval_string_arg(&args[1], ctx)?;
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i8_ptr_type = context.i8_type().ptr_type(AddressSpace::default());

        // total_len = strlen(a) + strlen(b) + 1
        let strlen_fn = ctx.module().get_function("strlen")
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "strlen not declared".into()))?;
        let a_len = ctx.builder().build_call(strlen_fn, &[a_ptr.into()], "a_len")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "strlen a failed".into()))?;
        let b_len = ctx.builder().build_call(strlen_fn, &[b_ptr.into()], "b_len")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "strlen b failed".into()))?;
        let a_len_val = a_len.try_as_basic_value().left().ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "strlen void".into()))?.into_int_value();
        let b_len_val = b_len.try_as_basic_value().left().ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "strlen void".into()))?.into_int_value();
        let one = i64_type.const_int(1, false);
        let total = ctx.builder().build_int_add(ctx.builder().build_int_add(a_len_val, b_len_val, "sum")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "add failed".into()))?, one, "total")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "add failed".into()))?;

        // dest = malloc(total)
        let malloc_fn = ctx.module().get_function("malloc")
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc not declared".into()))?;
        let dest = ctx.builder().build_call(malloc_fn, &[total.into()], "malloc_concat")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc failed".into()))?;
        let dest_ptr = dest.try_as_basic_value().left().ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc void".into()))?;
        let dest_i8 = ctx.builder().build_pointer_cast(dest_ptr.into_pointer_value(), i8_ptr_type, "dest_i8")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "ptr cast failed".into()))?;

        // strcpy(dest, a) + strcat(dest, b)
        let strcpy_fn = ctx.module().get_function("strcpy")
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "strcpy not declared".into()))?;
        let strcat_fn = ctx.module().get_function("strcat")
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "strcat not declared".into()))?;
        ctx.builder().build_call(strcpy_fn, &[dest_i8.into(), a_ptr.into()], "strcpy_call").ok();
        ctx.builder().build_call(strcat_fn, &[dest_i8.into(), b_ptr.into()], "strcat_call").ok();

        let dest_as_i64 = ctx.builder().build_ptr_to_int(dest_i8, i64_type, "ptr_to_int")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "ptr_to_int failed".into()))?;
        Ok(dest_as_i64)
    }

    /// Evaluate a string expression to an i8* LLVM pointer value
    fn eval_string_arg<'a>(&mut self, expr: &Expr, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::PointerValue<'a>> {
        match expr {
            Expr::String(s, _) => {
                let gv = self.emit_string_global(s, ctx);
                let i8_ptr = ctx.module().get_context().i8_type().ptr_type(AddressSpace::default());
                let ptr = gv.as_pointer_value().const_cast(i8_ptr);
                Ok(ptr)
            }
            _ => Err(LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "expected string argument".into())),
        }
    }

    /// Create or reuse a global string constant and return the GlobalValue
    fn emit_string_global<'a>(&mut self, s: &str, ctx: &mut LlvmContext<'a>) -> inkwell::values::GlobalValue<'a> {
        let context = ctx.module().get_context();
        let null_terminated = format!("{}\0", s);
        let gv = ctx.module_mut().add_global(
            context.i8_type().array_type(null_terminated.len() as u32),
            Some(AddressSpace::default()),
            &format!("__leo_str_{}_{}", s.len(), s.len() % 1000),
        );
        gv.set_initializer(&context.const_string(null_terminated.as_bytes(), false));
        gv.set_constant(true);
        gv
    }

    /// Emit printf for integer without newline
    fn emit_print_int_no_newline<'a>(&mut self, val: inkwell::values::IntValue<'a>, ctx: &mut LlvmContext<'a>) {
        let context = ctx.module().get_context();
        let fmt = "%ld\0".to_string();
        let gv = ctx.module_mut().add_global(
            context.i8_type().array_type(fmt.len() as u32),
            Some(AddressSpace::default()),
            &format!("__leo_fmt_int_nn_{}", val),
        );
        gv.set_initializer(&context.const_string(fmt.as_bytes(), false));
        gv.set_constant(true);
        let ptr = gv.as_pointer_value()
            .const_cast(context.i8_type().ptr_type(AddressSpace::default()));
        if let Some(printf) = ctx.module().get_function("printf") {
            ctx.builder().build_call(printf, &[ptr.into(), val.into()], "print_int_nn").ok();
        }
    }

    /// Emit printf for string literal without newline
    fn emit_print_str(&mut self, s: &str, ctx: &mut LlvmContext) -> LeoResult<()> {
        let context = ctx.module().get_context();
        let fmt = format!("%s\0");
        let str_lit = format!("{}\0", s);
        let fmt_gv = ctx.module_mut().add_global(
            context.i8_type().array_type(fmt.len() as u32),
            Some(AddressSpace::default()),
            &format!("__leo_fmt_str"),
        );
        fmt_gv.set_initializer(&context.const_string(fmt.as_bytes(), false));
        fmt_gv.set_constant(true);
        let str_gv = ctx.module_mut().add_global(
            context.i8_type().array_type(str_lit.len() as u32),
            Some(AddressSpace::default()),
            &format!("__leo_str_print_{}", s.len()),
        );
        str_gv.set_initializer(&context.const_string(str_lit.as_bytes(), false));
        str_gv.set_constant(true);
        let fmt_ptr = fmt_gv.as_pointer_value()
            .const_cast(context.i8_type().ptr_type(AddressSpace::default()));
        let str_ptr = str_gv.as_pointer_value()
            .const_cast(context.i8_type().ptr_type(AddressSpace::default()));
        if let Some(printf) = ctx.module().get_function("printf") {
            ctx.builder().build_call(printf, &[fmt_ptr.into(), str_ptr.into()], "print_str").ok();
        }
        Ok(())
    }

    /// Fold constant integer arithmetic at compile time
    fn fold_constants(op: &BinOp, l: i64, r: i64) -> Option<i64> {
        match op {
            BinOp::Add => Some(l.wrapping_add(r)),
            BinOp::Sub => Some(l.wrapping_sub(r)),
            BinOp::Mul => Some(l.wrapping_mul(r)),
            BinOp::Div if r != 0 => Some(l / r),
            BinOp::Mod if r != 0 => Some(l % r),
            BinOp::Eq => Some(if l == r { 1 } else { 0 }),
            BinOp::Ne => Some(if l != r { 1 } else { 0 }),
            BinOp::Lt => Some(if l < r { 1 } else { 0 }),
            BinOp::Le => Some(if l <= r { 1 } else { 0 }),
            BinOp::Gt => Some(if l > r { 1 } else { 0 }),
            BinOp::Ge => Some(if l >= r { 1 } else { 0 }),
            _ => None,
        }
    }

    /// Build explicit return with value, respecting function return type
    fn build_return_with<'a>(&mut self, val: inkwell::values::IntValue<'a>, ctx: &mut LlvmContext<'a>) -> LeoResult<()> {
        let context = ctx.module().get_context();
        let ret_val: BasicValueEnum = if let Some(fv) = ctx.current_fn() {
            let fn_type = fv.get_type();
            match fn_type.get_return_type() {
                Some(BasicTypeEnum::IntType(int_ty)) if int_ty == context.i32_type() => {
                    let trunc = ctx.builder().build_int_truncate(val, context.i32_type(), "ret.trunc")
                        .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "truncate failed".into()))?;
                    trunc.into()
                }
                _ => val.into(),
            }
        } else {
            val.into()
        };
        ctx.builder().build_return(Some(&ret_val))
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "return failed".into()))?;
        Ok(())
    }

    /// Emit binary arithmetic/comparison/logic (z-extends comparison results to i64)
    fn emit_binop<'a>(&mut self, op: &BinOp, lv: inkwell::values::IntValue<'a>, rv: inkwell::values::IntValue<'a>, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        let i64_type = ctx.module().get_context().i64_type();
        match op {
            BinOp::Add => ctx.builder().build_int_add(lv, rv, "add"),
            BinOp::Sub => ctx.builder().build_int_sub(lv, rv, "sub"),
            BinOp::Mul => ctx.builder().build_int_mul(lv, rv, "mul"),
            BinOp::Div => ctx.builder().build_int_signed_div(lv, rv, "div"),
            BinOp::Mod => ctx.builder().build_int_signed_rem(lv, rv, "rem"),
            BinOp::Eq => ctx.builder().build_int_compare(IntPredicate::EQ, lv, rv, "eq")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "eq.ext")),
            BinOp::Ne => ctx.builder().build_int_compare(IntPredicate::NE, lv, rv, "ne")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "ne.ext")),
            BinOp::Lt => ctx.builder().build_int_compare(IntPredicate::SLT, lv, rv, "lt")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "lt.ext")),
            BinOp::Le => ctx.builder().build_int_compare(IntPredicate::SLE, lv, rv, "le")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "le.ext")),
            BinOp::Gt => ctx.builder().build_int_compare(IntPredicate::SGT, lv, rv, "gt")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "gt.ext")),
            BinOp::Ge => ctx.builder().build_int_compare(IntPredicate::SGE, lv, rv, "ge")
                .and_then(|v| ctx.builder().build_int_z_extend(v, i64_type, "ge.ext")),
            BinOp::And => ctx.builder().build_and(lv, rv, "and"),
            BinOp::Or => ctx.builder().build_or(lv, rv, "or"),
            _ => return Ok(lv),
        }.map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("{:?} failed", op)))
    }

    /// Emit unary operation (negate, bitwise not)
    fn emit_unop<'a>(&mut self, op: &UnOp, val: inkwell::values::IntValue<'a>, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        match op {
            UnOp::Neg | UnOp::Minus => {
                let zero = ctx.module().get_context().i64_type().const_int(0, false);
                ctx.builder().build_int_sub(zero, val, "neg")
            }
            UnOp::Not => {
                let ones = ctx.module().get_context().i64_type().const_int(u64::MAX, true);
                ctx.builder().build_xor(val, ones, "not")
            }
            _ => return Ok(val),
        }.map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, format!("{:?} failed", op)))
    }

    /// Emit printf("%ld\n", val) to print an i64
    fn emit_print_int<'a>(&mut self, val: inkwell::values::IntValue<'a>, ctx: &mut LlvmContext<'a>) {
        let context = ctx.module().get_context();
        let fmt = format!("%ld\n\0");
        let gv = ctx.module_mut().add_global(
            context.i8_type().array_type(fmt.len() as u32),
            Some(AddressSpace::default()),
            &format!("__leo_fmt_int_{}", val),
        );
        gv.set_initializer(&context.const_string(fmt.as_bytes(), false));
        gv.set_constant(true);
        let ptr = gv.as_pointer_value()
            .const_cast(context.i8_type().ptr_type(AddressSpace::default()));
        if let Some(printf) = ctx.module().get_function("printf") {
            ctx.builder().build_call(printf, &[ptr.into(), val.into()], "print_int").ok();
        }
    }

    /// Emit puts(string) for string literal
    fn emit_puts(&mut self, s: &str, ctx: &mut LlvmContext) -> LeoResult<()> {
        let context = ctx.module().get_context();
        let fmt = format!("{}\0", s);
        let gv = ctx.module_mut().add_global(
            context.i8_type().array_type(fmt.len() as u32),
            Some(AddressSpace::default()),
            &format!("__leo_str_{}", s.len()),
        );
        gv.set_initializer(&context.const_string(fmt.as_bytes(), false));
        gv.set_constant(true);
        let ptr = gv.as_pointer_value()
            .const_cast(context.i8_type().ptr_type(AddressSpace::default()));
        if let Some(puts) = ctx.module().get_function("puts") {
            ctx.builder().build_call(puts, &[ptr.into()], "puts_call")
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "puts failed".into()))?;
        }
        Ok(())
    }

    fn eval_index<'a>(&mut self, obj: &Expr, idx: &Expr, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        let obj_val = self.eval_int(obj, ctx)?;
        let idx_val = self.eval_int(idx, ctx)?;
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
        let obj_ptr = ctx.builder().build_int_to_ptr(obj_val, i64_ptr_type, "obj_ptr")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "int_to_ptr failed".into()))?;
        let elem_ptr = unsafe {
            ctx.builder().build_in_bounds_gep(obj_ptr, &[idx_val], "elem_ptr")
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep failed".into()))?
        };
        let loaded = ctx.builder().build_load(elem_ptr, "elem_val")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "load elem failed".into()))?;
        Ok(loaded.into_int_value())
    }

    fn eval_select<'a>(&mut self, obj: &Expr, field: &str, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        match field {
            "len" => {
                match obj {
                    Expr::Ident(name, _) => {
                        let size = self.array_sizes.get(name).copied()
                            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError,
                                format!("{} has no known length", name)))?;
                        Ok(i64_type.const_int(size as u64, false))
                    }
                    Expr::String(s, _) => {
                        Ok(i64_type.const_int(s.len() as u64, false))
                    }
                    _ => Err(LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError,
                        ".len() only supported on named arrays and string literals".into())),
                }
            }
            _ => {
                let obj_val = self.eval_int(obj, ctx)?;
                let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
                let obj_ptr = ctx.builder().build_int_to_ptr(obj_val, i64_ptr_type, "struct_ptr")
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "int_to_ptr failed".into()))?;
                let field_idx = match field {
                    "kind" | "x" | "a" => i64_type.const_int(0, false),
                    "value" | "y" | "b" => i64_type.const_int(1, false),
                    _ => i64_type.const_int(0, false),
                };
                let field_ptr = unsafe {
                    ctx.builder().build_in_bounds_gep(obj_ptr, &[field_idx], "field_ptr")
                        .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "struct gep failed".into()))?
                };
                let loaded = ctx.builder().build_load(field_ptr, "field_val")
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "load field failed".into()))?;
                Ok(loaded.into_int_value())
            }
        }
    }

    fn eval_array_alloc<'a>(&mut self, expr: &Expr, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
        match expr {
            Expr::Array(elements, _) => {
                let count = elements.len() as u64;
                let alloc_size = i64_type.const_int(count * 8, false);
                let malloc_fn = ctx.module().get_function("malloc")
                    .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc not declared".into()))?;
                let mem = ctx.builder().build_call(malloc_fn, &[alloc_size.into()], "array_malloc")
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc failed".into()))?;
                let mem_ptr = mem.try_as_basic_value().left()
                    .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc void".into()))?;
                let base = ctx.builder().build_pointer_cast(mem_ptr.into_pointer_value(), i64_ptr_type, "array_base")
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "ptr cast failed".into()))?;
                for (i, elem) in elements.iter().enumerate() {
                    let val = self.eval_int(elem, ctx)?;
                    let idx = i64_type.const_int(i as u64, false);
                    let elem_ptr = unsafe {
                        ctx.builder().build_in_bounds_gep(base, &[idx], "store_ptr")
                            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep failed".into()))?
                    };
                    ctx.builder().build_store(elem_ptr, val)
                        .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "store elem failed".into()))?;
                }
                let result = ctx.builder().build_ptr_to_int(base, i64_type, "array_as_int")
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "ptr_to_int failed".into()))?;
                Ok(result)
            }
            Expr::ArrayRepeat(val, count_expr, _) => {
                let count_val = self.eval_int(count_expr, ctx)?;
                let count_const = match count_expr.as_ref() {
                    Expr::Number(n, _) => *n as u64,
                    _ => 1,
                };
                let alloc_size = ctx.builder().build_int_mul(
                    count_val, i64_type.const_int(8, false), "alloc_size"
                ).map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "mul failed".into()))?;
                let malloc_fn = ctx.module().get_function("malloc")
                    .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc not declared".into()))?;
                let mem = ctx.builder().build_call(malloc_fn, &[alloc_size.into()], "array_malloc")
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc failed".into()))?;
                let mem_ptr = mem.try_as_basic_value().left()
                    .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc void".into()))?;
                let base = ctx.builder().build_pointer_cast(mem_ptr.into_pointer_value(), i64_ptr_type, "array_base")
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "ptr cast failed".into()))?;
                let fill_val = self.eval_int(val, ctx)?;
                for i in 0..count_const.min(1024) {
                    let idx = i64_type.const_int(i as u64, false);
                    let elem_ptr = unsafe {
                        ctx.builder().build_in_bounds_gep(base, &[idx], "store_ptr")
                            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep failed".into()))?
                    };
                    ctx.builder().build_store(elem_ptr, fill_val)
                        .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "store elem failed".into()))?;
                }
                let result = ctx.builder().build_ptr_to_int(base, i64_type, "array_as_int")
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "ptr_to_int failed".into()))?;
                Ok(result)
            }
            _ => Ok(i64_type.const_int(0, false)),
        }
    }

    fn eval_struct_init<'a>(&mut self, expr: &Expr, ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
        match expr {
            Expr::StructInit(_name, fields, _) => {
                let total_size = i64_type.const_int(fields.len() as u64 * 8, false);
                let malloc_fn = ctx.module().get_function("malloc")
                    .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc not declared".into()))?;
                let mem = ctx.builder().build_call(malloc_fn, &[total_size.into()], "struct_malloc")
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc failed".into()))?;
                let mem_ptr = mem.try_as_basic_value().left()
                    .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "malloc void".into()))?;
                let base = ctx.builder().build_pointer_cast(mem_ptr.into_pointer_value(), i64_ptr_type, "struct_base")
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "ptr cast failed".into()))?;
                for (i, (_fname, fval)) in fields.iter().enumerate() {
                    let val = self.eval_int(fval, ctx)?;
                    let idx = i64_type.const_int(i as u64, false);
                    let field_ptr = unsafe {
                        ctx.builder().build_in_bounds_gep(base, &[idx], "field_store")
                            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep failed".into()))?
                    };
                    ctx.builder().build_store(field_ptr, val)
                        .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "store field failed".into()))?;
                }
                let result = ctx.builder().build_ptr_to_int(base, i64_type, "struct_as_int")
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "ptr_to_int failed".into()))?;
                Ok(result)
            }
            _ => Ok(i64_type.const_int(0, false)),
        }
    }

    fn eval_enum_constructor<'a>(&mut self, qualified: &str, args: &[Expr], ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        let parts: Vec<&str> = qualified.split("::").collect();
        if parts.len() != 2 {
            return Err(LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError,
                format!("invalid enum variant: {}", qualified)));
        }
        let enum_name = parts[0];
        let variant_name = parts[1];
        let tag = ctx.get_enum_variant_tag(enum_name, variant_name)
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError,
                format!("unknown variant: {}", qualified)))?;
        let context = ctx.module().get_context();
        let i32_type = context.i32_type();
        let i64_type = context.i64_type();
        let struct_type = ctx.module().get_struct_type(enum_name)
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError,
                format!("enum {} not defined in LLVM", enum_name)))?;
        let enum_ptr = ctx.builder().build_alloca(struct_type, qualified)
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "alloca enum failed".into()))?;
        let tag_ptr = ctx.builder().build_struct_gep(enum_ptr, 0, "tag_ptr")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "struct gep tag failed".into()))?;
        ctx.builder().build_store(tag_ptr, i32_type.const_int(tag as u64, false))
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "store tag failed".into()))?;
        if !args.is_empty() {
            let payload_ptr = ctx.builder().build_struct_gep(enum_ptr, 1, "payload_ptr")
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "struct gep payload failed".into()))?;
            let i64_ptr_type = i64_type.ptr_type(AddressSpace::default());
            let payload_as_i64 = ctx.builder().build_pointer_cast(payload_ptr, i64_ptr_type, "payload_i64")
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "ptr cast failed".into()))?;
            for (i, arg) in args.iter().enumerate() {
                let val = self.eval_int(arg, ctx)?;
                let idx = i64_type.const_int(i as u64, false);
                let field_ptr = unsafe {
                    ctx.builder().build_in_bounds_gep(payload_as_i64, &[idx], "field_ptr")
                        .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep failed".into()))?
                };
                ctx.builder().build_store(field_ptr, val)
                    .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "store field failed".into()))?;
            }
        }
        let result = ctx.builder().build_ptr_to_int(enum_ptr, i64_type, "enum_as_int")
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "ptr_to_int failed".into()))?;
        Ok(result)
    }

    fn eval_match<'a>(&mut self, scrutinee: &Expr, arms: &[(Expr, Expr)], ctx: &mut LlvmContext<'a>) -> LeoResult<inkwell::values::IntValue<'a>> {
        let context = ctx.module().get_context();
        let i32_type = context.i32_type();
        let i64_type = context.i64_type();
        let scrut_val = self.eval_int(scrutinee, ctx)?;
        let function = ctx.builder().get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "no function".into()))?;
        let is_enum_match = arms.iter().any(|(pat, _)| {
            match pat {
                Expr::Ident(name, _) => name.contains("::"),
                Expr::Call(callee, _, _) => matches!(callee.as_ref(), Expr::Ident(n, _) if n.contains("::")),
                _ => false,
            }
        });
        let switch_val = if is_enum_match {
            let scrut_ptr = ctx.builder().build_int_to_ptr(scrut_val, i32_type.ptr_type(AddressSpace::default()), "match_ptr")
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "int_to_ptr failed".into()))?;
            let tag = ctx.builder().build_load(scrut_ptr, "match_tag")
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "load tag failed".into()))?
                .into_int_value();
            let tag_i64 = ctx.builder().build_int_z_extend(tag, i64_type, "tag_i64")
                .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "zext failed".into()))?;
            tag_i64
        } else {
            scrut_val
        };
        let merge_block = context.append_basic_block(function, "match.merge");
        let mut cases: Vec<(inkwell::values::IntValue, inkwell::basic_block::BasicBlock)> = Vec::new();
        let mut arm_blocks: Vec<(inkwell::basic_block::BasicBlock, &Expr)> = Vec::new();
        let mut default_block: Option<inkwell::basic_block::BasicBlock> = None;
        for (i, (pattern, _body)) in arms.iter().enumerate() {
            let arm_block = context.append_basic_block(function, &format!("match.arm.{}", i));
            match pattern {
                Expr::Ident(name, _) if name == "_" => {
                    default_block = Some(arm_block);
                }
                Expr::Ident(name, _) if name.contains("::") => {
                    let parts: Vec<&str> = name.split("::").collect();
                    if parts.len() == 2 {
                        if let Some(tag_idx) = ctx.get_enum_variant_tag(parts[0], parts[1]) {
                            cases.push((i64_type.const_int(tag_idx as u64, false), arm_block));
                        }
                    }
                }
                Expr::Call(callee, _, _) => {
                    if let Expr::Ident(name, _) = callee.as_ref() {
                        if name.contains("::") {
                            let parts: Vec<&str> = name.split("::").collect();
                            if parts.len() == 2 {
                                if let Some(tag_idx) = ctx.get_enum_variant_tag(parts[0], parts[1]) {
                                    cases.push((i64_type.const_int(tag_idx as u64, false), arm_block));
                                }
                            }
                        }
                    }
                }
                _ => {
                    if let Expr::Number(n, _) = pattern {
                        cases.push((i64_type.const_int(*n as u64, false), arm_block));
                    } else {
                        cases.push((i64_type.const_int(i as u64, false), arm_block));
                    }
                }
            }
            arm_blocks.push((arm_block, _body));
        }
        let default = default_block.unwrap_or_else(|| context.append_basic_block(function, "match.default"));
        ctx.builder().build_switch(switch_val, default, &cases)
            .map_err(|_| LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "switch failed".into()))?;
        for (arm_block, body) in &arm_blocks {
            ctx.builder().position_at_end(*arm_block);
            let result = self.eval_int(body, ctx)?;
            let _ = result;
            self.emit_branch(merge_block, ctx)?;
        }
        if default_block.is_none() {
            ctx.builder().position_at_end(default);
            self.emit_branch(merge_block, ctx)?;
        }
        ctx.builder().position_at_end(merge_block);
        Ok(i64_type.const_int(0, false))
    }

    /// Map Leo type string to LLVM type
    pub fn llvm_type<'ctx>(ty: &str, ctx: &LlvmContext<'ctx>) -> BasicTypeEnum<'ctx> {
        let context = ctx.module().get_context();
        match ty {
            "i8" | "u8" | "char" => context.i8_type().into(),
            "i32" => context.i32_type().into(),
            "i64" => context.i64_type().into(),
            "f64" => context.f64_type().into(),
            "bool" => context.bool_type().into(),
            _ => context.i64_type().into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::context::Context;

    #[test]
    fn test_ir_builder_new() {
        let mut builder = IrBuilder::new();
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test");
        assert!(builder.build(&[], &mut ctx).is_ok());
    }
}
