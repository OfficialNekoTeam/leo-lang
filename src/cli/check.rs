use std::fs;

/// Type-check project without generating code
pub fn check() -> Result<(), String> {
    let entry = read_entry()?;
    let source = fs::read_to_string(&entry).map_err(|e| format!("read {} failed: {}", entry, e))?;

    let mut lexer = crate::lexer::Lexer::new(&source);
    let tokens = lexer.tokenize().map_err(|e| format!("{}", e))?;
    let mut parser = crate::parser::Parser::new(tokens);
    let stmts = parser.parse().map_err(|e| format!("{}", e))?;
    let mut checker = crate::sema::Checker::new();
    checker.check(&stmts).map_err(|e| format!("{}", e))?;
    eprintln!("Type check passed.");
    Ok(())
}

/// Read entry point from leo.toml
fn read_entry() -> Result<String, String> {
    let content =
        fs::read_to_string("leo.toml").map_err(|e| format!("read leo.toml failed: {}", e))?;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("entry = ") {
            return Ok(rest.trim().trim_matches('"').to_string());
        }
    }
    Err("entry not found in leo.toml".into())
}
