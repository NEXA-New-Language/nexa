use nexa::{compile_to_assembly, compile_to_binary};
use std::env;
use std::fs;
use std::process::{self, Command};

fn print_usage() {
    println!("NEXA — High-Performance Native Systems Programming Language");
    println!("\nUsage:");
    println!("  nexa run <file.nexa>            Compile and run natively instantly");
    println!("  nexa build <file.nexa> [-o out] Build standalone ELF binary");
    println!("  nexa asm <file.nexa>            Inspect generated x86-64 assembly");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let subcmd = &args[1];
    match subcmd.as_str() {
        "run" => {
            if args.len() < 3 {
                eprintln!("Error: Specify input file. Example: nexa run main.nexa");
                process::exit(1);
            }
            let file_path = &args[2];
            let source = fs::read_to_string(file_path).unwrap_or_else(|e| {
                eprintln!("Error reading '{}': {}", file_path, e);
                process::exit(1);
            });

            let temp_bin = "./__nexa_temp_bin";
            if let Err(diag) = compile_to_binary(file_path, &source, temp_bin) {
                eprint!("{}", diag.render(&source));
                process::exit(1);
            }

            // 直接ネイティブバイナリを実行！
            let status = Command::new(temp_bin).status();
            let _ = fs::remove_file(temp_bin);

            if let Ok(st) = status {
                if !st.success() {
                    process::exit(st.code().unwrap_or(1));
                }
            }
        }
        "build" => {
            if args.len() < 3 {
                eprintln!("Error: Specify input file. Example: nexa build main.nexa -o app");
                process::exit(1);
            }
            let file_path = &args[2];
            let mut output_path = "output".to_string();

            if args.len() >= 5 && args[3] == "-o" {
                output_path = args[4].clone();
            }

            let source = fs::read_to_string(file_path).unwrap_or_else(|e| {
                eprintln!("Error reading '{}': {}", file_path, e);
                process::exit(1);
            });

            match compile_to_binary(file_path, &source, &output_path) {
                Ok(_) => {
                    println!(" Build succeeded! Native executable created: ./{}\n", output_path);
                    println!("Run it with: ./{}", output_path);
                }
                Err(diag) => {
                    eprint!("{}", diag.render(&source));
                    process::exit(1);
                }
            }
        }
        "asm" => {
            if args.len() < 3 {
                eprintln!("Error: Specify input file.");
                process::exit(1);
            }
            let file_path = &args[2];
            let source = fs::read_to_string(file_path).unwrap_or_else(|e| {
                eprintln!("Error reading '{}': {}", file_path, e);
                process::exit(1);
            });

            match compile_to_assembly(file_path, &source) {
                Ok(asm) => println!("{}", asm),
                Err(diag) => {
                    eprint!("{}", diag.render(&source));
                    process::exit(1);
                }
            }
        }
        other => {
            // 引数がサブコマンドでなく直接ファイルパスだった場合は run を実行
            let file_path = other;
            let source = fs::read_to_string(file_path).unwrap_or_else(|e| {
                eprintln!("Error reading '{}': {}", file_path, e);
                process::exit(1);
            });

            let temp_bin = "./__nexa_temp_bin";
            if let Err(diag) = compile_to_binary(file_path, &source, temp_bin) {
                eprint!("{}", diag.render(&source));
                process::exit(1);
            }
            let _ = Command::new(temp_bin).status();
            let _ = fs::remove_file(temp_bin);
        }
    }
}
