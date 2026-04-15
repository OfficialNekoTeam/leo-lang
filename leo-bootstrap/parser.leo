struct Parser {
    src: str,
    pos: i64,
    len: i64,
    tok_kind: i64,
    tok_start: i64,
    tok_end: i64,
    indent: i64,
    has_err: i64,
}

impl Parser {
    fn peek(self: Parser) -> i64 {
        if self.pos >= self.len { return 0 }
        return self.src[self.pos]
    }

    fn advance_ch(self: Parser) -> i64 {
        let ch: i64 = self.src[self.pos]
        self.pos = self.pos + 1
        return ch
    }

    fn skip_ws(self: Parser) {
        while self.pos < self.len {
            let ch: i64 = self.peek()
            if ch == 32 || ch == 10 || ch == 9 || ch == 13 {
                self.pos = self.pos + 1
            } else {
                return
            }
        }
    }

    fn text(self: Parser) -> str {
        return str_slice(self.src, self.tok_start, self.tok_end)
    }

    fn keyword_kind(self: Parser) -> i64 {
        let t: str = self.text()
        if t == "fn" { return 40 }
        if t == "let" { return 41 }
        if t == "if" { return 42 }
        if t == "else" { return 43 }
        if t == "while" { return 44 }
        if t == "for" { return 45 }
        if t == "return" { return 46 }
        if t == "struct" { return 47 }
        if t == "enum" { return 48 }
        if t == "impl" { return 49 }
        if t == "true" { return 50 }
        if t == "false" { return 51 }
        if t == "pub" { return 52 }
        if t == "mut" { return 53 }
        if t == "const" { return 54 }
        if t == "break" { return 55 }
        if t == "continue" { return 56 }
        if t == "in" { return 58 }
        if t == "self" { return 59 }
        return 0
    }

