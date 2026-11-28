use crate::compile_to_assembly;
use crate::project::dependency::DependencyManager;
use crate::project::lockfile::{LockedPackage, Lockfile};
use crate::project::manifest::Manifest;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMode {
    Debug,
    Release,
}

#[derive(Debug)]
pub struct BuildResult {
    pub binary_path: PathBuf,
    pub mode: BuildMode,
    pub duration: std::time::Duration,
    pub binary_size: u64,
}

pub struct NativeBuilder;

impl NativeBuilder {
    pub fn build(project_dir: &Path, mode: BuildMode) -> Result<BuildResult, String> {
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
            format!(
                "Failed to read source file '{}': {}",
                entry_path.display(),
                e
            )
        })?;

        let resolved = DependencyManager::resolve_all(&manifest, project_dir)?;
        let lock_path = project_dir.join("nexa.lock");
        let mut lockfile = Lockfile::load(&lock_path)?.unwrap_or_else(Lockfile::new);
        let expected_deps = locked_dependencies(&resolved)?;
        if lockfile.packages.is_empty() {
            lockfile.packages.extend(expected_deps.clone());
        } else {
            lockfile.verify(&expected_deps)?;
        }

        let mode_subdir = match mode {
            BuildMode::Debug => "debug",
            BuildMode::Release => "release",
        };
        let out_dir = project_dir.join(&manifest.build.output).join(mode_subdir);
        fs::create_dir_all(&out_dir).map_err(|e| {
            format!(
                "Failed to create output directory '{}': {}",
                out_dir.display(),
                e
            )
        })?;

        let binary_path = out_dir.join(&manifest.project.name);
        let temp_asm_path = out_dir.join(format!("{}.s", manifest.project.name));

        let asm_code = compile_to_assembly(entry_path.to_str().unwrap(), &source)
            .map_err(|diag| diag.render(&source))?;

        fs::write(&temp_asm_path, &asm_code).map_err(|e| {
            format!(
                "Failed to write assembly to '{}': {}",
                temp_asm_path.display(),
                e
            )
        })?;

        let extra_cc_args: &[&str] = match mode {
            BuildMode::Debug => &["-g", "-O0"],
            BuildMode::Release => &["-O3", "-s"],
        };
        let ui = crate::source_has_ui_app(&source);
        let link_result =
            crate::link_native_binary(&temp_asm_path, &binary_path, ui, extra_cc_args);
        let _ = fs::remove_file(&temp_asm_path);
        link_result.map_err(|e| format!("Linker Error: {}", e))?;

        let checksum = Lockfile::compute_checksum(source.as_bytes());

        lockfile
            .packages
            .retain(|p| p.name != manifest.project.name);
        lockfile.packages.push(LockedPackage {
            name: manifest.project.name.clone(),
            version: manifest.project.version.clone(),
            source: "root".to_string(),
            checksum,
            dependencies: manifest.dependencies.keys().cloned().collect(),
        });
        lockfile.packages.sort_by(|a, b| a.name.cmp(&b.name));
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

pub(crate) fn locked_dependencies(
    resolved: &[crate::project::dependency::ResolvedDependency],
) -> Result<Vec<LockedPackage>, String> {
    let mut packages = Vec::new();
    for dep in resolved {
        let content = fs::read(&dep.entry).map_err(|e| {
            format!(
                "Failed to read dependency entry '{}': {}",
                dep.entry.display(),
                e
            )
        })?;
        packages.push(LockedPackage {
            name: dep.name.clone(),
            version: dep.version.clone(),
            source: dep.source_desc.clone(),
            checksum: Lockfile::compute_checksum(&content),
            dependencies: dep.dependencies.clone(),
        });
    }
    Ok(packages)
}
