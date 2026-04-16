use super::*;
use crate::ast::expr::Expr;
use crate::common::error::{ErrorCode, ErrorKind, LeoError, LeoResult};
use inkwell::IntPredicate;
use inkwell::AddressSpace;

impl IrBuilder {
    pub(super) fn builtin_file_read<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.is_empty() {
            return Ok(ctx.module().get_context().i64_type().const_int(0, false));
        }
        if let Expr::String(s, _) = &args[0] {
            if s.contains("..") || s.starts_with('/') {
                return Err(LeoError::new(
                    ErrorKind::Semantic,
                    ErrorCode::SemaTypeMismatch,
                    format!("file_read: path traversal blocked: {}", s),
                ));
            }
        }
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i32_type = context.i32_type();
        let i8_ptr_type = context.i8_type().ptr_type(AddressSpace::default());
        let path_ptr = self.eval_string_arg(&args[0], ctx)?;
        let strstr_fn = ctx.module().get_function("strstr").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strstr not declared".into(),
            )
        })?;
        let dotdot_gv = self.emit_string_global("..", ctx);
        let dotdot_ptr = dotdot_gv.as_pointer_value().const_cast(i8_ptr_type);
        let dotdot_result = ctx
            .builder()
            .build_call(
                strstr_fn,
                &[path_ptr.into(), dotdot_ptr.into()],
                "path_dotdot",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strstr dotdot failed".into(),
                )
            })?;
        let dotdot_val = dotdot_result.try_as_basic_value().left().ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strstr returned void".into(),
            )
        })?;
        let dotdot_found = ctx
            .builder()
            .build_ptr_to_int(dotdot_val.into_pointer_value(), i64_type, "dotdot_i64")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        let func = ctx
            .builder()
            .get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "no function".into(),
                )
            })?;
        let path_fail_bb = context.append_basic_block(func, "path_trav_fail");
        let path_ok_bb = context.append_basic_block(func, "path_trav_ok");
        let zero = i64_type.const_int(0, false);
        let dotdot_present = ctx
            .builder()
            .build_int_compare(IntPredicate::NE, dotdot_found, zero, "dotdot_present")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "compare failed".into(),
                )
            })?;
        ctx.builder()
            .build_conditional_branch(dotdot_present, path_fail_bb, path_ok_bb)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "branch failed".into(),
                )
            })?;
        ctx.builder().position_at_end(path_fail_bb);
        self.emit_puts("runtime error: path traversal blocked\n", ctx)?;
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();
        ctx.builder().position_at_end(path_ok_bb);
        let fopen_fn = ctx.module().get_function("fopen").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "fopen not declared".into(),
            )
        })?;
        let fread_fn = ctx.module().get_function("fread").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "fread not declared".into(),
            )
        })?;
        let fclose_fn = ctx.module().get_function("fclose").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "fclose not declared".into(),
            )
        })?;
        let malloc_fn = ctx.module().get_function("malloc").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "malloc not declared".into(),
            )
        })?;
        let mode_gv = self.emit_string_global("rb", ctx);
        let mode_ptr = mode_gv.as_pointer_value().const_cast(i8_ptr_type);
        let fp = ctx
            .builder()
            .build_call(fopen_fn, &[path_ptr.into(), mode_ptr.into()], "fopen")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fopen".into(),
                )
            })?;
        let fp_ptr = fp
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "void".into(),
                )
            })?
            .into_pointer_value();
        // NULL check: abort if fopen for read failed
        let fp_i64 = ctx
            .builder()
            .build_ptr_to_int(fp_ptr, i64_type, "fp_i64")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        self.emit_null_check(fp_i64, "runtime error: cannot open file\n", ctx)?;
        let fseek_fn = ctx.module().get_function("fseek").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "fseek not declared".into(),
            )
        })?;
        let ftell_fn = ctx.module().get_function("ftell").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "ftell not declared".into(),
            )
        })?;
        // fseek(fp, 0, SEEK_END) to get file size
        let seek_end = i32_type.const_int(2, false);
        let zero_i64 = i64_type.const_int(0, false);
        ctx.builder()
            .build_call(
                fseek_fn,
                &[fp_ptr.into(), zero_i64.into(), seek_end.into()],
                "fseek_end",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fseek_end".into(),
                )
            })?;
        // ftell(fp) → file_size
        let ftell_result = ctx
            .builder()
            .build_call(ftell_fn, &[fp_ptr.into()], "ftell")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ftell".into(),
                )
            })?;
        let file_size = ftell_result
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ftell void".into(),
                )
            })?
            .into_int_value();
        // fseek(fp, 0, SEEK_SET) to rewind
        let seek_set = i32_type.const_int(0, false);
        ctx.builder()
            .build_call(
                fseek_fn,
                &[fp_ptr.into(), zero_i64.into(), seek_set.into()],
                "fseek_set",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fseek_set".into(),
                )
            })?;
        // malloc(file_size + 1)
        let one_i64 = i64_type.const_int(1, false);
        let alloc_size = ctx
            .builder()
            .build_int_add(file_size, one_i64, "alloc_size")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "alloc_size add failed".into(),
                )
            })?;
        let buf = ctx
            .builder()
            .build_call(malloc_fn, &[alloc_size.into()], "fbuf")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "malloc".into(),
                )
            })?;
        let buf_raw = buf
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "void".into(),
                )
            })?
            .into_pointer_value();
        // NULL check: abort if file buffer malloc failed
        let buf_i64 = ctx
            .builder()
            .build_ptr_to_int(buf_raw, i64_type, "buf_i64")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        self.emit_null_check(buf_i64, "runtime error: out of memory\n", ctx)?;
        let buf_ptr = ctx
            .builder()
            .build_pointer_cast(buf_raw, i8_ptr_type, "fbuf")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cast".into(),
                )
            })?;
        let one = i64_type.const_int(1, false);
        // fread(buf, 1, file_size, fp)
        ctx.builder()
            .build_call(
                fread_fn,
                &[buf_ptr.into(), one.into(), file_size.into(), fp_ptr.into()],
                "fread",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fread".into(),
                )
            })?;
        ctx.builder()
            .build_call(fclose_fn, &[fp_ptr.into()], "fclose")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fclose".into(),
                )
            })?;
        // null-terminate at buf[file_size]
        let null_pos = unsafe {
            ctx.builder()
                .build_in_bounds_gep(buf_ptr, &[file_size], "np")
                .map_err(|_| {
                    LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "gep".into())
                })?
        };
        ctx.builder()
            .build_store(null_pos, context.i8_type().const_int(0, false))
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "store".into(),
                )
            })?;
        ctx.builder()
            .build_ptr_to_int(buf_ptr, i64_type, "fbuf_i64")
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "p2i".into())
            })
    }

    pub(super) fn builtin_file_write<'a>(
        &mut self,
        args: &[Expr],
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        if args.len() < 2 {
            return Ok(ctx.module().get_context().i64_type().const_int(0, false));
        }
        if let Expr::String(s, _) = &args[0] {
            if s.contains("..") || s.starts_with('/') {
                return Err(LeoError::new(
                    ErrorKind::Semantic,
                    ErrorCode::SemaTypeMismatch,
                    format!("file_write: path traversal blocked: {}", s),
                ));
            }
        }
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i8_ptr_type = context.i8_type().ptr_type(AddressSpace::default());
        let path_ptr = self.eval_string_arg(&args[0], ctx)?;
        let strstr_fn = ctx.module().get_function("strstr").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strstr not declared".into(),
            )
        })?;
        let dotdot_gv = self.emit_string_global("..", ctx);
        let dotdot_ptr = dotdot_gv.as_pointer_value().const_cast(i8_ptr_type);
        let dotdot_result = ctx
            .builder()
            .build_call(
                strstr_fn,
                &[path_ptr.into(), dotdot_ptr.into()],
                "wpath_dotdot",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strstr dotdot failed".into(),
                )
            })?;
        let dotdot_val = dotdot_result.try_as_basic_value().left().ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strstr returned void".into(),
            )
        })?;
        let dotdot_found = ctx
            .builder()
            .build_ptr_to_int(dotdot_val.into_pointer_value(), i64_type, "wdotdot_i64")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        let func = ctx
            .builder()
            .get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "no function".into(),
                )
            })?;
        let wpath_fail = context.append_basic_block(func, "wpath_trav_fail");
        let wpath_ok = context.append_basic_block(func, "wpath_trav_ok");
        let zero = i64_type.const_int(0, false);
        let dotdot_present = ctx
            .builder()
            .build_int_compare(IntPredicate::NE, dotdot_found, zero, "wdotdot_present")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "compare failed".into(),
                )
            })?;
        ctx.builder()
            .build_conditional_branch(dotdot_present, wpath_fail, wpath_ok)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "branch failed".into(),
                )
            })?;
        ctx.builder().position_at_end(wpath_fail);
        self.emit_puts("runtime error: path traversal blocked\n", ctx)?;
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();
        ctx.builder().position_at_end(wpath_ok);
        let content_ptr = self.eval_string_arg(&args[1], ctx)?;
        let fopen_fn = ctx.module().get_function("fopen").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "fopen".into(),
            )
        })?;
        let fwrite_fn = ctx.module().get_function("fwrite").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "fwrite".into(),
            )
        })?;
        let fclose_fn = ctx.module().get_function("fclose").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "fclose".into(),
            )
        })?;
        let strlen_fn = ctx.module().get_function("strlen").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strlen".into(),
            )
        })?;
        let mode_gv = self.emit_string_global("w", ctx);
        let mode_ptr = mode_gv.as_pointer_value().const_cast(i8_ptr_type);
        let fp = ctx
            .builder()
            .build_call(fopen_fn, &[path_ptr.into(), mode_ptr.into()], "fopen_w")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fopen".into(),
                )
            })?;
        let fp_ptr = fp
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "void".into(),
                )
            })?
            .into_pointer_value();
        // NULL check: abort if fopen for write failed
        let fp_i64 = ctx
            .builder()
            .build_ptr_to_int(fp_ptr, i64_type, "fp_i64")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        self.emit_null_check(fp_i64, "runtime error: cannot open file\n", ctx)?;
        let clen = ctx
            .builder()
            .build_call(strlen_fn, &[content_ptr.into()], "clen")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen".into(),
                )
            })?;
        let clen_val = clen
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "void".into(),
                )
            })?
            .into_int_value();
        let one = i64_type.const_int(1, false);
        ctx.builder()
            .build_call(
                fwrite_fn,
                &[
                    content_ptr.into(),
                    one.into(),
                    clen_val.into(),
                    fp_ptr.into(),
                ],
                "fwrite",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fwrite".into(),
                )
            })?;
        ctx.builder()
            .build_call(fclose_fn, &[fp_ptr.into()], "fclose_w")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fclose".into(),
                )
            })?;
        Ok(i64_type.const_int(0, false))
    }
}
