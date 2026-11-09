use nexa::project::builder::{BuildMode, NativeBuilder};
use nexa::project::lockfile::Lockfile;
use nexa::project::manifest::Manifest;
use nexa::project::template::ProjectTemplate;
use std::fs;
use std::path::Path;

#[test]
fn test_project_lifecycle() {
    let test_dir = Path::new("target/test_app");
    let _ = fs::remove_dir_all(test_dir);

    ProjectTemplate::create_project(test_dir, "test_app").expect("create_project should succeed");
    assert!(test_dir.join("nexa.toml").exists());
    assert!(test_dir.join("src/main.nexa").exists());
    assert!(test_dir.join("tests/test_main.nexa").exists());

    let (manifest, _) = Manifest::load_from_dir(test_dir).expect("Manifest must be valid");
    assert_eq!(manifest.project.name, "test_app");
    assert_eq!(manifest.build.target, "native");

    let debug_res = NativeBuilder::build(test_dir, BuildMode::Debug).expect("Debug build should succeed");
    assert!(debug_res.binary_path.exists());
    assert!(debug_res.binary_path.to_string_lossy().contains("debug"));

    let lock_path = test_dir.join("nexa.lock");
    assert!(lock_path.exists());
    let lockfile = Lockfile::load(&lock_path).unwrap().expect("Lockfile must exist");
    assert_eq!(lockfile.packages.len(), 1);
    assert_eq!(lockfile.packages[0].name, "test_app");

    let release_res = NativeBuilder::build(test_dir, BuildMode::Release).expect("Release build should succeed");
    assert!(release_res.binary_path.exists());
    assert!(release_res.binary_path.to_string_lossy().contains("release"));

    let _ = fs::remove_dir_all(test_dir);
}
