use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: leo <source.leo> [-o output]");
        process::exit(1);
    }

    let source_path = &args[1];
    let output_path = extract_output(&args).unwrap_or_else(|| "a.out".to_string());

    let source = match fs::read_to_string(source_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", source_path, e);
            process::exit(1);
        }
    };

    let pipeline = leo::compiler::Pipeline::new(&source, &output_path);
    if let Err(e) = pipeline.compile() {
        eprintln!("Compilation error: {}", e);
        process::exit(1);
    }

    eprintln!("Compilation successful: {}", output_path);
}

/// Extract -o flag value from args
fn extract_output(args: &[String]) -> Option<String> {
    for i in 0..args.len() {
        if args[i] == "-o" && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
    }
    None
}
