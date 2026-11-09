use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub project: ProjectConfig,
    pub language: LanguageConfig,
    #[serde(default)]
    pub build: BuildConfig,
    #[serde(default)]
    pub dependencies: BTreeMap<String, DependencyConfig>,
    #[serde(default, rename = "dev-dependencies")]
    pub dev_dependencies: BTreeMap<String, DependencyConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectConfig {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanguageConfig {
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildConfig {
    #[serde(default = "default_entry")]
    pub entry: String,
    #[serde(default = "default_target")]
    pub target: String,
    #[serde(default = "default_optimization")]
    pub optimization: String,
    #[serde(default = "default_output")]
    pub output: String,
}

fn default_entry() -> String { "src/main.nexa".to_string() }
fn default_target() -> String { "native".to_string() }
fn default_optimization() -> String { "release".to_string() }
fn default_output() -> String { "build".to_string() }

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            entry: default_entry(),
            target: default_target(),
            optimization: default_optimization(),
            output: default_output(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum DependencyConfig {
    Simple(String),
    Detailed {
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        path: Option<String>,
    },
}

impl Manifest {
    pub fn load_from_dir(dir: &Path) -> Result<(Self, PathBuf), String> {
        let manifest_path = dir.join("nexa.toml");
        if !manifest_path.exists() {
            return Err(format!(
                "Could not find 'nexa.toml' in '{}'.\nHint: Run 'nexa init' or 'nexa new <name>' to create a project.",
                dir.display()
            ));
        }

        let content = fs::read_to_string(&manifest_path).map_err(|e| {
            format!("Failed to read '{}': {}", manifest_path.display(), e)
        })?;

        let manifest: Manifest = toml::from_str(&content).map_err(|e| {
            format!("Failed to parse '{}':\n{}", manifest_path.display(), e)
        })?;

        manifest.validate(&manifest_path)?;
        Ok((manifest, manifest_path))
    }

    pub fn validate(&self, path: &Path) -> Result<(), String> {
        let filename = path.to_string_lossy();

        if self.project.name.is_empty() {
            return Err(format!("Error: Invalid project configuration in {}\nProject name cannot be empty.", filename));
        }
        let first_char = self.project.name.chars().next().unwrap();
        if !first_char.is_ascii_alphabetic() {
            return Err(format!(
                "Error: Invalid project name '{}' in {}\nProject name must start with an ASCII letter (a-z, A-Z).\nHint: Use a name like 'my_app' or 'game_engine'.",
                self.project.name, filename
            ));
        }
        if !self.project.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(format!(
                "Error: Invalid project name '{}' in {}\nProject name can only contain alphanumeric characters, underscores, and hyphens.",
                self.project.name, filename
            ));
        }

        let parts: Vec<&str> = self.project.version.split('.').collect();
        if parts.len() < 2 || parts.len() > 3 || parts.iter().any(|p| p.parse::<u32>().is_err()) {
            return Err(format!(
                "Error: Invalid version format '{}' in {}\nVersion must be in SemVer format (e.g. '1.0.0' or '0.1').",
                self.project.version, filename
            ));
        }

        if self.build.target != "native" {
            return Err(format!(
                "Error: Unsupported build target '{}' in {}\nSupported targets: 'native' (Linux x86-64).\nNote: WebAssembly and other cross-compilation targets are planned for future versions.",
                self.build.target, filename
            ));
        }

        if self.build.optimization != "debug" && self.build.optimization != "release" {
            return Err(format!(
                "Error: Invalid optimization mode '{}' in {}\nValid options are 'debug' or 'release'.",
                self.build.optimization, filename
            ));
        }

        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let content = toml::to_string_pretty(self).map_err(|e| format!("Failed to serialize manifest: {}", e))?;
        fs::write(path, content).map_err(|e| format!("Failed to write manifest to '{}': {}", path.display(), e))?;
        Ok(())
    }
}
