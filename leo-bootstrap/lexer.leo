struct Lexer {
    src: str,
    pos: i64,
    len: i64,
}

impl Lexer {
    fn peek(self: Lexer) -> i64 {
        if self.pos >= self.len { return 0 }
        return self.src[self.pos]
    }

    fn advance(self: Lexer) -> i64 {
        let ch: i64 = self.src[self.pos]
        self.pos = self.pos + 1
        return ch
    }

    fn skip_ws(self: Lexer) {
        while self.pos < self.len {
            let ch: i64 = self.peek()
            if ch == 32 || ch == 10 || ch == 9 {
                self.pos = self.pos + 1
            } else {
                return
            }
        }
    }

    fn read_ident(self: Lexer) -> str {
        let ch: i64 = self.advance()
        let s: str = char_to_str(ch)
        while is_alnum(self.peek()) {
            let c: str = char_to_str(self.advance())
            s = s + c
        }
        return s
    }

    fn read_number(self: Lexer) -> i64 {
        let ch: i64 = self.advance()
        let n: i64 = ch - 48
        while is_digit(self.peek()) {
            let d: i64 = self.advance() - 48
            n = n * 10 + d
        }
        return n
    }
}

fn main() {
    let src: str = "fn add 42"
    let lex = Lexer { src: src, pos: 0, len: src.len() }
    lex.skip_ws()
    let id1: str = lex.read_ident()
    println(id1)
    lex.skip_ws()
    let id2: str = lex.read_ident()
    println(id2)
    lex.skip_ws()
    let num: i64 = lex.read_number()
    println(num)
}
