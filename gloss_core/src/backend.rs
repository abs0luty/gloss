use crate::parser::EncoderType;
use std::collections::HashMap;
use std::sync::Arc;

pub trait EncoderBackend: Send + Sync {
    /// Human-readable backend identifier (used for debugging and equality checks)
    fn name(&self) -> &str;

    /// Import statements required for encoder generation
    fn module_imports(&self) -> Vec<String>;

    /// Return type to emit in encoder signatures (e.g. "json.Json")
    fn return_type(&self) -> String;

    /// Format an object expression given key/value pairs and indentation helpers
    fn encode_object(
        &self,
        indent: &str,
        fields: &[(String, String)],
        closing_indent: &str,
    ) -> String;

    /// Format an empty object literal at the provided indentation level
    fn encode_empty_object(&self, indent: &str) -> String;

    /// Literal encoding helpers
    fn encode_string_literal(&self, value: &str) -> String;

    /// Encode primitive values
    fn encode_string(&self, value_expr: &str) -> String;
    fn encode_int(&self, value_expr: &str) -> String;
    fn encode_float(&self, value_expr: &str) -> String;
    fn encode_bool(&self, value_expr: &str) -> String;

    /// Encode collections
    fn encode_nullable(&self, value_expr: &str, inner_encoder: &str) -> String;
    fn encode_array(&self, value_expr: &str, inner_encoder: &str) -> String;

    /// Gleam package dependencies that must exist in gleam.toml
    fn required_packages(&self) -> &[&'static str] {
        &[]
    }
}

#[derive(Default)]
pub struct JsonEncoderBackend;

impl JsonEncoderBackend {
    const ALIAS: &'static str = "json";
    const RETURN_TYPE: &'static str = "Json";

    fn qualify(fn_name: &str) -> String {
        format!("{}.{}", Self::ALIAS, fn_name)
    }
}

impl EncoderBackend for JsonEncoderBackend {
    fn name(&self) -> &str {
        "json"
    }

    fn module_imports(&self) -> Vec<String> {
        vec!["import gleam/json".to_string()]
    }

    fn return_type(&self) -> String {
        format!("{}.{}", Self::ALIAS, Self::RETURN_TYPE)
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
            "{}{}([\n{}\n{}])",
            closing_indent,
            Self::qualify("object"),
            entries,
            closing_indent
        )
    }

    fn encode_empty_object(&self, indent: &str) -> String {
        format!("{}{}([])", indent, Self::qualify("object"))
    }

    fn encode_string_literal(&self, value: &str) -> String {
        format!(
            r#"{alias}.string("{value}")"#,
            alias = Self::ALIAS,
            value = value
        )
    }

    fn encode_string(&self, value_expr: &str) -> String {
        format!("{}({})", Self::qualify("string"), value_expr)
    }

    fn encode_int(&self, value_expr: &str) -> String {
        format!("{}({})", Self::qualify("int"), value_expr)
    }

    fn encode_float(&self, value_expr: &str) -> String {
        format!("{}({})", Self::qualify("float"), value_expr)
    }

    fn encode_bool(&self, value_expr: &str) -> String {
        format!("{}({})", Self::qualify("bool"), value_expr)
    }

    fn encode_nullable(&self, value_expr: &str, inner_encoder: &str) -> String {
        format!(
            "{}({}, {})",
            Self::qualify("nullable"),
            value_expr,
            inner_encoder
        )
    }

    fn encode_array(&self, value_expr: &str, inner_encoder: &str) -> String {
        format!(
            "{}({}, {})",
            Self::qualify("array"),
            value_expr,
            inner_encoder
        )
    }

    fn required_packages(&self) -> &[&'static str] {
        &["gleam/json"]
    }
}

pub type EncoderBackendRef = Arc<dyn EncoderBackend + Send + Sync>;

#[derive(Default, Clone)]
pub struct BackendRegistry {
    backends: HashMap<EncoderType, EncoderBackendRef>,
}

impl BackendRegistry {
    pub fn new() -> Self {
        let mut registry = HashMap::new();
        registry.insert(
            EncoderType::Json,
            Arc::new(JsonEncoderBackend::default()) as EncoderBackendRef,
        );
        Self { backends: registry }
    }

    pub fn with_backend(mut self, encoder_type: EncoderType, backend: EncoderBackendRef) -> Self {
        self.backends.insert(encoder_type, backend);
        self
    }

    pub fn get(&self, encoder_type: EncoderType) -> Option<&EncoderBackendRef> {
        self.backends.get(&encoder_type)
    }

    pub fn values(&self) -> impl Iterator<Item = &EncoderBackendRef> {
        self.backends.values()
    }
}