    fn advance(self: Parser) {
        self.skip_ws()
        self.tok_start = self.pos
        if self.pos >= self.len {
            self.tok_kind = 0
            self.tok_end = self.pos
            return
        }
        let ch: i64 = self.peek()
        if ch == 47 {
            self.advance_ch()
            if self.pos < self.len && self.peek() == 47 {
                self.advance_ch()
                while self.pos < self.len {
                    if self.advance_ch() == 10 {
                        self.advance()
                        return
                    }
                }
                self.tok_kind = 0
                self.tok_end = self.pos
                return
            }
            self.tok_kind = 5
            self.tok_end = self.pos
            return
        }
        if ch == 43 { self.advance_ch(); self.tok_kind = 2; self.tok_end = self.pos; return }
        if ch == 45 {
            self.advance_ch()
            if self.pos < self.len && self.peek() == 62 {
                self.advance_ch()
                self.tok_kind = 14
            } else {
                self.tok_kind = 3
            }
            self.tok_end = self.pos
            return
        }
        if ch == 42 { self.advance_ch(); self.tok_kind = 4; self.tok_end = self.pos; return }
        if ch == 40 { self.advance_ch(); self.tok_kind = 7; self.tok_end = self.pos; return }
        if ch == 41 { self.advance_ch(); self.tok_kind = 8; self.tok_end = self.pos; return }
        if ch == 123 { self.advance_ch(); self.tok_kind = 9; self.tok_end = self.pos; return }
        if ch == 125 { self.advance_ch(); self.tok_kind = 10; self.tok_end = self.pos; return }
        if ch == 58 {
            self.advance_ch()
            if self.pos < self.len && self.peek() == 58 {
                self.advance_ch()
                self.tok_kind = 15
            } else {
                self.tok_kind = 11
            }
            self.tok_end = self.pos
            return
        }
        if ch == 44 { self.advance_ch(); self.tok_kind = 12; self.tok_end = self.pos; return }
        if ch == 59 { self.advance_ch(); self.tok_kind = 13; self.tok_end = self.pos; return }
        if ch == 33 {
            self.advance_ch()
            if self.pos < self.len && self.peek() == 61 {
                self.advance_ch()
                self.tok_kind = 22
            } else {
                self.tok_kind = 21
            }
            self.tok_end = self.pos
            return
        }
        if ch == 61 {
            self.advance_ch()
            if self.pos < self.len && self.peek() == 61 {
                self.advance_ch()
                self.tok_kind = 16
            } else {
                if self.pos < self.len && self.peek() == 62 {
                    self.advance_ch()
                    self.tok_kind = 14
                } else {
                    self.tok_kind = 6
                }
            }
            self.tok_end = self.pos
            return
        }
        if ch == 60 {
            self.advance_ch()
            if self.pos < self.len && self.peek() == 61 {
                self.advance_ch()
                self.tok_kind = 17
            } else {
                self.tok_kind = 18
            }
            self.tok_end = self.pos
            return
        }
        if ch == 62 {
            self.advance_ch()
            if self.pos < self.len && self.peek() == 61 {
                self.advance_ch()
                self.tok_kind = 19
            } else {
                self.tok_kind = 20
            }
            self.tok_end = self.pos
            return
        }
        if ch == 38 {
            self.advance_ch()
            if self.pos < self.len && self.peek() == 38 {
                self.advance_ch()
                self.tok_kind = 23
            } else {
                self.tok_kind = 24
            }
            self.tok_end = self.pos
            return
        }
        if ch == 124 {
            self.advance_ch()
            if self.pos < self.len && self.peek() == 124 {
                self.advance_ch()
                self.tok_kind = 25
            } else {
                self.tok_kind = 26
            }
            self.tok_end = self.pos
            return
        }
        if ch == 37 { self.advance_ch(); self.tok_kind = 27; self.tok_end = self.pos; return }
        if ch == 91 { self.advance_ch(); self.tok_kind = 31; self.tok_end = self.pos; return }
        if ch == 93 { self.advance_ch(); self.tok_kind = 32; self.tok_end = self.pos; return }
        if ch == 46 {
            self.advance_ch()
            if self.pos < self.len && self.peek() == 46 {
                self.advance_ch()
                self.tok_kind = 29
            } else {
                self.tok_kind = 30
            }
            self.tok_end = self.pos
            return
        }
        if is_digit(ch) {
            self.advance_ch()
            while self.pos < self.len && is_digit(self.peek()) {
                self.advance_ch()
            }
            self.tok_kind = 1
            self.tok_end = self.pos
            return
        }
        if is_alpha(ch) || ch == 95 {
            self.advance_ch()
            while self.pos < self.len && (is_alnum(self.peek()) || self.peek() == 95) {
                self.advance_ch()
            }
            self.tok_end = self.pos
            self.tok_kind = self.keyword_kind()
            if self.tok_kind == 0 {
                self.tok_kind = 100
            }
            return
        }
        if ch == 34 {
            self.advance_ch()
            while self.pos < self.len {
                let c: i64 = self.advance_ch()
                if c == 34 {
                    self.tok_kind = 101
                    self.tok_end = self.pos
                    return
                }
                if c == 92 {
                    if self.pos < self.len { self.advance_ch() }
                }
            }
            self.tok_kind = 101
            self.tok_end = self.pos
            return
        }
        self.advance_ch()
        self.tok_kind = 0
        self.tok_end = self.pos
    }

    fn expect(self: Parser, kind: i64) -> i64 {
        if self.tok_kind == kind {
            self.advance()
            return 1
        }
        self.has_err = 1
        return 0
    }

    fn match_tok(self: Parser, kind: i64) -> i64 {
        if self.tok_kind == kind {
            self.advance()
            return 1
        }
        return 0
    }

    fn skip_semi(self: Parser) {
        if self.tok_kind == 13 { self.advance() }
    }

    fn ind(self: Parser) {
        let i: i64 = 0
        while i < self.indent {
            print("  ")
            i = i + 1
        }
    }

