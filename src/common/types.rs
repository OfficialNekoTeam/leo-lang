use std::fmt;

use crate::common::error::{ErrorCode, ErrorKind, LeoError, LeoResult};

/// Split a comma-separated type argument list respecting `<>`, `[]`, and `()` nesting.
/// Returns `None` if brackets are unbalanced.
/// `split_type_args("str, Pair<i64, bool>")` → `Some(["str", "Pair<i64, bool>"])`
fn split_type_args(s: &str) -> Option<Vec<&str>> {
    let mut args = Vec::new();
    let mut stack = Vec::new();
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '<' | '[' | '(' => stack.push(c),
            '>' if i > 0 && s[..i].ends_with('-') => {}
            '>' | ']' | ')' => {
                let Some(open) = stack.pop() else {
                    return None;
                };
                if !matches!((open, c), ('<', '>') | ('[', ']') | ('(', ')')) {
                    return None;
                }
            }
            ',' if stack.is_empty() => {
                args.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    if !stack.is_empty() {
        return None;
    }
    let last = s[start..].trim();
    if !last.is_empty() {
        args.push(last);
    }
    Some(args)
}

fn type_parse_error(message: impl Into<String>) -> LeoError {
    LeoError::new(
        ErrorKind::Syntax,
        ErrorCode::ParserInvalidSyntax,
        message.into(),
    )
}

fn matching_close_paren(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (idx, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(idx),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq)]
pub enum LeoType {
    I8,
    I16,
    I32,
    I64,
    I128,
    ISize,
    U8,
    U16,
    U32,
    U64,
    U128,
    USize,
    F32,
    F64,
    Bool,
    Char,
    Str,
    Ptr,
    Array(Box<LeoType>, usize),
    Vec(Box<LeoType>),
    Tuple(Vec<LeoType>),
    Struct(String),
    Enum(String),
    Fn(Vec<LeoType>, Box<LeoType>),
    Unit,
    /// Unresolved type parameter, e.g. T in fn max<T>
    TypeVar(String),
    /// Instantiated generic, e.g. Stack<i64>
    Generic(String, Vec<LeoType>),
    /// Bottom type for functions that never return (panic, exit)
    Never,
    /// Explicit unknown type while inference is incomplete.
    Unknown,
}

impl LeoType {
    /// Fallible type parser. Sema and any caller with error context should prefer this.
    pub fn parse(s: &str) -> LeoResult<Self> {
        let ty = match s {
            "i8" => LeoType::I8,
            "i16" => LeoType::I16,
            "i32" => LeoType::I32,
            "i64" => LeoType::I64,
            "i128" => LeoType::I128,
            "isize" => LeoType::ISize,
            "u8" => LeoType::U8,
            "u16" => LeoType::U16,
            "u32" => LeoType::U32,
            "u64" => LeoType::U64,
            "u128" => LeoType::U128,
            "usize" => LeoType::USize,
            "f32" => LeoType::F32,
            "f64" => LeoType::F64,
            "bool" => LeoType::Bool,
            "char" => LeoType::Char,
            "str" | "string" => LeoType::Str,
            "unit" | "void" => LeoType::Unit,
            "never" | "!" => LeoType::Never,
            "ptr" => LeoType::Ptr,
            "unknown" | "_" => LeoType::Unknown,
            other => {
                if let Some(rest) = other.strip_prefix("fn(") {
                    let Some(close_pos) = matching_close_paren(rest) else {
                        return Err(type_parse_error(format!(
                            "invalid function type: {}",
                            other
                        )));
                    };
                    let params_str = &rest[..close_pos];
                    let after_params = rest[close_pos + 1..].trim();
                    let Some(ret_str) = after_params.strip_prefix("->") else {
                        return Err(type_parse_error(format!(
                            "invalid function type: {}",
                            other
                        )));
                    };
                    let params = if params_str.trim().is_empty() {
                        Vec::new()
                    } else {
                        split_type_args(params_str)
                            .ok_or_else(|| {
                                type_parse_error(format!("unbalanced function type: {}", other))
                            })?
                            .into_iter()
                            .map(LeoType::parse)
                            .collect::<LeoResult<Vec<_>>>()?
                    };
                    return Ok(LeoType::Fn(
                        params,
                        Box::new(LeoType::parse(ret_str.trim())?),
                    ));
                }
                if other.starts_with('(') && other.ends_with(')') {
                    let inner = other[1..other.len() - 1].trim();
                    if inner.is_empty() {
                        return Ok(LeoType::Unit);
                    }
                    let tuple_parts = split_type_args(inner).ok_or_else(|| {
                        type_parse_error(format!("unbalanced tuple type: {}", other))
                    })?;
                    if tuple_parts.len() > 1 || inner.ends_with(',') {
                        let elems = tuple_parts
                            .into_iter()
                            .map(LeoType::parse)
                            .collect::<LeoResult<Vec<_>>>()?;
                        return Ok(LeoType::Tuple(elems));
                    }
                    return LeoType::parse(inner);
                }
                // Array pattern: [T; N]
                if other.starts_with('[') && other.ends_with(']') {
                    let inner = &other[1..other.len() - 1];
                    if let Some(semi_pos) = inner.rfind(';') {
                        let elem_str = inner[..semi_pos].trim();
                        let size_str = inner[semi_pos + 1..].trim();
                        if let Ok(n) = size_str.parse::<usize>() {
                            return Ok(LeoType::Array(Box::new(LeoType::parse(elem_str)?), n));
                        }
                    }
                    return Err(type_parse_error(format!("invalid array type: {}", other)));
                }
                if other.starts_with('[') || other.ends_with(']') {
                    return Err(type_parse_error(format!("invalid array type: {}", other)));
                }
                // Generic pattern: Name<T1, T2, ...>
                if let Some(lt_pos) = other.find('<') {
                    if !other.ends_with('>') {
                        return Err(type_parse_error(format!("invalid generic type: {}", other)));
                    }
                    let name = &other[..lt_pos];
                    let inner = &other[lt_pos + 1..other.len() - 1];
                    let raw = split_type_args(inner).ok_or_else(|| {
                        type_parse_error(format!("unbalanced brackets in type: {}", other))
                    })?;
                    let args: Vec<LeoType> = raw
                        .into_iter()
                        .map(LeoType::parse)
                        .collect::<LeoResult<Vec<_>>>()?;
                    // Special-case Vec<T> to keep existing variant
                    if name == "Vec" && args.len() == 1 {
                        return Ok(LeoType::Vec(Box::new(
                            args.into_iter().next().unwrap_or(LeoType::Unknown),
                        )));
                    }
                    return Ok(LeoType::Generic(name.to_string(), args));
                }
                if other.contains('>') {
                    return Err(type_parse_error(format!("invalid generic type: {}", other)));
                }
                // Single uppercase letter → TypeVar (e.g. "T", "U")
                let bytes = other.as_bytes();
                if bytes.len() == 1 && bytes[0].is_ascii_uppercase() {
                    return Ok(LeoType::TypeVar(other.to_string()));
                }
                LeoType::Struct(other.to_string())
            }
        };
        Ok(ty)
    }

    /// Infallible type conversion for codegen call sites that have no error channel.
    /// Unknown or malformed type strings become `LeoType::Unknown`.
    pub fn from_str(s: &str) -> Self {
        Self::parse(s).unwrap_or(LeoType::Unknown)
    }

    pub fn is_string(&self) -> bool {
        matches!(self, LeoType::Str)
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            LeoType::I8
                | LeoType::I16
                | LeoType::I32
                | LeoType::I64
                | LeoType::I128
                | LeoType::ISize
                | LeoType::U8
                | LeoType::U16
                | LeoType::U32
                | LeoType::U64
                | LeoType::U128
                | LeoType::USize
                | LeoType::Char
                | LeoType::Bool
        )
    }

    pub fn is_float(&self) -> bool {
        matches!(self, LeoType::F32 | LeoType::F64)
    }

    pub fn is_pointer(&self) -> bool {
        matches!(
            self,
            LeoType::Ptr
                | LeoType::Str
                | LeoType::Struct(_)
                | LeoType::Vec(_)
                | LeoType::Tuple(_)
                | LeoType::Array(_, _)
                | LeoType::Generic(_, _)
        )
    }

    pub fn byte_size(&self) -> usize {
        match self {
            LeoType::I8 | LeoType::U8 | LeoType::Bool | LeoType::Char => 1,
            LeoType::I16 | LeoType::U16 => 2,
            LeoType::I32 | LeoType::U32 | LeoType::F32 => 4,
            LeoType::I64 | LeoType::U64 | LeoType::ISize | LeoType::USize | LeoType::F64 => 8,
            LeoType::I128 | LeoType::U128 => 16,
            LeoType::Str | LeoType::Ptr => 8,
            LeoType::Unit | LeoType::Never => 0,
            LeoType::Array(elem, n) => elem.byte_size() * n,
            LeoType::Tuple(elems) => elems.iter().map(LeoType::byte_size).sum(),
            LeoType::Vec(_) => 24,
            LeoType::Struct(_) | LeoType::Enum(_) | LeoType::Fn(_, _) => 0,
            LeoType::TypeVar(_) => 8,
            LeoType::Generic(_, _) => 0,
            LeoType::Unknown => 0,
        }
    }
}

