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
            if ch == 32 || ch == 10 || ch == 9 || ch == 13 {
                self.pos = self.pos + 1
            } else {
                return
            }
        }
    }

    fn next(self: Lexer) -> i64 {
        self.skip_ws()
        if self.pos >= self.len { return 0 }

        let ch: i64 = self.advance()
        if ch == 43 { return 2 }
        if ch == 45 { return 3 }
        if ch == 42 { return 4 }
        if ch == 47 { return 5 }
        if ch == 61 { return 6 }
        if ch == 40 { return 7 }
        if ch == 41 { return 8 }
        if ch == 123 { return 9 }
        if ch == 125 { return 10 }
        if ch == 58 { return 11 }
        if ch == 44 { return 12 }
        if ch == 59 { return 13 }
        if ch == 45 { return 3 }

        if is_digit(ch) {
            let n: i64 = ch - 48
            while is_digit(self.peek()) {
                let d: i64 = self.advance() - 48
                n = n * 10 + d
            }
            return 100
        }

        if is_alpha(ch) {
            let s: str = char_to_str(ch)
            while is_alnum(self.peek()) {
                let c: str = char_to_str(self.advance())
                s = s + c
            }
            return 101
        }

        return 0
    }
}

fn main() {
    let src: str = file_read("leo-bootstrap/test.leo")
    let lex = Lexer { src: src, pos: 0, len: src.len() }
    let running: i64 = 1
    while running {
        let tok: i64 = lex.next()
        if tok == 0 {
            running = 0
        } else {
            println(tok)
        }
    }
}