    fn parse_program(self: Parser) {
        self.advance()
        while self.tok_kind != 0 {
            self.parse_top()
        }
    }

    fn parse_top(self: Parser) {
        let k: i64 = self.tok_kind
        if k == 40 { self.parse_fn() }
        else { if k == 47 { self.parse_struct_decl() }
        else { if k == 48 { self.parse_enum_decl() }
        else { if k == 49 { self.parse_impl() }
        else { if k == 54 { self.parse_const() }
        else { if k == 52 { self.parse_import() }
        else { if k == 53 {
            self.advance()
            self.parse_top()
        } else {
            self.advance()
        } } } } } } }
    }

    fn parse_fn(self: Parser) {
        self.advance()
        let name: str = self.text()
        self.advance()
        self.ind()
        print("FnDecl(")
        print(name)
        print(")(")
        self.expect(7)
        let first: i64 = 1
        while self.tok_kind != 8 && self.tok_kind != 0 {
            if first == 0 { print(", ") }
            let pn: str = self.text()
            self.advance()
            self.expect(11)
            let pt: str = self.text()
            self.advance()
            print(pn)
            print(": ")
            print(pt)
            first = 0
            if self.tok_kind == 12 { self.advance() }
        }
        self.expect(8)
        if self.tok_kind == 14 { self.advance() }
        if self.tok_kind == 100 {
            let rt: str = self.text()
            self.advance()
            print(") -> ")
            print(rt)
        } else {
            print(")")
        }
        println("")
        self.parse_block()
    }

    fn parse_block(self: Parser) {
        self.expect(9)
        self.indent = self.indent + 1
        while self.tok_kind != 10 && self.tok_kind != 0 {
            self.parse_stmt()
        }
        self.indent = self.indent - 1
        self.expect(10)
    }

    fn parse_stmt(self: Parser) {
        let k: i64 = self.tok_kind
        if k == 41 { self.parse_let() }
        else { if k == 46 { self.parse_return() }
        else { if k == 42 { self.parse_if_stmt() }
        else { if k == 44 { self.parse_while() }
        else { if k == 45 { self.parse_for() }
        else { if k == 55 {
            self.advance()
            self.ind()
            println("Break")
            self.skip_semi()
        } else { if k == 56 {
            self.advance()
            self.ind()
            println("Continue")
            self.skip_semi()
        } else {
            self.parse_expr_stmt()
        } } } } } } }
    }

    fn parse_let(self: Parser) {
        self.advance()
        let name: str = self.text()
        self.advance()
        let ty: str = ""
        if self.match_tok(11) == 1 {
            ty = self.text()
            self.advance()
        }
        self.ind()
        print("Let(")
        print(name)
        if ty != "" {
            print(": ")
            print(ty)
        }
        if self.match_tok(6) == 1 {
            print(" = ")
            self.parse_expr(0)
        }
        println(")")
        self.skip_semi()
    }

    fn parse_return(self: Parser) {
        self.advance()
        self.ind()
        print("Return")
        if self.tok_kind != 10 && self.tok_kind != 13 && self.tok_kind != 0 {
            print(" ")
            self.parse_expr(0)
        }
        println("")
        self.skip_semi()
    }

    fn parse_if_stmt(self: Parser) {
        self.advance()
        self.ind()
        print("If(")
        self.parse_expr(0)
        println(")")
        self.parse_block()
        while self.tok_kind == 43 {
            self.advance()
            if self.tok_kind == 42 {
                self.advance()
                self.ind()
                print("ElseIf(")
                self.parse_expr(0)
                println(")")
                self.parse_block()
            } else {
                self.ind()
                println("Else")
                self.parse_block()
                return
            }
        }
    }

    fn parse_while(self: Parser) {
        self.advance()
        self.ind()
        print("While(")
        self.parse_expr(0)
        println(")")
        self.parse_block()
    }

