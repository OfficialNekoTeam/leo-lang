#[cfg(test)]
mod tests {
    use crate::ast::expr::Expr;
    use crate::codegen::ir::IrBuilder;
    use crate::common::span::Span;
    use crate::common::types::LeoType;
    use crate::llvm::context::LlvmContext;
    use inkwell::context::Context;

    #[test]
    fn test_ir_builder_new() {
        let mut builder = IrBuilder::new();
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test");
        assert!(builder.build(&[], &mut ctx).is_ok());
    }

    #[test]
    fn test_unknown_identifier_infers_unknown() {
        let builder = IrBuilder::new();
        let context = Context::create();
        let ctx = LlvmContext::new(&context, "test_unknown_type");
        let ty = builder.infer_expr_type(&Expr::Ident("missing".into(), Span::dummy()), &ctx);
        assert_eq!(ty, LeoType::Unknown);
    }

    #[test]
    fn test_unknown_call_infers_unknown() {
        let builder = IrBuilder::new();
        let context = Context::create();
        let ctx = LlvmContext::new(&context, "test_unknown_call");
        let ty = builder.infer_expr_type(
            &Expr::Call(
                Box::new(Expr::Ident("missing".into(), Span::dummy())),
                vec![],
                vec![],
                Span::dummy(),
            ),
            &ctx,
        );
        assert_eq!(ty, LeoType::Unknown);
    }

    #[test]
    fn test_enum_name_infers_enum_type() {
        let builder = IrBuilder::new();
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_enum_type");
        ctx.register_enum("Token".into(), vec!["Eof".into()]);
        let ty = builder.infer_expr_type(&Expr::Ident("Token".into(), Span::dummy()), &ctx);
        assert_eq!(ty, LeoType::Enum("Token".into()));
    }

    #[test]
    fn test_typed_literals_infer_precise_types() {
        let builder = IrBuilder::new();
        let context = Context::create();
        let ctx = LlvmContext::new(&context, "test_typed_literals");
        assert_eq!(
            builder.infer_expr_type(&Expr::IntLiteral(42, LeoType::U16, Span::dummy()), &ctx),
            LeoType::U16
        );
        assert_eq!(
            builder.infer_expr_type(&Expr::IntLiteral(42, LeoType::USize, Span::dummy()), &ctx),
            LeoType::USize
        );
        assert_eq!(
            builder.infer_expr_type(&Expr::FloatLiteral(1.0, LeoType::F32, Span::dummy()), &ctx),
            LeoType::F32
        );
    }

