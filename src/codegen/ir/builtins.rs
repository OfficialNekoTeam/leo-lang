use super::*;
use crate::ast::expr::Expr;
use crate::common::error::LeoResult;
use inkwell::values::IntValue;

impl IrBuilder {
    pub(super) fn try_dispatch_builtin<'a>(
        &mut self,
        func_name: &str,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<Option<IntValue<'a>>> {
        match func_name {
            "println" => self.builtin_println(args, ctx).map(Some),
            "print" => self.builtin_print(args, ctx).map(Some),
            "panic" => self.builtin_panic(args, ctx).map(Some),
            "assert" => self.builtin_assert(args, ctx).map(Some),
            "str_len" => self.builtin_str_len(args, ctx).map(Some),
            "str_char_at" => self.builtin_str_char_at(args, ctx).map(Some),
            "str_slice" => self.builtin_str_slice(args, ctx).map(Some),
            "str_concat" => self.builtin_str_concat(args, ctx).map(Some),
            "vec_new" => self.builtin_vec_new(args, ctx).map(Some),
            "vec_push" => self.builtin_vec_push(args, ctx).map(Some),
            "vec_get" => self.builtin_vec_get(args, ctx).map(Some),
            "vec_len" => self.builtin_vec_len(args, ctx).map(Some),
            "file_read" => self.builtin_file_read(args, ctx).map(Some),
            "file_write" => self.builtin_file_write(args, ctx).map(Some),
            "char_to_str" => self.builtin_char_to_str(args, ctx).map(Some),
            "is_digit" => self.builtin_is_digit(args, ctx).map(Some),
            "is_alpha" => self.builtin_is_alpha(args, ctx).map(Some),
            "is_alnum" => self.builtin_is_alnum(args, ctx).map(Some),
            "to_string" => self.builtin_to_string(args, ctx).map(Some),
            "free" => self.builtin_free(args, ctx).map(Some),
            _ => Ok(None),
        }
    }

    pub(super) fn builtin_free<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<IntValue<'a>> {
        if args.is_empty() {
            return Ok(ctx.module().get_context().i64_type().const_int(0, false));
        }
        let tv = self.eval_expr(&args[0], ctx)?;
        
        // Ensure "free" is declared
        let free_fn = ctx.module().get_function("free").unwrap_or_else(|| {
            let i8_ptr_type = ctx.module().get_context().i8_type().ptr_type(inkwell::AddressSpace::default());
            let void_type = ctx.module().get_context().void_type();
            let fn_type = void_type.fn_type(&[i8_ptr_type.into()], false);
            ctx.module_mut().add_function("free", fn_type, None)
        });

        let ptr = if tv.value.is_pointer_value() {
            tv.value.into_pointer_value()
        } else if tv.value.is_int_value() {
            let i8_ptr_type = ctx.module().get_context().i8_type().ptr_type(inkwell::AddressSpace::default());
            ctx.builder().build_int_to_ptr(tv.value.into_int_value(), i8_ptr_type, "free.i2p").map_err(|_| crate::common::error::LeoError::new(crate::common::error::ErrorKind::Syntax, crate::common::error::ErrorCode::CodegenLLVMError, "int_to_ptr for free failed".into()))?
        } else {
            return Err(crate::common::error::LeoError::new(
                crate::common::error::ErrorKind::Syntax,
                crate::common::error::ErrorCode::CodegenLLVMError,
                "can only free pointers or valid memory integers".into(),
            ));
        };

        ctx.builder().build_call(free_fn, &[ptr.into()], "call_free").map_err(|_| crate::common::error::LeoError::new(crate::common::error::ErrorKind::Syntax, crate::common::error::ErrorCode::CodegenLLVMError, "call free failed".into()))?;
        Ok(ctx.module().get_context().i64_type().const_int(0, false))
    }
}
