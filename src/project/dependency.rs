use crate::project::manifest::{DependencyConfig, Manifest};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ResolvedDependency {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub source_desc: String,
}

pub struct DependencyManager;

impl DependencyManager {
    pub fn resolve_all(
        manifest: &Manifest,
        project_dir: &Path,
    ) -> Result<Vec<ResolvedDependency>, String> {
        let mut resolved = Vec::new();

        for (name, dep) in &manifest.dependencies {
            let res = Self::resolve_single(name, dep, project_dir)?;
            resolved.push(res);
        }

        Ok(resolved)
    }

    fn resolve_single(
        name: &str,
        dep: &DependencyConfig,
        project_dir: &Path,
    ) -> Result<ResolvedDependency, String> {
        match dep {
            DependencyConfig::Detailed { path: Some(rel_path), .. } => {
                let dep_dir = project_dir.join(rel_path);
                if !dep_dir.exists() {
                    return Err(format!(
                        "Error: Dependency path '{}' does not exist (declared in dependency '{}').",
                        dep_dir.display(),
                        name
                    ));
                }
                let (dep_manifest, _) = Manifest::load_from_dir(&dep_dir).map_err(|e| {
                    format!("Error resolving dependency '{}':\n{}", name, e)
                })?;

                Ok(ResolvedDependency {
                    name: name.to_string(),
                    version: dep_manifest.project.version,
                    path: dep_dir,
                    source_desc: format!("path:{}", rel_path),
                })
            }
            DependencyConfig::Simple(ver) => {
                Err(format!(
                    "Error: Remote registry package '{}:{}' cannot be resolved.\nHint: Remote Package Registry is under development. Please use local path dependency for now:\n  {} = {{ path = \"../{}\" }}",
                    name, ver, name, name
                ))
            }
            _ => {
                Err(format!(
                    "Error: Invalid dependency specification for '{}'. Specify a 'path = \"...\"'.",
                    name
                ))
            }
        }
    }

    pub fn add_dependency(manifest: &mut Manifest, name: &str, path_opt: Option<&str>) {
        if let Some(path) = path_opt {
            manifest.dependencies.insert(
                name.to_string(),
                DependencyConfig::Detailed {
                    version: None,
                    path: Some(path.to_string()),
                },
            );
        } else {
            manifest.dependencies.insert(
                name.to_string(),
                DependencyConfig::Simple("1.0.0".to_string()),
            );
        }
    }

    pub fn remove_dependency(manifest: &mut Manifest, name: &str) -> bool {
        manifest.dependencies.remove(name).is_some()
    }
}
