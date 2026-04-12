use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let cmd = &args[1];
    let result = match cmd.as_str() {
        "init" => {
            let name = args.get(2).map(|s| s.as_str());
            leo::cli::init::init(name)
        }
        "build" => leo::cli::build::build().map(|_| ()),
        "run" => {
            let file = args.get(2).map(|s| s.as_str());
            leo::cli::run::run(file)
        }
        "check" => leo::cli::check::check(),
        "clean" => leo::cli::clean::clean(),
        // Legacy: direct file compilation
        _ => compile_file(&args),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

/// Print usage help
fn print_usage() {
    eprintln!("Leo Programming Language Compiler");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  leo init [name]    Initialize a new project");
    eprintln!("  leo build          Build the project");
    eprintln!("  leo run [file]     Build and run");
    eprintln!("  leo check          Type-check only");
    eprintln!("  leo clean          Remove target/");
    eprintln!("  leo <file.leo>     Compile a single file");
}

/// Legacy single-file compilation mode
fn compile_file(args: &[String]) -> Result<(), String> {
    let source_path = &args[1];
    let output = extract_output(args).unwrap_or_else(|| "a.out".to_string());
    let source = std::fs::read_to_string(source_path)
        .map_err(|e| format!("read {} failed: {}", source_path, e))?;
    let pipeline = leo::compiler::Pipeline::new(&source, &output);
    pipeline.compile().map_err(|e| format!("{}", e))?;
    eprintln!("Compiled: {}", output);
    Ok(())
}

/// Extract -o flag from args
fn extract_output(args: &[String]) -> Option<String> {
    for i in 0..args.len() {
        if args[i] == "-o" && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
    }
    None
}
