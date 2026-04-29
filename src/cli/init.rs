use std::fs;
use std::path::Path;

use crate::cli::security::escape_toml_string;

/// Initialize a new Leo project
pub fn init(name: Option<&str>) -> Result<(), String> {
    let base = match name {
        Some(n) => n.to_string(),
        None => ".".to_string(),
    };
    let base_path = Path::new(&base);

    if name.is_some() {
        fs::create_dir_all(base_path).map_err(|e| format!("create dir failed: {}", e))?;
    }

    let src_dir = base_path.join("src");
    fs::create_dir_all(&src_dir).map_err(|e| format!("create src/ failed: {}", e))?;

    let toml_path = base_path.join("leo.toml");
    if toml_path.exists() {
        return Err("leo.toml already exists".into());
    }
    let project_name = base_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("leo-project");
    let project_name = escape_toml_string(project_name)?;
    let toml_content = format!(
        "[project]\nname = \"{}\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.leo\"\noutput = \"target/main\"\n",
        project_name
    );
    fs::write(&toml_path, toml_content).map_err(|e| format!("write leo.toml failed: {}", e))?;

    let main_path = src_dir.join("main.leo");
    if main_path.exists() {
        return Err("src/main.leo already exists".into());
    }
    let main_content = "fn main() {\n    \"Hello, Leo!\"\n}\n";
    fs::write(&main_path, main_content).map_err(|e| format!("write main.leo failed: {}", e))?;

    eprintln!("Initialized Leo project in {}", base_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_new_project() {
        let dir = std::env::temp_dir().join("leo_test_init_new");
        let _ = fs::remove_dir_all(&dir);
        init(Some(dir.to_str().unwrap())).unwrap();
        assert!(dir.join("leo.toml").exists());
        assert!(dir.join("src/main.leo").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_init_duplicate_fails() {
        let dir = std::env::temp_dir().join("leo_test_init_dup");
        let _ = fs::remove_dir_all(&dir);
        init(Some(dir.to_str().unwrap())).unwrap();
        let result = init(Some(dir.to_str().unwrap()));
        assert!(result.is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
