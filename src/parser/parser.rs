use crate::common::{ErrorCode, ErrorKind, LeoError, LeoResult, Span};
use crate::ast::expr::{BinOp, Expr, UnOp};
use crate::ast::stmt::Stmt;
use crate::lexer::token::{Keyword, Symbol, Token, TokenWithSpan};

/// Recursive descent parser with Pratt expression parsing
pub struct Parser {
    tokens: Vec<TokenWithSpan>,
    pos: usize,
}

impl Parser {
    /// Create parser from token stream
    pub fn new(tokens: Vec<TokenWithSpan>) -> Self {
        Self { tokens, pos: 0 }
    }

    /// Parse full program into statement list
    pub fn parse(&mut self) -> LeoResult<Vec<Stmt>> {
        let mut stmts = Vec::new();
        while !self.is_eof() {
            if self.skip_comment() { continue; }
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    // --- Token helpers ---

    fn is_eof(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn peek(&self) -> Option<&TokenWithSpan> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&TokenWithSpan> {
        if self.pos < self.tokens.len() {
            self.pos += 1;
            self.tokens.get(self.pos - 1)
        } else {
            None
        }
    }

    fn prev_span(&self) -> Span {
        if self.pos > 0 { self.tokens[self.pos - 1].span } else { Span::dummy() }
    }

    fn cur_span(&self) -> Span {
        self.peek().map(|t| t.span).unwrap_or(Span::dummy())
    }

    fn merge(&self, a: Span, b: Span) -> Span {
        Span::new(a.start, b.end)
    }

    /// Expect specific symbol, return its span
    fn expect_sym(&mut self, sym: Symbol) -> LeoResult<Span> {
        let tws = self.peek().ok_or_else(|| self.eof_err("symbol"))?;
        match &tws.token {
            Token::Symbol(s) if *s == sym => { let s = tws.span; self.advance(); Ok(s) }
            _ => Err(self.unexpected(tws)),
        }
    }

    /// Expect identifier, return its name
    fn expect_ident(&mut self) -> LeoResult<String> {
        let tws = self.peek().ok_or_else(|| self.eof_err("identifier"))?;
        match &tws.token {
            Token::Identifier(n) => { let n = n.clone(); self.advance(); Ok(n) }
            _ => Err(self.unexpected(tws)),
        }
    }

    /// Consume matching symbol silently
    fn match_sym(&mut self, sym: Symbol) -> bool {
        if let Some(t) = self.peek() {
            if matches!(&t.token, Token::Symbol(s) if *s == sym) {
                self.advance(); return true;
            }
        }
        false
    }

    /// Consume matching keyword silently
    fn match_kw(&mut self, kw: Keyword) -> bool {
        if let Some(t) = self.peek() {
            if matches!(&t.token, Token::Keyword(k) if *k == kw) {
                self.advance(); return true;
            }
        }
        false
    }

    /// Skip optional semicolon
    fn skip_semi(&mut self) { self.match_sym(Symbol::Semicolon); }

    /// Skip comment token
    fn skip_comment(&mut self) -> bool {
        if matches!(self.peek(), Some(t) if matches!(t.token, Token::Comment(_))) {
            self.advance(); true
        } else { false }
    }

    /// Check if peek token is a specific symbol
    fn is_sym(&self, sym: Symbol) -> bool {
        self.peek().map_or(false, |t| matches!(&t.token, Token::Symbol(s) if *s == sym))
    }

    /// Check if current token starts an expression
    fn is_expr_start(&self) -> bool {
        self.peek().map_or(false, |t| match &t.token {
            Token::Number(_) | Token::Float(_) | Token::String(_) |
            Token::Identifier(_) | Token::Keyword(Keyword::True) |
            Token::Keyword(Keyword::False) |
            Token::Symbol(Symbol::LeftParen) |
            Token::Symbol(Symbol::Bang) |
            Token::Symbol(Symbol::Minus) => true,
            _ => false,
        })
    }

    fn eof_err(&self, expected: &str) -> LeoError {
        LeoError::new(ErrorKind::Syntax, ErrorCode::ParserUnexpectedToken,
            format!("expected {} but reached end of input", expected))
    }

    fn unexpected(&self, tws: &TokenWithSpan) -> LeoError {
        LeoError::new(ErrorKind::Syntax, ErrorCode::ParserUnexpectedToken,
            format!("unexpected token: {:?}", tws.token)).with_span(tws.span)
    }

    // --- Statement parsing ---

    /// Dispatch to correct statement parser
    fn parse_stmt(&mut self) -> LeoResult<Stmt> {
        let tws = self.peek().ok_or_else(|| self.eof_err("statement"))?;
        match &tws.token {
            Token::Keyword(Keyword::Let) => self.parse_let(),
            Token::Keyword(Keyword::Fn) => self.parse_fn(false),
            Token::Keyword(Keyword::Async) => { self.advance(); self.parse_fn(true) }
            Token::Keyword(Keyword::Return) => self.parse_return(),
            Token::Keyword(Keyword::If) => self.parse_if(),
            Token::Keyword(Keyword::While) => self.parse_while(),
            Token::Keyword(Keyword::For) => self.parse_for(),
            Token::Keyword(Keyword::Import) => self.parse_import(),
            Token::Keyword(Keyword::Struct) => self.parse_struct(),
            Token::Keyword(Keyword::Pub) => {
                self.advance();
                Ok(Stmt::Pub(Box::new(self.parse_stmt()?)))
            }
            Token::Keyword(Keyword::Break) => {
                let span = self.cur_span();
                self.advance();
                let expr = if self.is_expr_start() { Some(self.parse_expr()?) } else { None };
                self.skip_semi();
                Ok(Stmt::Break(expr, span))
            }
            Token::Keyword(Keyword::Continue) => {
                let span = self.cur_span();
                self.advance();
                self.skip_semi();
                Ok(Stmt::Continue(span))
            }
            _ => { let e = self.parse_expr()?; self.skip_semi(); Ok(Stmt::Expr(e)) }
        }
    }

    /// Parse let binding: let name [: Type] [= expr]
    fn parse_let(&mut self) -> LeoResult<Stmt> {
        self.advance();
        let name = self.expect_ident()?;
        let ty = if self.match_sym(Symbol::Colon) { Some(self.expect_ident()?) } else { None };
        let init = if self.match_sym(Symbol::Equal) { Some(self.parse_expr()?) } else { None };
        self.skip_semi();
        Ok(Stmt::Let(name, ty, init))
    }

    /// Parse function: fn name(params) [-> Type] { body }
    fn parse_fn(&mut self, is_async: bool) -> LeoResult<Stmt> {
        let start = self.cur_span();
        self.advance();
        let name = self.expect_ident()?;
        self.expect_sym(Symbol::LeftParen)?;
        let params = self.parse_params()?;
        self.expect_sym(Symbol::RightParen)?;
        let ret = if self.match_sym(Symbol::Arrow) { Some(self.expect_ident()?) } else { None };
        let body = self.parse_block()?;
        let span = self.merge(start, self.prev_span());
        if is_async { Ok(Stmt::AsyncFunction(name, params, ret, body, span)) }
        else { Ok(Stmt::Function(name, params, ret, body, span)) }
    }

    /// Parse comma-separated params: name: Type, ...
    fn parse_params(&mut self) -> LeoResult<Vec<(String, String)>> {
        let mut params = Vec::new();
        loop {
            if self.is_sym(Symbol::RightParen) { break; }
            let name = self.expect_ident()?;
            self.expect_sym(Symbol::Colon)?;
            let ty = self.expect_ident()?;
            params.push((name, ty));
            if !self.match_sym(Symbol::Comma) { break; }
        }
        Ok(params)
    }

    /// Parse block: { stmt* }
    fn parse_block(&mut self) -> LeoResult<Vec<Stmt>> {
        self.expect_sym(Symbol::LeftBrace)?;
        let mut stmts = Vec::new();
        while !self.is_eof() && !self.match_sym(Symbol::RightBrace) {
            if self.skip_comment() { continue; }
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    /// Parse return: return [expr]
    fn parse_return(&mut self) -> LeoResult<Stmt> {
        let span = self.cur_span();
        self.advance();
        let expr = if self.is_expr_start() { Some(self.parse_expr()?) } else { None };
        self.skip_semi();
        Ok(Stmt::Return(expr, span))
    }

    /// Parse if: if expr { stmts } [else { stmts }]
    fn parse_if(&mut self) -> LeoResult<Stmt> {
        let start = self.cur_span();
        self.advance();
        let cond = self.parse_expr()?;
        let then = self.parse_block()?;
        let branches = vec![(cond, then)];
        let els = if self.match_kw(Keyword::Else) { Some(self.parse_block()?) } else { None };
        Ok(Stmt::If(branches, els, self.merge(start, self.prev_span())))
    }

    /// Parse while: while expr { stmts }
    fn parse_while(&mut self) -> LeoResult<Stmt> {
        let start = self.cur_span();
        self.advance();
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::While(cond, body, self.merge(start, self.prev_span())))
    }

    /// Parse for: for name [in] expr { stmts }
    fn parse_for(&mut self) -> LeoResult<Stmt> {
        let start = self.cur_span();
        self.advance();
        let name = self.expect_ident()?;
        // consume optional 'in' (tokenized as identifier)
        if let Some(t) = self.peek() {
            if matches!(&t.token, Token::Identifier(s) if s == "in") { self.advance(); }
        }
        let iter = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::For(name, iter, body, self.merge(start, self.prev_span())))
    }

    /// Parse import: import name [{ items }]
    fn parse_import(&mut self) -> LeoResult<Stmt> {
        let start = self.cur_span();
        self.advance();
        let name = self.expect_ident()?;
        let items = if self.match_sym(Symbol::LeftBrace) {
            let mut v = Vec::new();
            while !self.is_eof() && !self.is_sym(Symbol::RightBrace) {
                v.push(self.expect_ident()?);
                self.match_sym(Symbol::Comma);
            }
            self.expect_sym(Symbol::RightBrace)?;
            Some(v)
        } else { None };
        Ok(Stmt::Import(name, items, self.merge(start, self.prev_span())))
    }

    /// Parse struct: struct Name { field: Type, ... }
    fn parse_struct(&mut self) -> LeoResult<Stmt> {
        let start = self.cur_span();
        self.advance();
        let name = self.expect_ident()?;
        self.expect_sym(Symbol::LeftBrace)?;
        let mut fields = Vec::new();
        while !self.is_eof() && !self.is_sym(Symbol::RightBrace) {
            let fname = self.expect_ident()?;
            self.expect_sym(Symbol::Colon)?;
            let ftype = self.expect_ident()?;
            fields.push((fname, ftype));
            self.match_sym(Symbol::Comma);
        }
        self.expect_sym(Symbol::RightBrace)?;
        Ok(Stmt::Struct(name, fields, self.merge(start, self.prev_span())))
    }

    // --- Expression parsing (Pratt precedence climbing) ---

    /// Parse expression with lowest precedence
    fn parse_expr(&mut self) -> LeoResult<Expr> {
        self.parse_prec(0)
    }

    /// Recursive precedence climbing
    fn parse_prec(&mut self, min: u8) -> LeoResult<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let Some((op, prec)) = self.peek_binop() else { break };
            if prec < min { break; }
            self.advance();
            let right = self.parse_prec(prec + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right), Span::dummy());
        }
        Ok(left)
    }

