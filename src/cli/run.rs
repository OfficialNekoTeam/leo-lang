use std::fs;
use std::process;

/// Run a Leo file directly or build+run project
pub fn run(file: Option<&str>) -> Result<(), String> {
    match file {
        Some(path) => run_single_file(path),
        None => run_project(),
    }
}

/// Compile and run a single .leo file
fn run_single_file(path: &str) -> Result<(), String> {
    let source = fs::read_to_string(path)
        .map_err(|e| format!("read {} failed: {}", path, e))?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("system clock error: {}", e))?
        .subsec_nanos();
    let output = format!(
        "{}/leo_run_{}_{}",
        std::env::temp_dir().display(),
        std::process::id(),
        nanos
    );
    let pipeline = crate::compiler::Pipeline::new(&source, &output);
    pipeline.compile().map_err(|e| format!("{}", e))?;
    let status = process::Command::new(&output)
        .status()
        .map_err(|e| format!("run failed: {}", e))?;
    let _ = fs::remove_file(&output);
    if !status.success() {
        return Err("program exited with non-zero status".into());
    }
    Ok(())
}

/// Build project then run the output
fn run_project() -> Result<(), String> {
    let output = crate::cli::build::build()?;
    let status = process::Command::new(&output)
        .status()
        .map_err(|e| format!("run failed: {}", e))?;
    if !status.success() {
        return Err("program exited with non-zero status".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_run_module_exists() {
        // Just verify the module compiles
        assert!(true);
    }
}
