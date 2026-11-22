use crate::project::manifest::{DependencyConfig, Manifest};
use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ResolvedDependency {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub source_desc: String,
    pub entry: PathBuf,
    pub dependencies: Vec<String>,
}

pub struct DependencyManager;

impl DependencyManager {
    /// Resolve local path dependencies, including transitive dependencies.
    /// Remote registry dependencies are intentionally unsupported in 1.0 and
    /// produce an actionable error rather than being silently ignored.
    pub fn resolve_all(
        manifest: &Manifest,
        project_dir: &Path,
    ) -> Result<Vec<ResolvedDependency>, String> {
        let root = normalize(project_dir);
        let mut resolved = BTreeMap::new();
        let mut visiting = HashSet::new();
        for (name, dep) in &manifest.dependencies {
            Self::resolve_recursive(name, dep, project_dir, &root, &mut visiting, &mut resolved)?;
        }
        Ok(resolved.into_values().collect())
    }

    fn resolve_recursive(
        name: &str,
        dep: &DependencyConfig,
        dir: &Path,
        root: &Path,
        visiting: &mut HashSet<String>,
        resolved: &mut BTreeMap<String, ResolvedDependency>,
    ) -> Result<(), String> {
        if visiting.contains(name) {
            return Err(format!(
                "Error: Dependency cycle detected involving '{}'.",
                name
            ));
        }
        let (res, dep_manifest) = Self::resolve_single(name, dep, dir, root)?;
        if let Some(existing) = resolved.get(name) {
            if existing.version != res.version || existing.path != res.path {
                return Err(format!(
                    "Error: Conflicting resolutions for dependency '{}': {} ({}) and {} ({}).",
                    name,
                    existing.version,
                    existing.path.display(),
                    res.version,
                    res.path.display()
                ));
            }
            return Ok(());
        }
        visiting.insert(name.to_string());
        for (child_name, child_dep) in &dep_manifest.dependencies {
            Self::resolve_recursive(child_name, child_dep, &res.path, root, visiting, resolved)?;
        }
        visiting.remove(name);
        resolved.insert(name.to_string(), res);
        Ok(())
    }

    fn resolve_single(
        name: &str,
        dep: &DependencyConfig,
        dir: &Path,
        root: &Path,
    ) -> Result<(ResolvedDependency, Manifest), String> {
        match dep {
            DependencyConfig::Detailed { path: Some(rel_path), version } => {
                if rel_path.is_empty() {
                    return Err(format!("Error: Dependency '{}' has an empty path.", name));
                }
                let dep_dir = normalize(&dir.join(rel_path));
                if !dep_dir.is_dir() {
                    return Err(format!(
                        "Error: Dependency path '{}' does not exist (declared in dependency '{}').",
                        dep_dir.display(), name
                    ));
                }
                let (dep_manifest, _) = Manifest::load_from_dir(&dep_dir)
                    .map_err(|e| format!("Error resolving dependency '{}':\n{}", name, e))?;
                if dep_manifest.project.name != name {
                    return Err(format!(
                        "Error: Dependency '{}' resolves to project named '{}'.",
                        name, dep_manifest.project.name
                    ));
                }
                if let Some(required) = version {
                    if required != &dep_manifest.project.version {
                        return Err(format!(
                            "Error: Dependency '{}' requires version '{}', but found '{}'.",
                            name, required, dep_manifest.project.version
                        ));
                    }
                }
                let entry = dep_dir.join(&dep_manifest.build.entry);
                if !entry.is_file() {
                    return Err(format!(
                        "Error: Dependency '{}' entry point '{}' does not exist.",
                        name, entry.display()
                    ));
                }
                let rel = relative_path(root, &dep_dir);
                Ok((
                    ResolvedDependency {
                        name: name.to_string(),
                        version: dep_manifest.project.version.clone(),
                        path: dep_dir,
                        source_desc: format!("path:{}", rel.display()),
                        entry,
                        dependencies: dep_manifest.dependencies.keys().cloned().collect(),
                    },
                    dep_manifest,
                ))
            }
            DependencyConfig::Simple(ver) => Err(format!(
                "Error: Remote registry package '{}:{}' cannot be resolved.\nHint: Remote Package Registry is not available in NEXA 1.0. Use a local path dependency:\n  {} = {{ path = \"../{}\" }}",
                name, ver, name, name
            )),
            _ => Err(format!(
                "Error: Invalid dependency specification for '{}'. Specify a 'path = \"...\"'.",
                name
            )),
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

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn relative_path(from: &Path, to: &Path) -> PathBuf {
    let from = normalize(from);
    let to = normalize(to);
    let fc: Vec<_> = from.components().collect();
    let tc: Vec<_> = to.components().collect();
    let common = fc.iter().zip(tc.iter()).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    for _ in common..fc.len() {
        out.push("..");
    }
    for comp in &tc[common..] {
        out.push(comp.as_os_str());
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}