    /// Map current token to binary operator + precedence
    fn peek_binop(&self) -> Option<(BinOp, u8)> {
        match &self.peek()?.token {
            Token::Symbol(Symbol::Pipe) => Some((BinOp::Or, 1)),
            Token::Symbol(Symbol::Ampersand) => Some((BinOp::And, 2)),
            Token::Symbol(Symbol::DoubleEqual) => Some((BinOp::Eq, 3)),
            Token::Symbol(Symbol::BangEqual) => Some((BinOp::Ne, 3)),
            Token::Symbol(Symbol::Less) => Some((BinOp::Lt, 4)),
            Token::Symbol(Symbol::LessEqual) => Some((BinOp::Le, 4)),
            Token::Symbol(Symbol::Greater) => Some((BinOp::Gt, 4)),
            Token::Symbol(Symbol::GreaterEqual) => Some((BinOp::Ge, 4)),
            Token::Symbol(Symbol::Plus) => Some((BinOp::Add, 5)),
            Token::Symbol(Symbol::Minus) => Some((BinOp::Sub, 5)),
            Token::Symbol(Symbol::Star) => Some((BinOp::Mul, 6)),
            Token::Symbol(Symbol::Slash) => Some((BinOp::Div, 6)),
            Token::Symbol(Symbol::Percent) => Some((BinOp::Mod, 6)),
            _ => None,
        }
    }

