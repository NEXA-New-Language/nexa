use nexa::parse_source;
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: nexa <file.nexa>");
        process::exit(1);
    }

    let file_path = &args[1];
    let source = match fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: Cannot read file '{}': {}", file_path, e);
            process::exit(1);
        }
    };

    match parse_source(file_path, &source) {
        Ok(program) => {
            println!(" Successfully parsed NEXA program!");
            println!("AST Statement count: {}", program.statements.len());
        }
        Err(diagnostic) => {
            eprint!("{}", diagnostic.render(&source));
            process::exit(1);
        }
    }
}
