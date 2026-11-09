use crate::analyze_source;
use crate::project::builder::{BuildMode, NativeBuilder};
use crate::project::dependency::DependencyManager;
use crate::project::lockfile::Lockfile;
use crate::project::manifest::Manifest;
use crate::project::template::ProjectTemplate;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

pub struct CliHandler;

impl CliHandler {
    pub fn handle_new(name: &str) -> Result<(), String> {
        let target_dir = Path::new(name);
        if target_dir.exists() {
            return Err(format!("Error: Destination directory '{}' already exists.", name));
        }
        ProjectTemplate::create_project(target_dir, name)?;
        println!("     Created binary (native) `{}` project", name);
        println!("      To run: cd {} && nexa run", name);
        Ok(())
    }

    pub fn handle_init() -> Result<(), String> {
        let current_dir = env::current_dir().map_err(|e| e.to_string())?;
        let name = current_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("app");

        if current_dir.join("nexa.toml").exists() {
            return Err("Error: 'nexa.toml' already exists in current directory.".to_string());
        }
        ProjectTemplate::create_project(&current_dir, name)?;
        println!(" Initialized binary (native) `{}` in current directory", name);
        Ok(())
    }

    pub fn handle_build(release: bool) -> Result<(), String> {
        let project_dir = Self::find_project_root()?;
        let mode = if release { BuildMode::Release } else { BuildMode::Debug };

        let result = NativeBuilder::build(&project_dir, mode)?;

        let mode_str = match result.mode {
            BuildMode::Debug => "unoptimized + debuginfo",
            BuildMode::Release => "optimized [release]",
        };

        println!(
            "   Compiling `{}` ({})",
            result.binary_path.file_name().unwrap().to_string_lossy(),
            mode_str
        );
        println!(
            "    Finished {} target(s) in {:.2}s (Binary size: {} KB)",
            match result.mode {
                BuildMode::Debug => "dev",
                BuildMode::Release => "release",
            },
            result.duration.as_secs_f64(),
            result.binary_size / 1024
        );
        println!("      Executable: {}", result.binary_path.display());
        Ok(())
    }

    pub fn handle_run(release: bool) -> Result<(), String> {
        let project_dir = Self::find_project_root()?;
        let mode = if release { BuildMode::Release } else { BuildMode::Debug };

        let result = NativeBuilder::build(&project_dir, mode)?;

        println!(
            "     Running `{}`",
            result.binary_path.display()
        );

        let status: ExitStatus = Command::new(&result.binary_path)
            .status()
            .map_err(|e| format!("Failed to execute '{}': {}", result.binary_path.display(), e))?;

        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(())
    }

    pub fn handle_check() -> Result<(), String> {
        let project_dir = Self::find_project_root()?;
        let (manifest, _) = Manifest::load_from_dir(&project_dir)?;
        let entry_path = project_dir.join(&manifest.build.entry);

        if !entry_path.exists() {
            return Err(format!("Entry point '{}' not found.", entry_path.display()));
        }

        let source = fs::read_to_string(&entry_path).map_err(|e| e.to_string())?;

        println!("    Checking `{}` v{}", manifest.project.name, manifest.project.version);
        match analyze_source(entry_path.to_str().unwrap(), &source) {
            Ok(_) => {
                println!("    Finished check: no syntax or semantic errors found.");
                Ok(())
            }
            Err(diag) => Err(diag.render(&source)),
        }
    }

