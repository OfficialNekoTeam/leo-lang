use std::fs;
use std::process;

use crate::cli::security::{run_checked, temp_run_output_path};

/// Run a Leo file directly or build+run project
pub fn run(file: Option<&str>) -> Result<(), String> {
    match file {
        Some(path) => run_single_file(path),
        None => run_project(),
    }
}

/// Compile and run a single .leo file
fn run_single_file(path: &str) -> Result<(), String> {
    let source = fs::read_to_string(path).map_err(|e| format!("read {} failed: {}", path, e))?;
    let output = temp_run_output_path()?;
    let output_str = output.to_string_lossy().to_string();
    let pipeline = crate::compiler::Pipeline::new(&source, &output_str);
    pipeline.compile().map_err(|e| format!("{}", e))?;
    let mut command = process::Command::new(&output_str);
    let result = run_checked(&mut command, "program");
    let _ = fs::remove_file(&output);
    result?;
    Ok(())
}

/// Build project then run the output
fn run_project() -> Result<(), String> {
    let output = crate::cli::build::build()?;
    let mut command = process::Command::new(&output);
    run_checked(&mut command, "program")
}
