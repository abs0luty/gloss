use std::fs;

use camino::Utf8PathBuf;
use gloss_core::{generate_for_project, BackendRegistry};
use tempfile::tempdir;

fn write_gleam_manifest(root: &Utf8PathBuf) {
    fs::write(
        root.join("gleam.toml"),
        r#"[project]
name = "app"
version = "1.0.0"

[dependencies]
"gleam/json" = "~> 1.0"
"#,
    )
    .expect("write gleam.toml");
}

#[test]
fn function_naming_overrides() {
    let temp = tempdir().expect("temp dir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf8 path");

    write_gleam_manifest(&root);

    fs::write(
        root.join("gloss.toml"),
        r#"
[fn_naming]
encoder_function_naming = "encode_{type_pascal}"
decoder_function_naming = "decode_{type_pascal}"
"#,
    )
    .expect("write global gloss.toml");

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");

    fs::write(
        src_dir.join("naming.gleam"),
        r#"
// gloss-file!: encoder_fn = "file_encode_{type}", decoder_fn = "file_decode_{type}"

// gloss!: encoder(json), decoder
pub type Alpha {
  Alpha(flag: Bool)
}

// gloss!: encoder(json), decoder, encoder_fn = "custom_encode_{type_snake}", decoder_fn = "custom_decode_{type_pascal}"
pub type Beta {
  Beta(value: Int)
}
"#,
    )
    .expect("write naming module");

    let registry = BackendRegistry::new();
    let generated = generate_for_project(&root, &registry).expect("generate project");

    let naming_path = src_dir.join("naming.gleam");
    let groups = generated
        .get(&naming_path)
        .expect("naming module generated");
    assert_eq!(groups.len(), 1);

    let decoder_code = groups[0].get_decoder_code(true, false);
    assert!(decoder_code.contains("pub fn file_decode_Alpha()"));
    assert!(decoder_code.contains("pub fn custom_decode_Beta()"));

    let encoder_code = groups[0].get_encoder_code(true, false);
    assert!(encoder_code.contains("pub fn file_encode_Alpha("));
    assert!(encoder_code.contains("pub fn custom_encode_beta("));
}
