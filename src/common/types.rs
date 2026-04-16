use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum LeoType {
    I64,
    I32,
    F64,
    Bool,
    Char,
    Str,
    Ptr,
    Array(Box<LeoType>, usize),
    Vec(Box<LeoType>),
    Struct(String),
    Enum(String),
    Fn(Vec<LeoType>, Box<LeoType>),
    Unit,
    /// Unresolved type parameter, e.g. T in fn max<T>
    TypeVar(String),
    /// Instantiated generic, e.g. Stack<i64>
    Generic(String, Vec<LeoType>),
}

impl LeoType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "i64" => LeoType::I64,
            "i32" => LeoType::I32,
            "f64" => LeoType::F64,
            "bool" => LeoType::Bool,
            "char" => LeoType::Char,
            "str" | "string" => LeoType::Str,
            "unit" | "void" => LeoType::Unit,
            other => {
                // Vec<T> pattern
                if let Some(inner) = other.strip_prefix("Vec<").and_then(|s| s.strip_suffix('>')) {
                    return LeoType::Vec(Box::new(LeoType::from_str(inner)));
                }
                // Single uppercase letter → TypeVar (e.g. "T", "U")
                let bytes = other.as_bytes();
                if bytes.len() == 1 && bytes[0].is_ascii_uppercase() {
                    return LeoType::TypeVar(other.to_string());
                }
                LeoType::Struct(other.to_string())
            }
        }
    }

    pub fn is_string(&self) -> bool {
        matches!(self, LeoType::Str)
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            LeoType::I64 | LeoType::I32 | LeoType::Char | LeoType::Bool
        )
    }

    pub fn is_float(&self) -> bool {
        matches!(self, LeoType::F64)
    }

    pub fn is_pointer(&self) -> bool {
        matches!(
            self,
            LeoType::Ptr
                | LeoType::Str
                | LeoType::Struct(_)
                | LeoType::Vec(_)
                | LeoType::Array(_, _)
        )
    }

    pub fn byte_size(&self) -> usize {
        match self {
            LeoType::I64 | LeoType::F64 => 8,
            LeoType::I32 => 4,
            LeoType::Bool => 1,
            LeoType::Char => 1,
            LeoType::Str | LeoType::Ptr => 8,
            LeoType::Unit => 0,
            LeoType::Array(elem, n) => elem.byte_size() * n,
            LeoType::Vec(_) => 24,
            LeoType::Struct(_) => 0,
            LeoType::Enum(_) => 0,
            LeoType::Fn(_, _) => 0,
            LeoType::TypeVar(_) => 8,
            LeoType::Generic(_, _) => 0,
        }
    }
}

impl fmt::Display for LeoType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LeoType::I64 => write!(f, "i64"),
            LeoType::I32 => write!(f, "i32"),
            LeoType::F64 => write!(f, "f64"),
            LeoType::Bool => write!(f, "bool"),
            LeoType::Char => write!(f, "char"),
            LeoType::Str => write!(f, "str"),
            LeoType::Ptr => write!(f, "ptr"),
            LeoType::Unit => write!(f, "unit"),
            LeoType::Array(elem, n) => write!(f, "[{}; {}]", elem, n),
            LeoType::Vec(elem) => write!(f, "Vec<{}>", elem),
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
        assert_eq!(LeoType::from_str("unit"), LeoType::Unit);
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
        assert_eq!(
            LeoType::from_str("Point"),
            LeoType::Struct("Point".into())
        );
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
}
