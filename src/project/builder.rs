use crate::compile_to_assembly;
use crate::project::lockfile::{LockedPackage, Lockfile};
use crate::project::manifest::Manifest;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMode {
    Debug,
    Release,
}

pub struct BuildResult {
    pub binary_path: PathBuf,
    pub mode: BuildMode,
    pub duration: std::time::Duration,
    pub binary_size: u64,
}

pub struct NativeBuilder;

impl NativeBuilder {
    pub fn build(
        project_dir: &Path,
        mode: BuildMode,
    ) -> Result<BuildResult, String> {
        let start_time = Instant::now();

        let (manifest, _) = Manifest::load_from_dir(project_dir)?;
        let entry_path = project_dir.join(&manifest.build.entry);
        if !entry_path.exists() {
            return Err(format!(
                "Error: Entry point '{}' does not exist.\nHint: Create '{}' or specify a valid entry in nexa.toml [build.entry].",
                entry_path.display(),
                manifest.build.entry
            ));
        }

        let source = fs::read_to_string(&entry_path).map_err(|e| {
            format!("Failed to read source file '{}': {}", entry_path.display(), e)
        })?;

        let mode_subdir = match mode {
            BuildMode::Debug => "debug",
            BuildMode::Release => "release",
        };
        let out_dir = project_dir.join(&manifest.build.output).join(mode_subdir);
        fs::create_dir_all(&out_dir).map_err(|e| {
            format!("Failed to create output directory '{}': {}", out_dir.display(), e)
        })?;

        let binary_path = out_dir.join(&manifest.project.name);
        let temp_asm_path = out_dir.join(format!("{}.s", manifest.project.name));

        let asm_code = compile_to_assembly(entry_path.to_str().unwrap(), &source)
            .map_err(|diag| diag.render(&source))?;

        fs::write(&temp_asm_path, &asm_code).map_err(|e| {
            format!("Failed to write assembly to '{}': {}", temp_asm_path.display(), e)
        })?;

        let mut cc_args = vec!["-no-pie"];
        match mode {
            BuildMode::Debug => {
                cc_args.push("-g");
                cc_args.push("-O0");
            }
            BuildMode::Release => {
                cc_args.push("-O3");
                cc_args.push("-s");
            }
        }
        let temp_asm_str = temp_asm_path.to_str().unwrap();
        let binary_str = binary_path.to_str().unwrap();
        cc_args.extend(["-o", binary_str, temp_asm_str]);

        let status = Command::new("cc")
            .args(&cc_args)
            .status()
            .map_err(|e| format!("Failed to invoke system linker 'cc': {}", e))?;

        let _ = fs::remove_file(&temp_asm_path);

        if !status.success() {
            return Err("Linker Error: Failed to generate native executable.".to_string());
        }

        let lock_path = project_dir.join("nexa.lock");
        let mut lockfile = Lockfile::load(&lock_path)?.unwrap_or_else(Lockfile::new);
        let checksum = Lockfile::compute_checksum(source.as_bytes());

        lockfile.packages.retain(|p| p.name != manifest.project.name);
        lockfile.packages.push(LockedPackage {
            name: manifest.project.name.clone(),
            version: manifest.project.version.clone(),
            source: "root".to_string(),
            checksum,
            dependencies: manifest.dependencies.keys().cloned().collect(),
        });
        lockfile.save(&lock_path)?;

        let metadata = fs::metadata(&binary_path).map_err(|e| e.to_string())?;
        let duration = start_time.elapsed();

        Ok(BuildResult {
            binary_path,
            mode,
            duration,
            binary_size: metadata.len(),
        })
    }
}