    fn parse_for(self: Parser) {
        self.advance()
        let var: str = self.text()
        self.advance()
        if self.tok_kind == 58 { self.advance() }
        self.ind()
        print("For(")
        print(var)
        print(" in ")
        self.parse_expr(0)
        println(")")
        self.parse_block()
    }

    fn parse_expr_stmt(self: Parser) {
        let lhs_text: str = self.text()
        self.parse_expr(0)
        if self.tok_kind == 6 {
            self.advance()
            self.ind()
            print(lhs_text)
            print(" = ")
            self.parse_expr(0)
            println("")
            self.skip_semi()
        } else {
            println("")
            self.skip_semi()
        }
    }

    fn parse_expr(self: Parser, min_prec: i64) {
        self.parse_unary()
        while self.tok_kind != 0 {
            let prec: i64 = self.binop_prec()
            if prec < min_prec { return }
            let op: str = self.text()
            self.advance()
            self.parse_unary()
            print(" ")
            print(op)
        }
    }

    fn parse_unary(self: Parser) {
        if self.tok_kind == 3 {
            self.advance()
            self.parse_unary()
            print(" (-)")
            return
        }
        if self.tok_kind == 21 {
            self.advance()
            self.parse_unary()
            print(" (!)")
            return
        }
        self.parse_postfix()
    }

    fn parse_postfix(self: Parser) {
        self.parse_primary()
        while self.tok_kind != 0 {
            if self.tok_kind == 7 {
                self.advance()
                let argc: i64 = 0
                while self.tok_kind != 8 && self.tok_kind != 0 {
                    if argc > 0 {
                        if self.tok_kind == 12 { self.advance() }
                    }
                    self.parse_expr(0)
                    argc = argc + 1
                }
                self.expect(8)
                print(" [Call(")
                print(argc)
                print(")]")

            } else { if self.tok_kind == 30 {
                self.advance()
                let field: str = self.text()
                self.advance()
                print(" [.")
                print(field)
                print("]")
            } else { if self.tok_kind == 31 {
                self.advance()
                self.parse_expr(0)
                self.expect(32)
                print(" [Index]")
            } else {
                return
            } } }
        }
    }

    fn parse_primary(self: Parser) {
        let k: i64 = self.tok_kind
        if k == 1 {
            let n: str = self.text()
            print(" [Num:")
            print(n)
            print("]")
            self.advance()
            return
        }
        if k == 101 {
            print(" [Str]")
            self.advance()
            return
        }
        if k == 50 {
            print(" [Bool:true]")
            self.advance()
            return
        }
        if k == 51 {
            print(" [Bool:false]")
            self.advance()
            return
        }
        if k == 59 {
            print(" [Self]")
            self.advance()
            return
        }
        if k == 100 {
            let name: str = self.text()
            self.advance()
            if self.tok_kind == 9 {
                self.parse_struct_init(name)
            } else {
                print(" [Ident:")
                print(name)
                print("]")
            }
            return
        }
        if k == 7 {
            self.advance()
            self.parse_expr(0)
            self.expect(8)
            return
        }
        if k == 31 {
            self.advance()
            if self.tok_kind == 32 {
                self.advance()
                print(" [Array:empty]")
                return
            }
            self.parse_expr(0)
            if self.tok_kind == 13 {
                self.advance()
                self.parse_expr(0)
                self.expect(32)
                print(" [ArrayRepeat]")
                return
            }
            while self.tok_kind == 12 {
                self.advance()
                self.parse_expr(0)
            }
            self.expect(32)
            print(" [Array]")
            return
        }
        self.advance()
    }

    fn parse_struct_init(self: Parser, name: str) {
        self.expect(9)
        print(" [StructInit:")
        print(name)
        print("]")
        while self.tok_kind != 10 && self.tok_kind != 0 {
            self.advance()
            self.expect(11)
            self.parse_expr(0)
            if self.tok_kind == 12 { self.advance() }
        }
        self.expect(10)
    }

