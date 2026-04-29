use std::path::PathBuf;
use std::process::{Command, ExitStatus};

/// Validate a project-local path from leo.toml.
pub(crate) fn validate_project_path(path: &str, key: &str) -> Result<String, String> {
    if path.contains('\0') {
        return Err(format!("leo.toml: '{}' contains null byte", key));
    }
    if path.contains("..") || path.starts_with('/') || path.starts_with('\\') {
        return Err(format!(
            "leo.toml: '{}' contains unsafe path: {}",
            key, path
        ));
    }
    if path.starts_with('-') {
        return Err(format!(
            "leo.toml: '{}' starts with '-', which would be interpreted as a compiler flag: {}",
            key, path
        ));
    }
    Ok(path.to_string())
}

/// Escape a TOML basic string value.
pub(crate) fn escape_toml_string(value: &str) -> Result<String, String> {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c.is_control() => {
                return Err("project name contains unsupported control character".into())
            }
            c => escaped.push(c),
        }
    }
    Ok(escaped)
}

/// Build a non-fixed temp output path for `leo run`.
pub(crate) fn temp_run_output_path() -> Result<PathBuf, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("system clock error: {}", e))?
        .as_nanos();
    Ok(std::env::temp_dir().join(format!("leo_run_{}_{}", std::process::id(), nanos)))
}

/// Return the linker program. `LEO_LINKER` is an explicit override; default is clang.
pub(crate) fn linker_program() -> Result<String, String> {
    let linker = std::env::var("LEO_LINKER").unwrap_or_else(|_| "clang".to_string());
    if linker.is_empty() || linker.contains('\0') {
        return Err("invalid linker program".into());
    }
    Ok(linker)
}

/// Check a child process exit status and preserve the action context.
pub(crate) fn check_status(status: ExitStatus, action: &str) -> Result<(), String> {
    if status.success() {
        Ok(())
    } else {
        Err(match status.code() {
            Some(code) => format!("{} failed with exit code {}", action, code),
            None => format!("{} terminated by signal", action),
        })
    }
}

/// Run a command without shell expansion and require a zero exit status.
pub(crate) fn run_checked(command: &mut Command, action: &str) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|e| format!("{} failed to start: {}", action, e))?;
    check_status(status, action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_project_path() {
        assert_eq!(
            validate_project_path("src/main.leo", "entry").unwrap(),
            "src/main.leo"
        );
        assert!(validate_project_path("../main.leo", "entry").is_err());
        assert!(validate_project_path("/tmp/main", "output").is_err());
        assert!(validate_project_path("-bad", "output").is_err());
        assert!(validate_project_path("bad\0path", "entry").is_err());
    }

    #[test]
    fn test_escape_toml_string() {
        assert_eq!(escape_toml_string("leo").unwrap(), "leo");
        assert_eq!(escape_toml_string("a\"b").unwrap(), "a\\\"b");
        assert_eq!(escape_toml_string("a\\b").unwrap(), "a\\\\b");
        assert_eq!(escape_toml_string("a\nb").unwrap(), "a\\nb");
    }

    #[test]
    fn test_temp_run_output_path_not_fixed() {
        let path = temp_run_output_path().unwrap();
        let s = path.to_string_lossy();
        assert!(s.contains("leo_run_"));
        assert!(!s.ends_with("/leo_run_tmp"));
    }
}