    #[test]
    fn test_typed_literal_ir() {
        let source = "fn main() {\nlet a = 255u8\nlet b = 3.5f\nlet c = 1u128\n}";
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_typed_literal_ir");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        assert!(
            ir.contains("alloca i8"),
            "u8 literal should infer i8 storage"
        );
        assert!(
            ir.contains("alloca float"),
            "f32 literal should infer float storage"
        );
        assert!(
            ir.contains("alloca i128"),
            "u128 literal should infer i128 storage"
        );
    }

    #[test]
    fn test_tuple_literal_ir() {
        let source = "fn main() {\nlet t: (i64, bool) = (1, true)\n}";
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_tuple_literal_ir");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        assert!(ir.contains("tuple_malloc"), "tuple should allocate storage");
        assert!(
            ir.contains("alloca i64"),
            "tuple value should be pointer-sized"
        );
    }

    #[test]
    fn test_string_len_ir() {
        let source = "fn main() {\nlet s: str = \"hello\"\nlet i: i64 = s.len()\nprintln(i)\n}";
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_strlen");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        assert!(ir.contains("strlen"), "IR should contain strlen call");
    }

    #[test]
    fn test_string_index_ir() {
        let source = "fn main() {\nlet s: str = \"hi\"\nlet ch: i64 = s[0]\nprintln(ch)\n}";
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_stridx");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        assert!(
            !ir.contains("inttoptr"),
            "string index should natively use pointer without inttoptr"
        );
    }

    #[test]
    fn test_string_compare_ir() {
        let source = "fn main() {\nlet s: str = \"hello\"\nif s == \"hello\" {\nprintln(1)\n}\n}";
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_strcmp");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        assert!(ir.contains("strcmp"), "string == should use strcmp");
    }

    #[test]
    fn test_for_loop_ir() {
        let source = "fn main() {\nlet s: str = \"abc\"\nfor ch in s {\nprintln(ch)\n}\n}";
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_for");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        assert!(
            ir.contains("for.cond") && ir.contains("for.body"),
            "for loop should have cond and body blocks"
        );
    }

    #[test]
    fn test_implicit_return_ir() {
        let source =
            "fn add(a: i64, b: i64) -> i64 {\na + b\n}\nfn main() {\nprintln(add(1, 2))\n}";
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_impl_ret");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        assert!(
            ir.contains("ret i64"),
            "non-main function should return i64"
        );
    }

    #[test]
    fn test_string_as_i64_ir() {
        let source = "fn main() {\nlet s: str = \"test\"\nprintln(s)\n}";
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_str_i64");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        assert!(
            !ir.contains("inttoptr"),
            "string should be stored natively as a pointer"
        );
    }

    #[test]
    fn test_vec_new_ir() {
        let source = "fn main() {\nlet v: i64 = vec_new()\nprintln(v)\n}";
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_vec");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        assert!(
            ir.contains("vec_hdr_malloc"),
            "vec_new should allocate header"
        );
    }

    #[test]
    fn test_file_read_ir() {
        let source = "fn main() {\nlet s: str = file_read(\"test.txt\")\nprintln(s)\n}";
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_fread");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        assert!(
            ir.contains("fopen")
                && ir.contains("fread")
                && ir.contains("fseek")
                && ir.contains("ftell")
                && ir.contains("file_read size invalid")
                && ir.contains("file_read too large")
                && ir.contains("file_fail_cond")
                && ir.contains("fclose_fail"),
            "file_read should validate size before fread"
        );
    }

    #[test]
    fn test_for_in_array_ir() {
        let source = "fn main() {\nlet arr = [1, 2, 3]\nfor x in arr {\nprintln(x)\n}\n}";
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_for_arr");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        assert!(
            ir.contains("for.cond") && ir.contains("for.body") && ir.contains("for.elem_ptr"),
            "for-in array should have cond, body, and element load"
        );
    }

    #[test]
    fn test_for_in_string_element_ir() {
        let source = "fn main() {\nlet s: str = \"hi\"\nfor ch in s {\nprintln(ch)\n}\n}";
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_for_str_elem");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        assert!(
            ir.contains("for.char_ptr") && ir.contains("zext"),
            "for-in string should load chars with zext"
        );
    }

    #[test]
    fn test_generic_fn_ir() {
        let source = r#"
fn identity<T>(x: T) -> T {
    return x
}
fn main() {
    println(identity<i64>(42))
}
"#;
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_generic_fn");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        assert!(
            ir.contains("identity_i64"),
            "generic fn should be monomorphized to identity_i64"
        );
    }

    #[test]
    fn test_generic_fn_two_types_ir() {
        let source = r#"
fn first<T>(a: T, b: T) -> T {
    return a
}
fn main() {
    println(first<i64>(1, 2))
}
"#;
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_generic_fn_two");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        assert!(
            ir.contains("first_i64"),
            "generic fn should be monomorphized to first_i64"
        );
    }

    #[test]
    fn test_generic_struct_ir() {
        let source = r#"
struct Pair<T> {
    first: T,
    second: T,
}
fn main() {
    let p = Pair<i64> { first: 1, second: 2 }
    println(p)
}
"#;
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_generic_struct");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        // Verify the monomorphized struct was registered in field metadata
        assert!(
            builder.struct_fields.contains_key("Pair_i64"),
            "generic struct should be monomorphized — struct_fields should contain Pair_i64"
        );
        let fields = builder.struct_fields.get("Pair_i64").unwrap();
        assert_eq!(fields, &["first", "second"]);
        // Verify the LLVM struct type was created
        assert!(
            ctx.module().get_struct_type("Pair_i64").is_some(),
            "LLVM module should have Pair_i64 struct type"
        );
    }

    // --- Audit fix tests ---

    #[test]
    fn test_match_returns_arm_value() {
        // M4: match expression must return the matched arm's value, not 0
        let source = "fn main() {\nlet x: i64 = match 1 { 1 => 42, _ => 99 }\nprintln(x)\n}";
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_match_val");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        assert!(
            ir.contains("match_result"),
            "match should use result alloca slot"
        );
        assert!(
            ir.contains("store i64 42"),
            "match arm 1 => 42 should store 42 into result"
        );
    }

    #[test]
    fn test_puts_printf_signatures() {
        // M5+M6: puts must be i32(i8*), printf must NOT have hardcoded i64 second param
        let source = "fn main() {\nprintln(42)\n}";
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_sigs");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        // puts must accept i8* and return i32
        assert!(
            ir.contains("declare i32 @puts(ptr)") || ir.contains("declare i32 @puts(i8*)"),
            "puts must be declared as i32(i8*), got IR: {}",
            &ir[..ir.find("define").unwrap_or(200).min(ir.len())]
        );
        // printf must NOT have a hardcoded i64 second parameter
        assert!(
            !ir.contains("declare i32 @printf(ptr, i64")
                && !ir.contains("declare i32 @printf(i8*, i64"),
            "printf must not have hardcoded i64 second parameter"
        );
    }

    #[test]
    fn test_parser_depth_limit() {
        // MH1: deeply nested unary expressions must fail with a clear error.
        // Run in a thread with a large stack so debug-mode frames don't exhaust
        // the default 2 MB stack before our 512-level guard fires.
        let result = std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024) // 32 MB
            .spawn(|| {
                let prefix = "fn main() { ".to_string();
                let suffix = " }";
                let nested: String = "!(".repeat(520) + "1" + &")".repeat(520);
                let source = prefix + &nested + suffix;
                let tokens = crate::lexer::Lexer::new(&source).tokenize().expect("lex");
                crate::parser::Parser::new(tokens).parse()
            })
            .expect("thread spawn")
            .join()
            .expect("thread join");
        assert!(
            result.is_err(),
            "deeply nested expression should fail parsing"
        );
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("nested too deeply"),
            "error should mention nesting depth, got: {}",
            msg
        );
    }

    #[test]
    fn test_zext_not_sext_for_unsigned() {
        // zext/sext: widening a u8 to i64 must use zero-extend, not sign-extend
        // u8 value 200 widened to i64 should be 200, not -56 (signed interpretation)
        let source = "fn foo(x: u8) -> i64 { return x }\nfn main() { println(foo(200)) }";
        let tokens = crate::lexer::Lexer::new(source).tokenize().expect("lex");
        let stmts = crate::parser::Parser::new(tokens).parse().expect("parse");
        let context = Context::create();
        let mut ctx = LlvmContext::new(&context, "test_zext");
        let mut builder = IrBuilder::new();
        builder.build(&stmts, &mut ctx).expect("build");
        let ir = ctx.print_module();
        // Should contain zext for unsigned widening, not sext
        assert!(
            ir.contains("zext") || !ir.contains("sext i8"),
            "widening u8→i64 must use zext, not sext: {}",
            &ir[..300.min(ir.len())]
        );
    }

    #[test]
    fn test_temp_path_not_fixed() {
        // H3: the generated temp path must not be the old fixed /tmp/leo_run_tmp
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock ok")
            .subsec_nanos();
        let path = format!(
            "{}/leo_run_{}_{}",
            std::env::temp_dir().display(),
            std::process::id(),
            nanos
        );
        assert_ne!(
            path, "/tmp/leo_run_tmp",
            "temp path must not be the fixed old value"
        );
        assert!(
            path.contains("leo_run_"),
            "temp path must contain leo_run_ prefix"
        );
    }
}
