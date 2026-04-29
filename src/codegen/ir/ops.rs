use super::*;
use crate::ast::expr::{BinOp, Expr, UnOp};
use crate::codegen::ir::types::TypedValue;
use crate::common::error::{ErrorCode, ErrorKind, LeoError, LeoResult};
use crate::common::types::LeoType;
use inkwell::values::{BasicValueEnum, FloatValue, IntValue};
use inkwell::AddressSpace;
use inkwell::IntPredicate;

impl IrBuilder {
    pub(super) fn try_string_compare<'a>(
        &mut self,
        op: &BinOp,
        left: &Expr,
        right: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<Option<inkwell::values::IntValue<'a>>> {
        if !matches!(op, BinOp::Eq | BinOp::Ne) {
            return Ok(None);
        }
        let left_is_str = self.expr_is_string(left, ctx);
        let right_is_str = self.expr_is_string(right, ctx);
        if !left_is_str || !right_is_str {
            return Ok(None);
        }
        let left_ptr = self.eval_string_arg(left, ctx)?;
        let right_ptr = self.eval_string_arg(right, ctx)?;
        let strcmp_fn = ctx.module().get_function("strcmp").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strcmp not declared".into(),
            )
        })?;
        let cmp_result = ctx
            .builder()
            .build_call(
                strcmp_fn,
                &[left_ptr.into(), right_ptr.into()],
                "strcmp_call",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strcmp call failed".into(),
                )
            })?;
        let cmp_val = cmp_result
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strcmp returned void".into(),
                )
            })?
            .into_int_value();
        let i64_type = ctx.module().get_context().i64_type();
        let zero = ctx.module().get_context().i32_type().const_int(0, false);
        let predicate = match op {
            BinOp::Eq => IntPredicate::EQ,
            BinOp::Ne => IntPredicate::NE,
            _ => return Ok(None),
        };
        let cmp_bool = ctx
            .builder()
            .build_int_compare(predicate, cmp_val, zero, "str_cmp")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "str compare failed".into(),
                )
            })?;
        let result = ctx
            .builder()
            .build_int_z_extend(cmp_bool, i64_type, "str_cmp_ext")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "zext failed".into(),
                )
            })?;
        // Free heap string temporaries consumed by the comparison
        if self.expr_is_heap_string(left, ctx) {
            self.emit_free_ptr(left_ptr, ctx)?;
        }
        if self.expr_is_heap_string(right, ctx) {
            self.emit_free_ptr(right_ptr, ctx)?;
        }
        Ok(Some(result))
    }

    /// Try string concat via strcat for Add when at least one operand is a string.
    /// Allocates a new buffer, copies left then right via strcpy/strcat.
    pub(super) fn try_string_concat<'a>(
        &mut self,
        op: &BinOp,
        left: &Expr,
        right: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<Option<inkwell::values::IntValue<'a>>> {
        if !matches!(op, BinOp::Add) {
            return Ok(None);
        }
        let left_is_str = self.expr_is_string(left, ctx);
        let right_is_str = self.expr_is_string(right, ctx);
        if !left_is_str && !right_is_str {
            return Ok(None);
        }
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let i8_ptr_type = context.i8_type().ptr_type(AddressSpace::default());
        let strcpy_fn = ctx.module().get_function("strcpy").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strcpy not declared".into(),
            )
        })?;
        let strcat_fn = ctx.module().get_function("strcat").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strcat not declared".into(),
            )
        })?;
        let left_ptr = self.eval_string_arg(left, ctx)?;
        let right_ptr = self.eval_string_arg(right, ctx)?;
        let strlen_fn = ctx.module().get_function("strlen").ok_or_else(|| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "strlen not declared".into(),
            )
        })?;
        let left_len_call = ctx
            .builder()
            .build_call(strlen_fn, &[left_ptr.into()], "left_len")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen left failed".into(),
                )
            })?;
        let left_len = left_len_call
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen void".into(),
                )
            })?
            .into_int_value();
        let right_len_call = ctx
            .builder()
            .build_call(strlen_fn, &[right_ptr.into()], "right_len")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen right failed".into(),
                )
            })?;
        let right_len = right_len_call
            .try_as_basic_value()
            .left()
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strlen void".into(),
                )
            })?
            .into_int_value();
        let one = i64_type.const_int(1, false);
        let max_sum = i64_type.const_int((i64::MAX - 1) as u64, false);
        let sum_len_raw = ctx
            .builder()
            .build_int_add(left_len, right_len, "sum_len")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "add failed".into(),
                )
            })?;
        let sum_overflow = ctx
            .builder()
            .build_int_compare(IntPredicate::SGT, sum_len_raw, max_sum, "concat2_overflow")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "overflow cmp failed".into(),
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
        let overflow_fail = context.append_basic_block(func, "concat2_overflow_fail");
        let overflow_ok = context.append_basic_block(func, "concat2_overflow_ok");
        ctx.builder()
            .build_conditional_branch(sum_overflow, overflow_fail, overflow_ok)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "overflow branch failed".into(),
                )
            })?;
        ctx.builder().position_at_end(overflow_fail);
        self.emit_puts("runtime error: string concat length overflow\n", ctx)?;
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();
        ctx.builder().position_at_end(overflow_ok);
        let buf_size = ctx
            .builder()
            .build_int_add(sum_len_raw, one, "total_size")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "add failed".into(),
                )
            })?;
        let buf_raw = self.emit_checked_malloc(buf_size, "concat_buf", ctx)?;
        let buf_ptr = ctx
            .builder()
            .build_pointer_cast(buf_raw, i8_ptr_type, "concat_i8")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cast failed".into(),
                )
            })?;
        // strcpy(buf, left) then strcat(buf, right)
        ctx.builder()
            .build_call(
                strcpy_fn,
                &[buf_ptr.into(), left_ptr.into()],
                "strcpy_concat",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strcpy failed".into(),
                )
            })?;
        ctx.builder()
            .build_call(
                strcat_fn,
                &[buf_ptr.into(), right_ptr.into()],
                "strcat_concat",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "strcat failed".into(),
                )
            })?;
        // Free consumed temporary string operands.
        // Only frees expressions that provably produced a fresh heap allocation
        // (known builtins or binary string +). Variables are never freed here.
        if self.expr_is_heap_string(left, ctx) {
            self.emit_free_ptr(left_ptr, ctx)?;
        }
        if self.expr_is_heap_string(right, ctx) {
            self.emit_free_ptr(right_ptr, ctx)?;
        }
        let result = ctx
            .builder()
            .build_ptr_to_int(buf_ptr, i64_type, "concat_result")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "ptr_to_int failed".into(),
                )
            })?;
        Ok(Some(result))
    }
    pub(super) fn emit_binop<'a>(
        &mut self,
        op: &BinOp,
        lv: TypedValue<'a>,
        rv: TypedValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<TypedValue<'a>> {
        let _i64_type = ctx.module().get_context().i64_type();
        let _i1_type = ctx.module().get_context().bool_type();
        let _f64_type = ctx.module().get_context().f64_type();

        // Handle float arithmetic
        if lv.ty.is_float() || rv.ty.is_float() {
            let result_ty = if lv.ty == LeoType::F64 || rv.ty == LeoType::F64 {
                LeoType::F64
            } else if lv.ty == LeoType::F32 || rv.ty == LeoType::F32 {
                LeoType::F32
            } else {
                LeoType::F64
            };
            let lf = self.coerce_to_float(lv, &result_ty, ctx)?;
            let rf = self.coerce_to_float(rv, &result_ty, ctx)?;
            return self.emit_float_binop_typed(op, lf, rf, &result_ty, ctx);
        }

        let l_ty = lv.ty.clone();
        let l_val = self.coerce_to_int(lv, ctx)?;
        let r_val = self.coerce_to_int(rv, ctx)?;

        match op {
            BinOp::Add => ctx
                .builder()
                .build_int_add(l_val, r_val, "add")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), l_ty.clone())),
            BinOp::Sub => ctx
                .builder()
                .build_int_sub(l_val, r_val, "sub")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), l_ty.clone())),
            BinOp::Mul => ctx
                .builder()
                .build_int_mul(l_val, r_val, "mul")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), l_ty.clone())),
            BinOp::Div => {
                self.emit_div_zero_check(r_val, "runtime error: division by zero\n", ctx)?;
                self.emit_div_overflow_check(l_val, r_val, ctx)?;
                ctx.builder()
                    .build_int_signed_div(l_val, r_val, "div")
                    .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), l_ty.clone()))
            }
            BinOp::Mod => {
                self.emit_div_zero_check(r_val, "runtime error: division by zero\n", ctx)?;
                self.emit_div_overflow_check(l_val, r_val, ctx)?;
                ctx.builder()
                    .build_int_signed_rem(l_val, r_val, "rem")
                    .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), l_ty.clone()))
            }
            BinOp::Eq => ctx
                .builder()
                .build_int_compare(IntPredicate::EQ, l_val, r_val, "eq")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), LeoType::Bool)),
            BinOp::Ne => ctx
                .builder()
                .build_int_compare(IntPredicate::NE, l_val, r_val, "ne")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), LeoType::Bool)),
            BinOp::Lt => ctx
                .builder()
                .build_int_compare(IntPredicate::SLT, l_val, r_val, "lt")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), LeoType::Bool)),
            BinOp::Le => ctx
                .builder()
                .build_int_compare(IntPredicate::SLE, l_val, r_val, "le")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), LeoType::Bool)),
            BinOp::Gt => ctx
                .builder()
                .build_int_compare(IntPredicate::SGT, l_val, r_val, "gt")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), LeoType::Bool)),
            BinOp::Ge => ctx
                .builder()
                .build_int_compare(IntPredicate::SGE, l_val, r_val, "ge")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), LeoType::Bool)),
            BinOp::And | BinOp::BitAnd => ctx
                .builder()
                .build_and(l_val, r_val, "and")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), l_ty.clone())),
            BinOp::Or | BinOp::BitOr => ctx
                .builder()
                .build_or(l_val, r_val, "or")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), l_ty.clone())),
            BinOp::Shr => {
                self.emit_shift_bounds_check(r_val, ctx)?;
                ctx.builder()
                    .build_right_shift(l_val, r_val, true, "shr")
                    .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), l_ty.clone()))
            }
            BinOp::Shl => {
                self.emit_shift_bounds_check(r_val, ctx)?;
                ctx.builder()
                    .build_left_shift(l_val, r_val, "shl")
                    .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), l_ty.clone()))
            }
        }
        .map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("{:?} failed", op),
            )
        })
    }

    fn coerce_to_float<'a>(
        &mut self,
        tv: TypedValue<'a>,
        target_ty: &LeoType,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::FloatValue<'a>> {
        let context = ctx.module().get_context();
        let float_type = if *target_ty == LeoType::F32 {
            context.f32_type()
        } else {
            context.f64_type()
        };
        match tv.value {
            BasicValueEnum::FloatValue(fv) => {
                if fv.get_type() == float_type {
                    Ok(fv)
                } else if *target_ty == LeoType::F32 {
                    ctx.builder()
                        .build_float_trunc(fv, float_type, "ftrunc")
                        .map_err(|_| {
                            LeoError::new(
                                ErrorKind::Syntax,
                                ErrorCode::CodegenLLVMError,
                                "float trunc failed".into(),
                            )
                        })
                } else {
                    ctx.builder()
                        .build_float_ext(fv, float_type, "fext")
                        .map_err(|_| {
                            LeoError::new(
                                ErrorKind::Syntax,
                                ErrorCode::CodegenLLVMError,
                                "float ext failed".into(),
                            )
                        })
                }
            }
            BasicValueEnum::IntValue(iv) => ctx
                .builder()
                .build_signed_int_to_float(iv, float_type, "sitofp")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "float conversion failed".into(),
                    )
                }),
            _ => Err(LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "cannot coerce to float".into(),
            )),
        }
    }

    pub(super) fn emit_float_binop_typed<'a>(
        &mut self,
        op: &BinOp,
        lv: inkwell::values::FloatValue<'a>,
        rv: inkwell::values::FloatValue<'a>,
        result_ty: &LeoType,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<TypedValue<'a>> {
        let _i1_type = ctx.module().get_context().bool_type();
        match op {
            BinOp::Add => ctx
                .builder()
                .build_float_add(lv, rv, "fadd")
                .map(|v: FloatValue| {
                    TypedValue::new(BasicValueEnum::FloatValue(v), result_ty.clone())
                }),
            BinOp::Sub => ctx
                .builder()
                .build_float_sub(lv, rv, "fsub")
                .map(|v: FloatValue| {
                    TypedValue::new(BasicValueEnum::FloatValue(v), result_ty.clone())
                }),
            BinOp::Mul => ctx
                .builder()
                .build_float_mul(lv, rv, "fmul")
                .map(|v: FloatValue| {
                    TypedValue::new(BasicValueEnum::FloatValue(v), result_ty.clone())
                }),
            BinOp::Div => ctx
                .builder()
                .build_float_div(lv, rv, "fdiv")
                .map(|v: FloatValue| {
                    TypedValue::new(BasicValueEnum::FloatValue(v), result_ty.clone())
                }),
            BinOp::Mod => ctx
                .builder()
                .build_float_rem(lv, rv, "frem")
                .map(|v: FloatValue| {
                    TypedValue::new(BasicValueEnum::FloatValue(v), result_ty.clone())
                }),
            BinOp::Eq => ctx
                .builder()
                .build_float_compare(inkwell::FloatPredicate::OEQ, lv, rv, "feq")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), LeoType::Bool)),
            BinOp::Ne => ctx
                .builder()
                .build_float_compare(inkwell::FloatPredicate::ONE, lv, rv, "fne")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), LeoType::Bool)),
            BinOp::Lt => ctx
                .builder()
                .build_float_compare(inkwell::FloatPredicate::OLT, lv, rv, "flt")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), LeoType::Bool)),
            BinOp::Le => ctx
                .builder()
                .build_float_compare(inkwell::FloatPredicate::OLE, lv, rv, "fle")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), LeoType::Bool)),
            BinOp::Gt => ctx
                .builder()
                .build_float_compare(inkwell::FloatPredicate::OGT, lv, rv, "fgt")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), LeoType::Bool)),
            BinOp::Ge => ctx
                .builder()
                .build_float_compare(inkwell::FloatPredicate::OGE, lv, rv, "fge")
                .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), LeoType::Bool)),
            _ => {
                return Err(LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    format!("unsupported float operator: {:?}", op),
                ))
            }
        }
        .map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("float {:?} failed", op),
            )
        })
    }
    pub(super) fn eval_float_binop<'a>(
        &mut self,
        op: &BinOp,
        left: &Expr,
        right: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<TypedValue<'a>> {
        let lv = self.eval_expr(left, ctx)?;
        let rv = self.eval_expr(right, ctx)?;
        let result_ty = if lv.ty == LeoType::F64 || rv.ty == LeoType::F64 {
            LeoType::F64
        } else if lv.ty == LeoType::F32 || rv.ty == LeoType::F32 {
            LeoType::F32
        } else {
            LeoType::F64
        };
        let lf = self.coerce_to_float(lv, &result_ty, ctx)?;
        let rf = self.coerce_to_float(rv, &result_ty, ctx)?;
        self.emit_float_binop_typed(op, lf, rf, &result_ty, ctx)
    }

    /// Emit unary operation (negate, bitwise not)
    /// Short-circuit evaluation for && and ||
    pub(super) fn eval_short_circuit<'a>(
        &mut self,
        op: &BinOp,
        left: &Expr,
        right: &Expr,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<TypedValue<'a>> {
        let function = ctx
            .builder()
            .get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "no function for short-circuit".into(),
                )
            })?;
        let context = ctx.module().get_context();
        let i64_type = context.i64_type();
        let zero = i64_type.const_int(0, false);
        let rhs_block = context.append_basic_block(function, "sc.rhs");
        let merge_block = context.append_basic_block(function, "sc.merge");
        let result_ptr = ctx
            .builder()
            .build_alloca(i64_type, "sc_result")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "alloca sc_result failed".into(),
                )
            })?;
        let lv_typed = self.eval_expr(left, ctx)?;
        let lv = self.coerce_to_int(lv_typed, ctx)?;
        let lv_nz = ctx
            .builder()
            .build_int_compare(IntPredicate::NE, lv, zero, "sc.lv_nz")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "sc cmp failed".into(),
                )
            })?;
        match op {
            BinOp::And => {
                ctx.builder().build_store(result_ptr, zero).map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "store sc 0".into(),
                    )
                })?;
                ctx.builder()
                    .build_conditional_branch(lv_nz, rhs_block, merge_block)
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "sc br".into(),
                        )
                    })?;
            }
            BinOp::Or => {
                let one = i64_type.const_int(1, false);
                ctx.builder().build_store(result_ptr, one).map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "store sc 1".into(),
                    )
                })?;
                ctx.builder()
                    .build_conditional_branch(lv_nz, merge_block, rhs_block)
                    .map_err(|_| {
                        LeoError::new(
                            ErrorKind::Syntax,
                            ErrorCode::CodegenLLVMError,
                            "sc br".into(),
                        )
                    })?;
            }
            _ => {}
        }
        ctx.builder().position_at_end(rhs_block);
        let rv_typed = self.eval_expr(right, ctx)?;
        let rv = self.coerce_to_int(rv_typed, ctx)?;
        let rv_bool = ctx
            .builder()
            .build_int_compare(IntPredicate::NE, rv, zero, "sc.rv_nz")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "sc rv cmp".into(),
                )
            })?;
        let rv_val = ctx
            .builder()
            .build_int_z_extend(rv_bool, i64_type, "sc_rv_bool")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "sc zext".into(),
                )
            })?;
        ctx.builder().build_store(result_ptr, rv_val).map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "store sc rv".into(),
            )
        })?;
        ctx.builder()
            .build_unconditional_branch(merge_block)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "sc br merge".into(),
                )
            })?;
        ctx.builder().position_at_end(merge_block);
        let result = ctx
            .builder()
            .build_load(result_ptr, "sc_val")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "load sc".into(),
                )
            })?;
        Ok(TypedValue::new(result, LeoType::Bool))
    }

    pub(super) fn emit_unop<'a>(
        &mut self,
        op: &UnOp,
        val: TypedValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<TypedValue<'a>> {
        let _i64_type = ctx.module().get_context().i64_type();
        let _i1_type = ctx.module().get_context().bool_type();
        let int_val = val.value.into_int_value();
        match op {
            UnOp::Neg | UnOp::Minus => {
                let zero = int_val.get_type().const_int(0, false);
                ctx.builder()
                    .build_int_sub(zero, int_val, "neg")
                    .map(|v: IntValue| TypedValue::new(BasicValueEnum::IntValue(v), val.ty.clone()))
            }
            UnOp::Not => {
                if val.ty == LeoType::Bool {
                    ctx.builder().build_not(int_val, "not").map(|v: IntValue| {
                        TypedValue::new(BasicValueEnum::IntValue(v), LeoType::Bool)
                    })
                } else {
                    let ones = int_val.get_type().const_all_ones();
                    ctx.builder()
                        .build_xor(int_val, ones, "not")
                        .map(|v: IntValue| {
                            TypedValue::new(BasicValueEnum::IntValue(v), val.ty.clone())
                        })
                }
            }
            _ => Ok(val),
        }
        .map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                format!("{:?} failed", op),
            )
        })
    }

    fn coerce_to_int<'a>(
        &mut self,
        tv: TypedValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<inkwell::values::IntValue<'a>> {
        let i64_type = ctx.module().get_context().i64_type();
        match tv.value {
            BasicValueEnum::IntValue(iv) => {
                let bw = iv.get_type().get_bit_width();
                if bw < 64 {
                    ctx.builder()
                        .build_int_z_extend(iv, i64_type, "zext")
                        .map_err(|_| {
                            LeoError::new(
                                ErrorKind::Syntax,
                                ErrorCode::CodegenLLVMError,
                                "zext fail".into(),
                            )
                        })
                } else if bw > 64 {
                    ctx.builder()
                        .build_int_truncate(iv, i64_type, "trunc")
                        .map_err(|_| {
                            LeoError::new(
                                ErrorKind::Syntax,
                                ErrorCode::CodegenLLVMError,
                                "trunc fail".into(),
                            )
                        })
                } else {
                    Ok(iv)
                }
            }
            BasicValueEnum::FloatValue(fv) => ctx
                .builder()
                .build_float_to_signed_int(fv, i64_type, "fptosi")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "int conversion failed".into(),
                    )
                }),
            BasicValueEnum::PointerValue(pv) => ctx
                .builder()
                .build_ptr_to_int(pv, i64_type, "ptr2int")
                .map_err(|_| {
                    LeoError::new(
                        ErrorKind::Syntax,
                        ErrorCode::CodegenLLVMError,
                        "ptr to int conversion failed".into(),
                    )
                }),
            _ => Err(LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "cannot coerce to int".into(),
            )),
        }
    }

    fn emit_shift_bounds_check<'a>(
        &mut self,
        r_val: inkwell::values::IntValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<()> {
        let i64_type = ctx.module().get_context().i64_type();
        let sixty_four = i64_type.const_int(64, false);
        let zero = i64_type.const_int(0, false);
        let is_ge = ctx
            .builder()
            .build_int_compare(
                inkwell::IntPredicate::SGE,
                r_val,
                sixty_four,
                "shl_bound_ge",
            )
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cmp fail".into(),
                )
            })?;
        let is_lt = ctx
            .builder()
            .build_int_compare(inkwell::IntPredicate::SLT, r_val, zero, "shl_bound_lt")
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "cmp fail".into(),
                )
            })?;
        let is_ub = ctx.builder().build_or(is_ge, is_lt, "is_ub").map_err(|_| {
            LeoError::new(
                ErrorKind::Syntax,
                ErrorCode::CodegenLLVMError,
                "or fail".into(),
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
                    "no parent fn for shift bounds".into(),
                )
            })?;
        let fail_bb = ctx
            .module()
            .get_context()
            .append_basic_block(func, "shift_fail");
        let ok_bb = ctx
            .module()
            .get_context()
            .append_basic_block(func, "shift_ok");
        ctx.builder()
            .build_conditional_branch(is_ub, fail_bb, ok_bb)
            .map_err(|_| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "branch".into(),
                )
            })?;

        ctx.builder().position_at_end(fail_bb);
        self.emit_puts("runtime error: shift amount out of bounds\n", ctx)?;
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();

        ctx.builder().position_at_end(ok_bb);
        Ok(())
    }

    fn emit_div_overflow_check<'a>(
        &mut self,
        l_val: inkwell::values::IntValue<'a>,
        r_val: inkwell::values::IntValue<'a>,
        ctx: &mut LlvmContext<'a>,
    ) -> LeoResult<()> {
        let i64_type = ctx.module().get_context().i64_type();
        let min_val = i64_type.const_int(0x8000000000000000, false);
        let neg_one = i64_type.const_int(u64::MAX, false);

        let is_min = ctx
            .builder()
            .build_int_compare(inkwell::IntPredicate::EQ, l_val, min_val, "div_min")
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "cmp".into())
            })?;
        let is_neg_one = ctx
            .builder()
            .build_int_compare(inkwell::IntPredicate::EQ, r_val, neg_one, "div_neg_one")
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "cmp".into())
            })?;
        let is_ub = ctx
            .builder()
            .build_and(is_min, is_neg_one, "div_of_ub")
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "and".into())
            })?;

        let func = ctx
            .builder()
            .get_insert_block()
            .and_then(|bb| bb.get_parent())
            .ok_or_else(|| {
                LeoError::new(
                    ErrorKind::Syntax,
                    ErrorCode::CodegenLLVMError,
                    "no parent fn for div overflow".into(),
                )
            })?;
        let fail_bb = ctx
            .module()
            .get_context()
            .append_basic_block(func, "div_of_fail");
        let ok_bb = ctx
            .module()
            .get_context()
            .append_basic_block(func, "div_of_ok");
        ctx.builder()
            .build_conditional_branch(is_ub, fail_bb, ok_bb)
            .map_err(|_| {
                LeoError::new(ErrorKind::Syntax, ErrorCode::CodegenLLVMError, "br".into())
            })?;

        ctx.builder().position_at_end(fail_bb);
        self.emit_puts("runtime error: division overflow\n", ctx)?;
        self.emit_abort(ctx)?;
        let _ = ctx.builder().build_unreachable();

        ctx.builder().position_at_end(ok_bb);
        Ok(())
    }
}
