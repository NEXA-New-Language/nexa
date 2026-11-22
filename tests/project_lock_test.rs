use nexa::project::builder::{BuildMode, NativeBuilder};
use nexa::project::lockfile::Lockfile;
use nexa::project::manifest::Manifest;
use nexa::project::template::ProjectTemplate;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn temp_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!(
        "target/project-lock-tests/{}-{}-{}",
        name,
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn build_verifies_and_updates_lockfile_for_path_dependencies() {
    let root = temp_dir("dep_lock");
    let app = root.join("app");
    let lib = root.join("lib");
    ProjectTemplate::create_project(&app, "app").unwrap();
    ProjectTemplate::create_project(&lib, "lib").unwrap();

    let (mut manifest, path) = Manifest::load_from_dir(&app).unwrap();
    manifest.dependencies.insert(
        "lib".to_string(),
        nexa::project::manifest::DependencyConfig::Detailed {
            version: Some("1.0.0".to_string()),
            path: Some("../lib".to_string()),
        },
    );
    manifest.save(&path).unwrap();

    NativeBuilder::build(&app, BuildMode::Debug).unwrap();
    let lock = Lockfile::load(&app.join("nexa.lock")).unwrap().unwrap();
    assert!(lock.packages.iter().any(|p| p.name == "lib"));
    assert!(lock.packages.iter().any(|p| p.name == "app"));

    // Changing dependency source must invalidate the lock until update/build sync.
    fs::write(lib.join("src/main.nexa"), "print(\"changed\")\n").unwrap();
    let err = NativeBuilder::build(&app, BuildMode::Debug).unwrap_err();
    assert!(err.contains("out of date"), "unexpected error: {}", err);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn manifest_rejects_unsafe_paths() {
    let root = temp_dir("unsafe_paths");
    ProjectTemplate::create_project(&root, "unsafe").unwrap();
    let (mut manifest, path) = Manifest::load_from_dir(&root).unwrap();
    manifest.build.output = "../outside".to_string();
    manifest.save(&path).unwrap();
    let err = Manifest::load_from_dir(&root).unwrap_err();
    assert!(err.contains("build.output"));

    let content = fs::read_to_string(root.join("nexa.toml")).unwrap();
    let mut manifest: Manifest = toml::from_str(&content).unwrap();
    manifest.build.output = "build".to_string();
    manifest.build.entry = "/etc/passwd".to_string();
    manifest.save(&root.join("nexa.toml")).unwrap();
    let err = Manifest::load_from_dir(&root).unwrap_err();
    assert!(err.contains("build.entry"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn template_does_not_overwrite_existing_files() {
    let root = temp_dir("no_overwrite");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.nexa"), "// user work\n").unwrap();
    let err = ProjectTemplate::create_project(&root, "app").unwrap_err();
    assert!(err.contains("Refusing to overwrite"));
    assert_eq!(
        fs::read_to_string(root.join("src/main.nexa")).unwrap(),
        "// user work\n"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn transitive_path_dependencies_are_resolved() {
    let root = temp_dir("transitive");
    let app = root.join("app");
    let a = root.join("a");
    let b = root.join("b");
    ProjectTemplate::create_project(&app, "app").unwrap();
    ProjectTemplate::create_project(&a, "a").unwrap();
    ProjectTemplate::create_project(&b, "b").unwrap();

    let (mut ma, pa) = Manifest::load_from_dir(&a).unwrap();
    ma.dependencies.insert(
        "b".to_string(),
        nexa::project::manifest::DependencyConfig::Detailed {
            version: None,
            path: Some("../b".to_string()),
        },
    );
    ma.save(&pa).unwrap();

    let (mut mapp, papp) = Manifest::load_from_dir(&app).unwrap();
    mapp.dependencies.insert(
        "a".to_string(),
        nexa::project::manifest::DependencyConfig::Detailed {
            version: None,
            path: Some("../a".to_string()),
        },
    );
    mapp.save(&papp).unwrap();

    let resolved = nexa::project::dependency::DependencyManager::resolve_all(&mapp, &app).unwrap();
    let names: Vec<_> = resolved.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"a"));
    assert!(names.contains(&"b"));
    let _ = fs::remove_dir_all(&root);
}
