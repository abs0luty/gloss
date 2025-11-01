use std::fs;

use camino::Utf8PathBuf;
use gloss_core::{generate_for_project, Config};
use tempfile::tempdir;

#[test]
fn function_naming_overrides() {
    let temp = tempdir().expect("temp dir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf8 path");

    fs::write(
        root.join("gloss.toml"),
        r#"
[fn_naming]
encoder_fn_pattern = "encode_{type_pascal}"
decoder_fn_pattern = "decode_{type_pascal}"
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

    let config = Config::load_or_default(&root);
    let generated = generate_for_project(&root, &config).expect("generate project");

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
