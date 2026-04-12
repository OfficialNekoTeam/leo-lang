use std::collections::HashMap;

pub struct Scope {
    vars: HashMap<String, String>,
    parent: Option<Box<Scope>>,
}

impl Scope {
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
            parent: None,
        }
    }

    pub fn with_parent(parent: Scope) -> Self {
        Self {
            vars: HashMap::new(),
            parent: Some(Box::new(parent)),
        }
    }

    pub fn define(&mut self, name: String, ty: String) {
        self.vars.insert(name, ty);
    }

    pub fn resolve(&self, name: &str) -> Option<&String> {
        self.vars.get(name).or_else(|| self.parent.as_ref()?.resolve(name))
    }
}