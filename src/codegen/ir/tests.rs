#[cfg(test)]
mod tests {
    use crate::codegen::ir::IrBuilder;
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
                && ir.contains("ftell"),
            "file_read should use fopen+fseek+ftell+fread"
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
}
