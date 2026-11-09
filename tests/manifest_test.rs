use nexa::project::lockfile::Lockfile;
use nexa::project::manifest::Manifest;
use std::fs;
use std::path::Path;

#[test]
fn test_manifest_valid_and_defaults() {
    let toml_str = r#"
[project]
name = "valid_project"
version = "1.0.0"

[language]
version = "1.0"
"#;
    let manifest: Manifest = toml::from_str(toml_str).expect("Should parse");
    manifest.validate(Path::new("nexa.toml")).expect("Should be valid");

    // デフォルト値の検証
    assert_eq!(manifest.build.entry, "src/main.nexa");
    assert_eq!(manifest.build.target, "native");
    assert_eq!(manifest.build.optimization, "release");
    assert_eq!(manifest.build.output, "build");
}

#[test]
fn test_manifest_invalid_project_name_starts_with_number() {
    let toml_str = r#"
[project]
name = "123_invalid"
version = "1.0.0"

[language]
version = "1.0"
"#;
    let manifest: Manifest = toml::from_str(toml_str).expect("Should parse");
    let res = manifest.validate(Path::new("nexa.toml"));
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("must start with an ASCII letter"));
}

#[test]
fn test_manifest_invalid_version() {
    let toml_str = r#"
[project]
name = "my_app"
version = "invalid_version"

[language]
version = "1.0"
"#;
    let manifest: Manifest = toml::from_str(toml_str).expect("Should parse");
    let res = manifest.validate(Path::new("nexa.toml"));
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("Version must be in SemVer format"));
}

#[test]
fn test_manifest_unsupported_target() {
    let toml_str = r#"
[project]
name = "my_app"
version = "1.0.0"

[language]
version = "1.0"

[build]
target = "wasm32"
"#;
    let manifest: Manifest = toml::from_str(toml_str).expect("Should parse");
    let res = manifest.validate(Path::new("nexa.toml"));
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(err.contains("Unsupported build target 'wasm32'"));
}

#[test]
fn test_lockfile_deterministic_checksum() {
    let source1 = b"let a = 100\nprint(a)";
    let source2 = b"let a = 100\nprint(a)";
    let source3 = b"let a = 200\nprint(a)";

    let sum1 = Lockfile::compute_checksum(source1);
    let sum2 = Lockfile::compute_checksum(source2);
    let sum3 = Lockfile::compute_checksum(source3);

    // 同じソースなら常に完全に同一のチェックサムになる
    assert_eq!(sum1, sum2);
    // 内容が変わればチェックサムも異なる
    assert_ne!(sum1, sum3);
}