impl fmt::Display for LeoType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LeoType::I8 => write!(f, "i8"),
            LeoType::I16 => write!(f, "i16"),
            LeoType::I32 => write!(f, "i32"),
            LeoType::I64 => write!(f, "i64"),
            LeoType::I128 => write!(f, "i128"),
            LeoType::ISize => write!(f, "isize"),
            LeoType::U8 => write!(f, "u8"),
            LeoType::U16 => write!(f, "u16"),
            LeoType::U32 => write!(f, "u32"),
            LeoType::U64 => write!(f, "u64"),
            LeoType::U128 => write!(f, "u128"),
            LeoType::USize => write!(f, "usize"),
            LeoType::F32 => write!(f, "f32"),
            LeoType::F64 => write!(f, "f64"),
            LeoType::Bool => write!(f, "bool"),
            LeoType::Char => write!(f, "char"),
            LeoType::Str => write!(f, "str"),
            LeoType::Ptr => write!(f, "ptr"),
            LeoType::Unit => write!(f, "unit"),
            LeoType::Never => write!(f, "!"),
            LeoType::Unknown => write!(f, "unknown"),
            LeoType::Array(elem, n) => write!(f, "[{}; {}]", elem, n),
            LeoType::Vec(elem) => write!(f, "Vec<{}>", elem),
            LeoType::Tuple(elems) => {
                write!(f, "(")?;
                for (i, elem) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", elem)?;
                }
                if elems.len() == 1 {
                    write!(f, ",")?;
                }
                write!(f, ")")
            }
            LeoType::Struct(name) => write!(f, "{}", name),
            LeoType::Enum(name) => write!(f, "{}", name),
            LeoType::Fn(params, ret) => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", p)?;
                }
                write!(f, ") -> {}", ret)
            }
            LeoType::TypeVar(name) => write!(f, "{}", name),
            LeoType::Generic(name, args) => {
                write!(f, "{}<", name)?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", a)?;
                }
                write!(f, ">")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str_primitives() {
        assert_eq!(LeoType::from_str("i64"), LeoType::I64);
        assert_eq!(LeoType::from_str("f64"), LeoType::F64);
        assert_eq!(LeoType::from_str("bool"), LeoType::Bool);
        assert_eq!(LeoType::from_str("char"), LeoType::Char);
        assert_eq!(LeoType::from_str("str"), LeoType::Str);
        assert_eq!(LeoType::from_str("ptr"), LeoType::Ptr);
        assert_eq!(LeoType::from_str("unit"), LeoType::Unit);
        assert_eq!(LeoType::from_str("unknown"), LeoType::Unknown);
    }

    #[test]
    fn test_from_str_vec() {
        assert_eq!(
            LeoType::from_str("Vec<i64>"),
            LeoType::Vec(Box::new(LeoType::I64))
        );
        assert_eq!(
            LeoType::from_str("Vec<str>"),
            LeoType::Vec(Box::new(LeoType::Str))
        );
    }

    #[test]
    fn test_from_str_typevar() {
        assert_eq!(LeoType::from_str("T"), LeoType::TypeVar("T".into()));
        assert_eq!(LeoType::from_str("U"), LeoType::TypeVar("U".into()));
    }

    #[test]
    fn test_from_str_struct() {
        assert_eq!(LeoType::from_str("Point"), LeoType::Struct("Point".into()));
    }

    #[test]
    fn test_display_roundtrip() {
        let ty = LeoType::Vec(Box::new(LeoType::I64));
        assert_eq!(format!("{}", ty), "Vec<i64>");
        let ty2 = LeoType::Array(Box::new(LeoType::Bool), 3);
        assert_eq!(format!("{}", ty2), "[bool; 3]");
    }

    #[test]
    fn test_is_pointer() {
        assert!(LeoType::Struct("Foo".into()).is_pointer());
        assert!(LeoType::Vec(Box::new(LeoType::I64)).is_pointer());
        assert!(LeoType::Str.is_pointer());
        assert!(!LeoType::I64.is_pointer());
    }

    #[test]
    fn test_from_str_extended_ints() {
        assert_eq!(LeoType::from_str("i8"), LeoType::I8);
        assert_eq!(LeoType::from_str("i16"), LeoType::I16);
        assert_eq!(LeoType::from_str("i128"), LeoType::I128);
        assert_eq!(LeoType::from_str("isize"), LeoType::ISize);
        assert_eq!(LeoType::from_str("u8"), LeoType::U8);
        assert_eq!(LeoType::from_str("u16"), LeoType::U16);
        assert_eq!(LeoType::from_str("u32"), LeoType::U32);
        assert_eq!(LeoType::from_str("u64"), LeoType::U64);
        assert_eq!(LeoType::from_str("u128"), LeoType::U128);
        assert_eq!(LeoType::from_str("usize"), LeoType::USize);
    }

    #[test]
    fn test_from_str_f32_never() {
        assert_eq!(LeoType::from_str("f32"), LeoType::F32);
        assert_eq!(LeoType::from_str("never"), LeoType::Never);
        assert_eq!(LeoType::from_str("!"), LeoType::Never);
    }

    #[test]
    fn test_byte_sizes() {
        assert_eq!(LeoType::I8.byte_size(), 1);
        assert_eq!(LeoType::U16.byte_size(), 2);
        assert_eq!(LeoType::I32.byte_size(), 4);
        assert_eq!(LeoType::U64.byte_size(), 8);
        assert_eq!(LeoType::ISize.byte_size(), 8);
        assert_eq!(LeoType::USize.byte_size(), 8);
        assert_eq!(LeoType::I128.byte_size(), 16);
        assert_eq!(LeoType::U128.byte_size(), 16);
        assert_eq!(LeoType::F32.byte_size(), 4);
        assert_eq!(LeoType::Never.byte_size(), 0);
    }

    #[test]
    fn test_is_integer_extended() {
        assert!(LeoType::I8.is_integer());
        assert!(LeoType::I128.is_integer());
        assert!(LeoType::USize.is_integer());
        assert!(LeoType::U32.is_integer());
        assert!(!LeoType::F32.is_integer());
        assert!(LeoType::F32.is_float());
    }

    #[test]
    fn test_from_str_array_notation() {
        assert_eq!(
            LeoType::from_str("[i64; 5]"),
            LeoType::Array(Box::new(LeoType::I64), 5)
        );
        assert_eq!(
            LeoType::from_str("[bool; 0]"),
            LeoType::Array(Box::new(LeoType::Bool), 0)
        );
        assert_eq!(
            LeoType::from_str("[str; 3]"),
            LeoType::Array(Box::new(LeoType::Str), 3)
        );
    }

    #[test]
    fn test_from_str_nested_generic() {
        // Map<str, Pair<i64, bool>> must not be split at the inner comma
        assert_eq!(
            LeoType::from_str("Map<str, Pair<i64, bool>>"),
            LeoType::Generic(
                "Map".into(),
                vec![
                    LeoType::Str,
                    LeoType::Generic("Pair".into(), vec![LeoType::I64, LeoType::Bool]),
                ]
            )
        );
    }

    #[test]
    fn test_from_str_generic_with_array_arg() {
        // Vec<[i64; 4]> — array inside generic args
        assert_eq!(
            LeoType::from_str("Vec<[i64; 4]>"),
            LeoType::Vec(Box::new(LeoType::Array(Box::new(LeoType::I64), 4)))
        );
    }

    #[test]
    fn test_parse_unbalanced_extra_close_is_err() {
        assert!(LeoType::parse("Pair<i64, bool>>").is_err());
    }

    #[test]
    fn test_parse_mismatched_brackets_is_err() {
        assert!(LeoType::parse("Pair<i64]").is_err());
    }

    #[test]
    fn test_parse_unbalanced_unclosed_is_err() {
        assert!(LeoType::parse("Map<str, Pair<i64, bool>").is_err());
    }

    #[test]
    fn test_parse_invalid_array_is_err() {
        assert!(LeoType::parse("[i64]").is_err());
        assert!(LeoType::parse("[i64; n]").is_err());
    }

    #[test]
    fn test_from_str_unbalanced_falls_back_to_unknown() {
        // Unclosed bracket: parse returns Err; from_str maps that to Unknown
        assert_eq!(
            LeoType::from_str("Map<str, Pair<i64, bool>"),
            LeoType::Unknown
        );
    }

    #[test]
    fn test_parse_fn_type_args_with_parens() {
        // split_type_args must handle () depth — Fn(i64, bool) -> str is not yet
        // a type annotation literal, but split_type_args should not miscount parens
        // inside a hypothetical arg like "Fn(i64)".
        assert!(split_type_args("Fn(i64), str").is_some());
        assert_eq!(split_type_args("Fn(i64), str").unwrap().len(), 2);
    }

    #[test]
    fn test_parse_function_type() {
        assert_eq!(
            LeoType::parse("fn(i64, bool) -> str").unwrap(),
            LeoType::Fn(vec![LeoType::I64, LeoType::Bool], Box::new(LeoType::Str))
        );
        assert_eq!(
            LeoType::parse("fn(fn(i64) -> i64) -> f32").unwrap(),
            LeoType::Fn(
                vec![LeoType::Fn(vec![LeoType::I64], Box::new(LeoType::I64))],
                Box::new(LeoType::F32)
            )
        );
    }

    #[test]
    fn test_parse_tuple_type() {
        assert_eq!(
            LeoType::parse("(i64, bool, str)").unwrap(),
            LeoType::Tuple(vec![LeoType::I64, LeoType::Bool, LeoType::Str])
        );
        assert_eq!(
            LeoType::parse("(i64,)").unwrap(),
            LeoType::Tuple(vec![LeoType::I64])
        );
        assert_eq!(LeoType::parse("()").unwrap(), LeoType::Unit);
    }
}
