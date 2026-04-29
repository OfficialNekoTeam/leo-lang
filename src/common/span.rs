use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pos {
    pub line: u32,
    pub column: u32,
    pub offset: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: Pos,
    pub end: Pos,
}

impl Pos {
    pub fn new(line: u32, column: u32, offset: u32) -> Self {
        Self {
            line,
            column,
            offset,
        }
    }

    pub fn is_before(self, other: Pos) -> bool {
        self.offset < other.offset
    }
}

impl Span {
    pub fn new(start: Pos, end: Pos) -> Self {
        Self { start, end }
    }

    pub fn dummy() -> Self {
        Self {
            start: Pos::new(0, 0, 0),
            end: Pos::new(0, 0, 0),
        }
    }

    pub fn contains(self, pos: Pos) -> bool {
        pos.offset >= self.start.offset && pos.offset <= self.end.offset
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{} - {}:{}",
            self.start.line, self.start.column, self.end.line, self.end.column
        )
    }
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Span({}..{})", self.start.offset, self.end.offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pos_new() {
        let pos = Pos::new(1, 1, 0);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.column, 1);
        assert_eq!(pos.offset, 0);
    }

    #[test]
    fn test_span_new() {
        let start = Pos::new(1, 1, 0);
        let end = Pos::new(1, 5, 4);
        let span = Span::new(start, end);
        assert_eq!(span.start.line, 1);
        assert_eq!(span.end.column, 5);
    }

    #[test]
    fn test_span_contains() {
        let span = Span::new(Pos::new(1, 1, 0), Pos::new(1, 5, 4));
        assert!(span.contains(Pos::new(1, 3, 2)));
        assert!(!span.contains(Pos::new(1, 6, 5)));
    }

    #[test]
    fn test_span_dummy() {
        let span = Span::dummy();
        assert_eq!(span.start.offset, 0);
        assert_eq!(span.end.offset, 0);
    }

    #[test]
    fn test_pos_is_before() {
        let a = Pos::new(1, 1, 0);
        let b = Pos::new(1, 5, 4);
        assert!(a.is_before(b));
        assert!(!b.is_before(a));
    }
}
