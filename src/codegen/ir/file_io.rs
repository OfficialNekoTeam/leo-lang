use super::*;
use crate::ast::expr::Expr;
use crate::common::error::{ErrorCode, ErrorKind, LeoError, LeoResult};
use inkwell::AddressSpace;
use inkwell::IntPredicate;

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
            // MH2: null bytes bypass C strstr; reject at compile time for string literals
            if s.contains('\0') {
                return Err(LeoError::new(
                    ErrorKind::Semantic,
                    ErrorCode::SemaTypeMismatch,
                    "file_read: path contains null byte".into(),
                ));
            }
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
        self.emit_puts(ERR_PATH_TRAVERSAL, ctx)?;
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();
        ctx.builder().position_at_end(path_ok_bb);
        // Runtime check: reject absolute paths (leading '/' or '\')
        let abs_fail_bb = context.append_basic_block(func, "abs_path_fail");
        let abs_ok_bb = context.append_basic_block(func, "abs_path_ok");
        let first_byte = ctx
            .builder()
            .build_load(path_ptr, "first_byte")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load first byte".into(),
                )
            })?
            .into_int_value();
        let slash = context.i8_type().const_int(0x2F, false);
        let backslash = context.i8_type().const_int(0x5C, false);
        let is_unix_abs = ctx
            .builder()
            .build_int_compare(IntPredicate::EQ, first_byte, slash, "is_unix_abs")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "compare failed".into(),
                )
            })?;
        let is_win_abs = ctx
            .builder()
            .build_int_compare(IntPredicate::EQ, first_byte, backslash, "is_win_abs")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "compare failed".into(),
                )
            })?;
        let is_abs = ctx
            .builder()
            .build_or(is_unix_abs, is_win_abs, "is_abs")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "or failed".into(),
                )
            })?;
        ctx.builder()
            .build_conditional_branch(is_abs, abs_fail_bb, abs_ok_bb)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "branch failed".into(),
                )
            })?;
        ctx.builder().position_at_end(abs_fail_bb);
        self.emit_puts(ERR_PATH_TRAVERSAL, ctx)?;
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();
        ctx.builder().position_at_end(abs_ok_bb);
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
        let mode_gv = self.emit_string_global("rb", ctx);
        let mode_ptr = mode_gv.as_pointer_value().const_cast(i8_ptr_type);
        let fp_ptr = self.emit_checked_fopen(path_ptr, mode_ptr, "fopen", ctx)?;
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
        let seek_end_result = ctx
            .builder()
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
        let seek_end_status = seek_end_result
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fseek_end void".into(),
                )
            })?
            .into_int_value();
        self.emit_file_int_check(
            seek_end_status,
            IntPredicate::NE,
            i32_type.const_int(0, false),
            ERR_FILE_READ_SIZE,
            fp_ptr,
            fclose_fn,
            ctx,
        )?;
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
        self.emit_file_int_check(
            file_size,
            IntPredicate::SLT,
            i64_type.const_int(0, false),
            ERR_FILE_READ_SIZE,
            fp_ptr,
            fclose_fn,
            ctx,
        )?;
        let max_file_size = i64_type.const_int(MAX_FILE_READ_BYTES + 1, false);
        self.emit_file_int_check(
            file_size,
            IntPredicate::SGE,
            max_file_size,
            ERR_FILE_READ_TOO_LARGE,
            fp_ptr,
            fclose_fn,
            ctx,
        )?;
        // fseek(fp, 0, SEEK_SET) to rewind
        let seek_set = i32_type.const_int(0, false);
        let seek_set_result = ctx
            .builder()
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
        let seek_set_status = seek_set_result
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fseek_set void".into(),
                )
            })?
            .into_int_value();
        self.emit_file_int_check(
            seek_set_status,
            IntPredicate::NE,
            i32_type.const_int(0, false),
            ERR_FILE_READ_SIZE,
            fp_ptr,
            fclose_fn,
            ctx,
        )?;
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
        let buf_raw = self.emit_file_checked_malloc(alloc_size, "fbuf", fp_ptr, fclose_fn, ctx)?;
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
            // MH2: null bytes bypass C strstr; reject at compile time for string literals
            if s.contains('\0') {
                return Err(LeoError::new(
                    ErrorKind::Semantic,
                    ErrorCode::SemaTypeMismatch,
                    "file_write: path contains null byte".into(),
                ));
            }
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
        self.emit_puts(ERR_PATH_TRAVERSAL, ctx)?;
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();
        ctx.builder().position_at_end(wpath_ok);
        // Runtime check: reject absolute paths (leading '/' or '\')
        let wabs_fail_bb = context.append_basic_block(func, "wabs_path_fail");
        let wabs_ok_bb = context.append_basic_block(func, "wabs_path_ok");
        let wfirst_byte = ctx
            .builder()
            .build_load(path_ptr, "wfirst_byte")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load first byte".into(),
                )
            })?
            .into_int_value();
        let wslash = context.i8_type().const_int(0x2F, false);
        let wbackslash = context.i8_type().const_int(0x5C, false);
        let wis_unix_abs = ctx
            .builder()
            .build_int_compare(IntPredicate::EQ, wfirst_byte, wslash, "wis_unix_abs")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "compare failed".into(),
                )
            })?;
        let wis_win_abs = ctx
            .builder()
            .build_int_compare(IntPredicate::EQ, wfirst_byte, wbackslash, "wis_win_abs")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "compare failed".into(),
                )
            })?;
        let wis_abs = ctx
            .builder()
            .build_or(wis_unix_abs, wis_win_abs, "wis_abs")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "or failed".into(),
                )
            })?;
        ctx.builder()
            .build_conditional_branch(wis_abs, wabs_fail_bb, wabs_ok_bb)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "branch failed".into(),
                )
            })?;
        ctx.builder().position_at_end(wabs_fail_bb);
        self.emit_puts(ERR_PATH_TRAVERSAL, ctx)?;
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();
        ctx.builder().position_at_end(wabs_ok_bb);
        let content_ptr = self.eval_string_arg(&args[1], ctx)?;
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
        let fp_ptr = self.emit_checked_fopen(path_ptr, mode_ptr, "fopen_w", ctx)?;
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

    /// Emit a file-read failure branch that closes the opened FILE* before aborting.
    fn emit_file_int_check<'a>(
        &mut self,
        lhs: inkwell::values::IntValue<'a>,
        predicate: IntPredicate,
        rhs: inkwell::values::IntValue<'a>,
        msg: &str,
        fp_ptr: inkwell::values::PointerValue<'a>,
        fclose_fn: inkwell::values::FunctionValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<()> {
        let function = ctx
            .builder()
            .get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "no function for file check".into(),
                )
            })?;
        let context = ctx.module().get_context();
        let fail_block = context.append_basic_block(function, "file_fail");
        let ok_block = context.append_basic_block(function, "file_ok");
        let should_fail = ctx
            .builder()
            .build_int_compare(predicate, lhs, rhs, "file_fail_cond")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "file check compare failed".into(),
                )
            })?;
        ctx.builder()
            .build_conditional_branch(should_fail, fail_block, ok_block)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "file check branch failed".into(),
                )
            })?;
        ctx.builder().position_at_end(fail_block);
        ctx.builder()
            .build_call(fclose_fn, &[fp_ptr.into()], "fclose_fail")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "fclose failed".into(),
                )
            })?;
        self.emit_puts(msg, ctx)?;
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();
        ctx.builder().position_at_end(ok_block);
        Ok(())
    }

    /// Allocate memory during file_read and close the open FILE* before aborting on OOM.
    fn emit_file_checked_malloc<'a>(
        &mut self,
        size: inkwell::values::IntValue<'a>,
        name: &str,
        fp_ptr: inkwell::values::PointerValue<'a>,
        fclose_fn: inkwell::values::FunctionValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::PointerValue<'a>> {
        let malloc_fn = ctx.module().get_function("malloc").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "malloc not declared".into(),
            )
        })?;
        let ptr = ctx
            .builder()
            .build_call(malloc_fn, &[size.into()], name)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "malloc failed".into(),
                )
            })?
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "malloc returned void".into(),
                )
            })?
            .into_pointer_value();
        let ptr_i64 = ctx
            .builder()
            .build_ptr_to_int(
                ptr,
                ctx.module().get_context().i64_type(),
                "file_malloc_i64",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        self.emit_file_int_check(
            ptr_i64,
            IntPredicate::EQ,
            ctx.module().get_context().i64_type().const_int(0, false),
            ERR_OUT_OF_MEMORY,
            fp_ptr,
            fclose_fn,
            ctx,
        )?;
        Ok(ptr)
    }
}
