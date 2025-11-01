use std::fs;

use camino::Utf8PathBuf;
use gloss_core::{generate_for_project, BackendRegistry, EncoderBackend, EncoderType};
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn custom_encoder_backend_is_used() {
    let temp = tempdir().expect("temp dir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf8 path");

    fs::write(
        root.join("gleam.toml"),
        r#"[project]
name = "app"
version = "1.0.0"

[dependencies]
"gleam/json" = "~> 1.0"
"#,
    )
    .expect("write gleam manifest");

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("create src directory");

    fs::write(
        src_dir.join("entry.gleam"),
        r#"
// gloss!: encoder(json), decoder
pub type Entry {
  Entry(id: Int, name: String)
}
"#,
    )
    .expect("write module");

    let registry = BackendRegistry::new().with_backend(
        EncoderType::Json,
        Arc::new(CustomJsonBackend) as gloss_core::EncoderBackendRef,
    );
    let generated = generate_for_project(&root, &registry).expect("generate project");

    let entry_path = src_dir.join("entry.gleam");
    let groups = generated.get(&entry_path).expect("entry module generated");
    assert_eq!(groups.len(), 1);

    let encoder_code = groups[0].get_encoder_code(true, false);
    assert!(encoder_code.contains("import custom/json as cj"));
    assert!(encoder_code.contains("-> cj.Value"));
    assert!(encoder_code.contains("cj.object(["));
    assert!(encoder_code.contains("cj.string(name)"));

    let decoder_code = groups[0].get_decoder_code(true, false);
    assert!(decoder_code.contains("import gleam/dynamic/decode"));
    assert!(decoder_code.contains("decode.success"));
}

struct CustomJsonBackend;

impl EncoderBackend for CustomJsonBackend {
    fn name(&self) -> &str {
        "custom-json"
    }

    fn module_imports(&self) -> Vec<String> {
        vec!["import custom/json as cj".to_string()]
    }

    fn return_type(&self) -> String {
        "cj.Value".to_string()
    }

    fn encode_object(
        &self,
        indent: &str,
        fields: &[(String, String)],
        closing_indent: &str,
    ) -> String {
        if fields.is_empty() {
            return self.encode_empty_object(closing_indent);
        }

        let entries = fields
            .iter()
            .map(|(key, value)| format!(r#"{indent}  #("{key}", {value})"#))
            .collect::<Vec<_>>()
            .join(",\n");

        format!(
            "{closing_indent}cj.object([\n{entries}\n{closing_indent}])",
            closing_indent = closing_indent,
            entries = entries
        )
    }

    fn encode_empty_object(&self, indent: &str) -> String {
        format!("{indent}cj.object([])", indent = indent)
    }

    fn encode_string_literal(&self, value: &str) -> String {
        format!(r#"cj.string("{value}")"#, value = value)
    }

    fn encode_string(&self, value_expr: &str) -> String {
        format!("cj.string({})", value_expr)
    }

    fn encode_int(&self, value_expr: &str) -> String {
        format!("cj.int({})", value_expr)
    }

    fn encode_float(&self, value_expr: &str) -> String {
        format!("cj.float({})", value_expr)
    }

    fn encode_bool(&self, value_expr: &str) -> String {
        format!("cj.bool({})", value_expr)
    }

    fn encode_nullable(&self, value_expr: &str, inner_encoder: &str) -> String {
        format!("cj.nullable({}, {})", value_expr, inner_encoder)
    }

    fn encode_array(&self, value_expr: &str, inner_encoder: &str) -> String {
        format!("cj.array({}, {})", value_expr, inner_encoder)
    }

    fn required_packages(&self) -> &[&'static str] {
        &[]
    }
}