    pub fn handle_test() -> Result<(), String> {
        let project_dir = Self::find_project_root()?;
        let tests_dir = project_dir.join("tests");

        if !tests_dir.exists() {
            println!("No 'tests/' directory found.");
            return Ok(());
        }

        let mut test_files = Vec::new();
        for entry in fs::read_dir(&tests_dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("nexa") {
                test_files.push(path);
            }
        }

        if test_files.is_empty() {
            println!("No test files (*.nexa) found in 'tests/'.");
            return Ok(());
        }

        println!("    Running {} test(s)", test_files.len());
        let temp_bin = project_dir.join("build").join("__nexa_test_bin");

        let mut passed = 0;
        let mut failed = 0;

        for test_path in test_files {
            let test_name = test_path.file_name().unwrap().to_string_lossy();
            let source = fs::read_to_string(&test_path).map_err(|e| e.to_string())?;

            match crate::compile_to_binary(test_path.to_str().unwrap(), &source, temp_bin.to_str().unwrap()) {
                Ok(_) => {
                    let output = Command::new(&temp_bin).output();
                    let _ = fs::remove_file(&temp_bin);

                    match output {
                        Ok(out) if out.status.success() => {
                            println!("test {} ... ok", test_name);
                            passed += 1;
                        }
                        Ok(out) => {
                            println!("test {} ... FAILED (Exit code: {:?})", test_name, out.status.code());
                            failed += 1;
                        }
                        Err(e) => {
                            println!("test {} ... FAILED to execute: {}", test_name, e);
                            failed += 1;
                        }
                    }
                }
                Err(diag) => {
                    println!("test {} ... COMPILATION FAILED:\n{}", test_name, diag.render(&source));
                    failed += 1;
                }
            }
        }

        println!("\ntest result: {}. {} passed; {} failed", if failed == 0 { "ok" } else { "FAILED" }, passed, failed);
        if failed > 0 {
            std::process::exit(1);
        }
        Ok(())
    }

    pub fn handle_clean() -> Result<(), String> {
        let project_dir = Self::find_project_root()?;
        let (manifest, _) = Manifest::load_from_dir(&project_dir)?;
        let build_dir = project_dir.join(&manifest.build.output);

        if build_dir.exists() {
            fs::remove_dir_all(&build_dir).map_err(|e| e.to_string())?;
            println!("     Removed build directory: {}", build_dir.display());
        } else {
            println!("     Build directory is already clean.");
        }
        Ok(())
    }

    pub fn handle_add(name: &str, path_opt: Option<&str>) -> Result<(), String> {
        let project_dir = Self::find_project_root()?;
        let (mut manifest, manifest_path) = Manifest::load_from_dir(&project_dir)?;

        DependencyManager::add_dependency(&mut manifest, name, path_opt);
        manifest.save(&manifest_path)?;

        println!("      Adding `{}` to dependencies in nexa.toml", name);
        Self::handle_update()?;
        Ok(())
    }

    pub fn handle_remove(name: &str) -> Result<(), String> {
        let project_dir = Self::find_project_root()?;
        let (mut manifest, manifest_path) = Manifest::load_from_dir(&project_dir)?;

        if DependencyManager::remove_dependency(&mut manifest, name) {
            manifest.save(&manifest_path)?;
            println!("    Removing `{}` from dependencies in nexa.toml", name);
            Self::handle_update()?;
        } else {
            println!("   Dependency `{}` was not found in nexa.toml", name);
        }
        Ok(())
    }

    pub fn handle_update() -> Result<(), String> {
        let project_dir = Self::find_project_root()?;
        let (manifest, _) = Manifest::load_from_dir(&project_dir)?;
        let lock_path = project_dir.join("nexa.lock");

        let mut lockfile = Lockfile::new();
        let resolved = DependencyManager::resolve_all(&manifest, &project_dir)?;

        for dep in resolved {
            let dep_entry = dep.path.join("src/main.nexa");
            let checksum = if dep_entry.exists() {
                let content = fs::read(&dep_entry).unwrap_or_default();
                Lockfile::compute_checksum(&content)
            } else {
                "0000000000000000".to_string()
            };

            lockfile.packages.push(crate::project::lockfile::LockedPackage {
                name: dep.name,
                version: dep.version,
                source: dep.source_desc,
                checksum,
                dependencies: Vec::new(),
            });
        }

        lockfile.save(&lock_path)?;
        println!("     Updated nexa.lock");
        Ok(())
    }

    fn find_project_root() -> Result<PathBuf, String> {
        let mut curr = env::current_dir().map_err(|e| e.to_string())?;
        loop {
            if curr.join("nexa.toml").exists() {
                return Ok(curr);
            }
            if !curr.pop() {
                break;
            }
        }
        Err("Error: Could not find 'nexa.toml' in current directory or any parent directory.\nHint: Run 'nexa init' to create a project here.".to_string())
    }
}
