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
            ir.contains("inttoptr"),
            "string index should use inttoptr for string var"
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
            ir.contains("ptrtoint") || ir.contains("inttoptr"),
            "string should be stored as i64 pointer"
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
            ir.contains("fopen") && ir.contains("fread"),
            "file_read should use fopen+fread"
        );
    }
}
