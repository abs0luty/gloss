use std::fs;

use camino::Utf8PathBuf;
use gloss_core::{generate_for_project, BackendRegistry};
use tempfile::tempdir;

#[test]
fn absent_field_mode_maybe_absent_defaults_to_optional_field() {
    let temp = tempdir().expect("temp dir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf8 path");

    fs::write(
        root.join("gloss.toml"),
        r#"
absent_field_mode = "maybe_absent"
"#,
    )
    .expect("write root gloss.toml");

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");

    fs::write(
        src_dir.join("settings.gleam"),
        r#"
// gloss!: decoder
pub type Settings {
  Settings(threshold: Option(Int), label: String)
}
"#,
    )
    .expect("write settings module");

    let registry = BackendRegistry::new();
    let generated = generate_for_project(&root, &registry).expect("generate project");

    let settings_path = src_dir.join("settings.gleam");
    let groups = generated
        .get(&settings_path)
        .expect("settings module generated");
    assert_eq!(groups.len(), 1);

    let decoder_code = groups[0].get_decoder_code(true, false);
    assert!(decoder_code.contains("decode.optional_field(\"threshold\", option.None"));
}

#[test]
fn field_markers_override_absent_field_mode() {
    let temp = tempdir().expect("temp dir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf8 path");

    fs::write(
        root.join("gloss.toml"),
        r#"
absent_field_mode = "error_if_absent"
"#,
    )
    .expect("write root gloss.toml");

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");

    fs::write(
        src_dir.join("toggle.gleam"),
        r#"
// gloss!: decoder
pub type Toggle {
  Toggle(
    // gloss!: maybe_absent
    flag: Option(Bool),
    // gloss!: must_exist
    mode: Option(String),
  )
}

"#,
    )
    .expect("write toggle module");

    let registry = BackendRegistry::new();
    let generated = generate_for_project(&root, &registry).expect("generate project");

    let toggle_path = src_dir.join("toggle.gleam");
    let groups = generated
        .get(&toggle_path)
        .expect("toggle module generated");
    assert_eq!(groups.len(), 1);

    let decoder_code = groups[0].get_decoder_code(true, false);
    assert!(decoder_code.contains("decode.optional_field(\"flag\", option.None"));
    assert!(decoder_code.contains("decode.field(\"mode\""));
}
