use nexa::cli::CliHandler;
use std::env;
use std::process;

fn print_usage() {
    println!("NEXA Project System 1.0 — High-Performance Native Systems Programming Language");
    println!("\nUSAGE:");
    println!("  nexa <COMMAND> [OPTIONS]\n");
    println!("COMMANDS:");
    println!("  new <name>       Create a new NEXA project in a new directory");
    println!("  init             Initialize a NEXA project in current directory");
    println!("  build            Compile NEXA project into native machine code (debug)");
    println!("  build --release  Compile with full optimizations (release)");
    println!("  run              Build and execute native binary");
    println!("  run --release    Build and execute release binary");
    println!("  check            Analyze syntax and semantics without building binary");
    println!("  test             Execute all tests in tests/");
    println!("  add <package>    Add a dependency to nexa.toml");
    println!("  remove <package> Remove a dependency from nexa.toml");
    println!("  update           Update dependencies and sync nexa.lock");
    println!("  clean            Remove build artifacts");
    println!("  --version, -V    Show NEXA version information");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let cmd = args[1].as_str();
    let result = match cmd {
        "new" => {
            if args.len() < 3 {
                eprintln!("Error: Project name required. Example: nexa new MyApp");
                process::exit(1);
            }
            CliHandler::handle_new(&args[2])
        }
        "init" => CliHandler::handle_init(),
        "build" => {
            let release = args.iter().any(|a| a == "--release");
            CliHandler::handle_build(release)
        }
        "run" => {
            let release = args.iter().any(|a| a == "--release");
            if args.len() >= 3 && args[2].ends_with(".nexa") {
                let file_path = &args[2];
                let source = std::fs::read_to_string(file_path).unwrap_or_else(|e| {
                    eprintln!("Error reading '{}': {}", file_path, e);
                    process::exit(1);
                });
                let temp_bin = "./__nexa_temp_bin";
                if let Err(diag) = nexa::compile_to_binary(file_path, &source, temp_bin) {
                    eprint!("{}", diag.render(&source));
                    process::exit(1);
                }
                let st = std::process::Command::new(temp_bin).status();
                let _ = std::fs::remove_file(temp_bin);
                if let Ok(code) = st {
                    process::exit(code.code().unwrap_or(0));
                }
                return;
            }
            CliHandler::handle_run(release)
        }
        "check" => CliHandler::handle_check(),
        "test" => CliHandler::handle_test(),
        "add" => {
            if args.len() < 3 {
                eprintln!("Error: Package name required. Example: nexa add helper --path ../helper");
                process::exit(1);
            }
            let name = &args[2];
            let path_opt = if args.len() >= 5 && args[3] == "--path" {
                Some(args[4].as_str())
            } else {
                None
            };
            CliHandler::handle_add(name, path_opt)
        }
        "remove" => {
            if args.len() < 3 {
                eprintln!("Error: Package name required. Example: nexa remove helper");
                process::exit(1);
            }
            CliHandler::handle_remove(&args[2])
        }
        "update" => CliHandler::handle_update(),
        "clean" => CliHandler::handle_clean(),
        "--version" | "-V" => {
            println!("nexa 1.0.0 (project system 1.0, native x86_64-linux)");
            Ok(())
        }
        other => {
            if other.ends_with(".nexa") {
                let source = std::fs::read_to_string(other).unwrap_or_else(|e| {
                    eprintln!("Error reading '{}': {}", other, e);
                    process::exit(1);
                });
                let temp_bin = "./__nexa_temp_bin";
                if let Err(diag) = nexa::compile_to_binary(other, &source, temp_bin) {
                    eprint!("{}", diag.render(&source));
                    process::exit(1);
                }
                let _ = std::process::Command::new(temp_bin).status();
                let _ = std::fs::remove_file(temp_bin);
                return;
            }
            eprintln!("Error: Unknown command '{}'.\n", other);
            print_usage();
            process::exit(1);
        }
    };

    if let Err(err_msg) = result {
        eprintln!("{}", err_msg);
        process::exit(1);
    }
}
