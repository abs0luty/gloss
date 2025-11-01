use std::fs;

use camino::Utf8PathBuf;
use gloss_core::{generate_for_project, BackendRegistry, GlossError};
use tempfile::tempdir;

fn write_project_scaffold(root: &Utf8PathBuf, gleam_toml: &str) {
    fs::write(root.join("gleam.toml"), gleam_toml).expect("write gleam.toml");

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    fs::write(
        src_dir.join("sample.gleam"),
        r#"
// gloss!: encoder(json), decoder
pub type Sample {
  Sample(id: Int)
}
"#,
    )
    .expect("write sample module");
}

#[test]
fn missing_gleam_json_dependency_errors() {
    let temp = tempdir().expect("temp dir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf8 path");

    write_project_scaffold(
        &root,
        r#"[project]
name = "app"
version = "1.0.0"
"#,
    );

    let registry = BackendRegistry::new();
    let error =
        generate_for_project(&root, &registry).expect_err("expected dependency check error");

    match error {
        GlossError::GenerationError(message) => {
            assert!(
                message.contains("gleam/json"),
                "expected message to mention gleam/json, got: {message}"
            );
        }
        other => panic!("unexpected error kind: {other:?}"),
    }
}

#[test]
fn dev_dependency_is_accepted_for_encoders() {
    let temp = tempdir().expect("temp dir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf8 path");

    write_project_scaffold(
        &root,
        r#"[project]
name = "app"
version = "1.0.0"

[dev-dependencies]
"gleam/json" = "~> 1.0"
"#,
    );

    let registry = BackendRegistry::new();
    let generated =
        generate_for_project(&root, &registry).expect("expected encoders to generate successfully");

    assert!(
        !generated.is_empty(),
        "expected generated output when dependency is present"
    );
}
