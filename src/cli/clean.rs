use std::fs;

/// Remove target/ directory
pub fn clean() -> Result<(), String> {
    if fs::metadata("target").is_ok() {
        fs::remove_dir_all("target").map_err(|e| format!("remove target/ failed: {}", e))?;
        eprintln!("Cleaned target/");
    } else {
        eprintln!("No target/ directory found.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_clean_module_exists() {
        assert!(true);
    }
}