    /// Parse unary: -expr, !expr
    fn parse_unary(&mut self) -> LeoResult<Expr> {
        let op = if self.match_sym(Symbol::Minus) { Some(UnOp::Neg) }
                 else if self.match_sym(Symbol::Bang) { Some(UnOp::Not) }
                 else { None };
        if let Some(op) = op {
            let e = self.parse_unary()?;
            return Ok(Expr::Unary(op, Box::new(e), Span::dummy()));
        }
        self.parse_postfix()
    }

    /// Parse postfix: call(), .field, [index]
    fn parse_postfix(&mut self) -> LeoResult<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.match_sym(Symbol::LeftParen) {
                let mut args = Vec::new();
                while !self.is_eof() && !self.is_sym(Symbol::RightParen) {
                    args.push(self.parse_expr()?);
                    self.match_sym(Symbol::Comma);
                }
                self.expect_sym(Symbol::RightParen)?;
                expr = Expr::Call(Box::new(expr), args, Span::dummy());
            } else if self.match_sym(Symbol::Dot) {
                let name = self.expect_ident()?;
                expr = Expr::Select(Box::new(expr), name, Span::dummy());
            } else if self.match_sym(Symbol::LeftBracket) {
                let idx = self.parse_expr()?;
                self.expect_sym(Symbol::RightBracket)?;
                expr = Expr::Index(Box::new(expr), Box::new(idx), Span::dummy());
            } else { break; }
        }
        Ok(expr)
    }

    /// Parse primary: literal, ident, (expr)
    fn parse_primary(&mut self) -> LeoResult<Expr> {
        let tws = self.peek().ok_or_else(|| self.eof_err("expression"))?;
        let span = tws.span;
        match &tws.token {
            Token::Number(n) => { let n = *n; self.advance(); Ok(Expr::Number(n, span)) }
            Token::Float(f) => { let f = *f; self.advance(); Ok(Expr::Float(f, span)) }
            Token::String(s) => { let s = s.clone(); self.advance(); Ok(Expr::String(s, span)) }
            Token::Identifier(n) => { let n = n.clone(); self.advance(); Ok(Expr::Ident(n, span)) }
            Token::Keyword(Keyword::True) => { self.advance(); Ok(Expr::Bool(true, span)) }
            Token::Keyword(Keyword::False) => { self.advance(); Ok(Expr::Bool(false, span)) }
            Token::Symbol(Symbol::LeftParen) => {
                self.advance();
                let e = self.parse_expr()?;
                self.expect_sym(Symbol::RightParen)?;
                Ok(e)
            }
            _ => Err(self.unexpected(tws)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    /// Helper: lex + parse source code
    fn parse_src(src: &str) -> Vec<Stmt> {
        let mut lex = Lexer::new(src);
        let tokens = lex.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        parser.parse().unwrap()
    }

    #[test]
    fn test_parse_number() {
        let stmts = parse_src("42");
        assert!(matches!(&stmts[0], Stmt::Expr(Expr::Number(42, _))));
    }

    #[test]
    fn test_parse_let() {
        let stmts = parse_src("let x: i32 = 42");
        assert!(matches!(&stmts[0], Stmt::Let(n, _, _) if n == "x"));
    }

    #[test]
    fn test_parse_fn() {
        let stmts = parse_src("fn add(a: i32, b: i32) -> i32 { return a }");
        assert!(matches!(&stmts[0], Stmt::Function(..)));
    }

    #[test]
    fn test_parse_binary_precedence() {
        let stmts = parse_src("1 + 2 * 3");
        assert!(matches!(&stmts[0], Stmt::Expr(Expr::Binary(BinOp::Add, ..))));
    }

    #[test]
    fn test_parse_call() {
        let stmts = parse_src("foo(1, 2)");
        assert!(matches!(&stmts[0], Stmt::Expr(Expr::Call(..))));
    }

    #[test]
    fn test_parse_if() {
        let stmts = parse_src("if x { return 1 }");
        assert!(matches!(&stmts[0], Stmt::If(..)));
    }

    #[test]
    fn test_parse_struct() {
        let stmts = parse_src("struct Foo { x: i32, y: i32 }");
        assert!(matches!(&stmts[0], Stmt::Struct(..)));
    }

    #[test]
    fn test_parse_while() {
        let stmts = parse_src("while true { break }");
        // 'break' not parsed as keyword, falls to expr -> error is ok
        // test while dispatch works
        assert_eq!(stmts.len(), 1);
    }
}
