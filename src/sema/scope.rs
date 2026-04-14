use std::collections::HashMap;

/// Symbol entry stored in scope
#[derive(Debug, Clone)]
pub struct SymbolEntry {
    pub name: String,
    pub ty: String,
    pub mutable: bool,
}

/// Lexical scope with parent chain lookup
pub struct Scope {
    symbols: HashMap<String, SymbolEntry>,
    parent: Option<Box<Scope>>,
}

impl Scope {
    /// Create root scope
    pub fn new() -> Self {
        Self { symbols: HashMap::new(), parent: None }
    }

    /// Create child scope with parent reference
    pub fn with_parent(parent: Scope) -> Self {
        Self { symbols: HashMap::new(), parent: Some(Box::new(parent)) }
    }

    /// Define a new symbol in current scope
    pub fn define(&mut self, name: String, ty: String, mutable: bool) {
        self.symbols.insert(name.clone(), SymbolEntry { name, ty, mutable });
    }

    /// Resolve symbol by walking up scope chain
    pub fn resolve(&self, name: &str) -> Option<&SymbolEntry> {
        self.symbols.get(name).or_else(|| self.parent.as_ref()?.resolve(name))
    }

    /// Check if symbol exists in current scope only
    pub fn defined_locally(&self, name: &str) -> bool {
        self.symbols.contains_key(name)
    }

    /// Get all symbol names in current scope
    pub fn symbol_names(&self) -> Vec<&String> {
        self.symbols.keys().collect()
    }

    /// Extract parent scope, consuming child
    pub fn into_parent(self) -> Option<Scope> {
        self.parent.map(|p| *p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_define_and_resolve() {
        let mut scope = Scope::new();
        scope.define("x".into(), "i32".into(), false);
        let entry = scope.resolve("x").unwrap();
        assert_eq!(entry.ty, "i32");
        assert!(!entry.mutable);
    }

    #[test]
    fn test_scope_resolve_not_found() {
        let scope = Scope::new();
        assert!(scope.resolve("x").is_none());
    }

    #[test]
    fn test_scope_parent_lookup() {
        let mut parent = Scope::new();
        parent.define("x".into(), "i32".into(), false);
        let child = Scope::with_parent(parent);
        let entry = child.resolve("x").unwrap();
        assert_eq!(entry.ty, "i32");
    }

    #[test]
    fn test_scope_shadow() {
        let mut parent = Scope::new();
        parent.define("x".into(), "i32".into(), false);
        let mut child = Scope::with_parent(parent);
        child.define("x".into(), "f64".into(), true);
        let entry = child.resolve("x").unwrap();
        assert_eq!(entry.ty, "f64");
        assert!(entry.mutable);
    }

    #[test]
    fn test_scope_defined_locally() {
        let mut scope = Scope::new();
        scope.define("x".into(), "i32".into(), false);
        assert!(scope.defined_locally("x"));
        assert!(!scope.defined_locally("y"));
    }

    #[test]
    fn test_scope_symbol_names() {
        let mut scope = Scope::new();
        scope.define("x".into(), "i32".into(), false);
        scope.define("y".into(), "f64".into(), true);
        let mut names = scope.symbol_names();
        names.sort();
        assert_eq!(names, vec!["x", "y"]);
    }
}