    fn binop_prec(self: Parser) -> i64 {
        let k: i64 = self.tok_kind
        if k == 25 { return 1 }
        if k == 23 { return 2 }
        if k == 16 || k == 22 { return 3 }
        if k == 18 || k == 17 || k == 20 || k == 19 { return 4 }
        if k == 2 || k == 3 { return 5 }
        if k == 4 || k == 5 || k == 27 { return 6 }
        return -1
    }

    fn parse_struct_decl(self: Parser) {
        self.advance()
        let name: str = self.text()
        self.advance()
        self.ind()
        print("StructDecl(")
        print(name)
        println(")")
        self.expect(9)
        self.indent = self.indent + 1
        while self.tok_kind != 10 && self.tok_kind != 0 {
            self.ind()
            let fn_: str = self.text()
            self.advance()
            self.expect(11)
            let ft: str = self.text()
            self.advance()
            print(fn_)
            print(": ")
            println(ft)
            if self.tok_kind == 12 { self.advance() }
        }
        self.indent = self.indent - 1
        self.expect(10)
    }

    fn parse_enum_decl(self: Parser) {
        self.advance()
        let name: str = self.text()
        self.advance()
        self.ind()
        print("EnumDecl(")
        print(name)
        println(")")
        self.expect(9)
        self.indent = self.indent + 1
        while self.tok_kind != 10 && self.tok_kind != 0 {
            self.ind()
            let vn: str = self.text()
            self.advance()
            print(vn)
            if self.tok_kind == 7 {
                self.advance()
                print("(")
                let f: i64 = 1
                while self.tok_kind != 8 && self.tok_kind != 0 {
                    if f == 0 { print(", ") }
                    let pt: str = self.text()
                    self.advance()
                    print(pt)
                    f = 0
                    if self.tok_kind == 12 { self.advance() }
                }
                print(")")
                self.expect(8)
            }
            println("")
            if self.tok_kind == 12 { self.advance() }
        }
        self.indent = self.indent - 1
        self.expect(10)
    }

    fn parse_impl(self: Parser) {
        self.advance()
        let name: str = self.text()
        self.advance()
        self.ind()
        print("Impl(")
        print(name)
        if self.tok_kind == 11 {
            self.advance()
            let tn: str = self.text()
            self.advance()
            print(" : ")
            print(tn)
        }
        println(")")
        self.expect(9)
        self.indent = self.indent + 1
        while self.tok_kind != 10 && self.tok_kind != 0 {
            self.parse_fn()
        }
        self.indent = self.indent - 1
        self.expect(10)
    }

    fn parse_const(self: Parser) {
        self.advance()
        let name: str = self.text()
        self.advance()
        self.expect(11)
        let ty: str = self.text()
        self.advance()
        self.expect(6)
        self.ind()
        print("Const(")
        print(name)
        print(": ")
        print(ty)
        print(") = ")
        self.parse_expr(0)
        println("")
        self.skip_semi()
    }

    fn parse_import(self: Parser) {
        self.advance()
        let name: str = self.text()
        self.advance()
        self.ind()
        print("Import(")
        print(name)
        if self.tok_kind == 9 {
            self.advance()
            print(" {")
            let f: i64 = 1
            while self.tok_kind != 10 && self.tok_kind != 0 {
                if f == 0 { print(", ") }
                let it: str = self.text()
                self.advance()
                print(it)
                f = 0
                if self.tok_kind == 12 { self.advance() }
            }
            print("}")
            self.expect(10)
        }
        println(")")
        self.skip_semi()
    }
}

fn main() {
    let src: str = file_read("leo-bootstrap/test.leo")
    let p = Parser { src: src, pos: 0, len: src.len(), tok_kind: 0, tok_start: 0, tok_end: 0, indent: 0, has_err: 0 }
    p.parse_program()
}
