use std::fs;
use std::path::Path;

/// Build project from leo.toml config
pub fn build() -> Result<String, String> {
    let entry = read_entry_from_toml()?;
    let source = fs::read_to_string(&entry)
        .map_err(|e| format!("read {} failed: {}", entry, e))?;
    let output = read_output_from_toml()?;

    let output_dir = Path::new(&output).parent().unwrap_or(Path::new("."));
    fs::create_dir_all(output_dir).map_err(|e| format!("create output dir failed: {}", e))?;

    let pipeline = crate::compiler::Pipeline::new(&source, &output);
    pipeline.compile().map_err(|e| format!("{}", e))?;
    Ok(output)
}

/// Read entry point from leo.toml
fn read_entry_from_toml() -> Result<String, String> {
    let content = fs::read_to_string("leo.toml")
        .map_err(|e| format!("read leo.toml failed: {}", e))?;
    extract_toml_value(&content, "entry")
}

/// Read output path from leo.toml
fn read_output_from_toml() -> Result<String, String> {
    let content = fs::read_to_string("leo.toml")
        .map_err(|e| format!("read leo.toml failed: {}", e))?;
    extract_toml_value(&content, "output")
}

/// Extract a key=value from TOML-like config
fn extract_toml_value(content: &str, key: &str) -> Result<String, String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(&format!("{} = ", key)) {
            let val = rest.trim().trim_matches('"').trim();
            return Ok(val.to_string());
        }
    }
    Err(format!("key '{}' not found in leo.toml", key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_toml_value() {
        let content = "[project]\nname = \"test\"\n\n[build]\nentry = \"src/main.leo\"\noutput = \"target/main\"\n";
        assert_eq!(extract_toml_value(content, "entry").unwrap(), "src/main.leo");
        assert_eq!(extract_toml_value(content, "output").unwrap(), "target/main");
        assert_eq!(extract_toml_value(content, "name").unwrap(), "test");
    }

    #[test]
    fn test_extract_missing_key() {
        let content = "[project]\nname = \"test\"\n";
        assert!(extract_toml_value(content, "entry").is_err());
    }
}
