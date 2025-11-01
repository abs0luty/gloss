use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Global default field naming convention
    #[serde(default)]
    pub field_naming_strategy: FieldNamingConvention,

    /// How to handle absent fields for Option(T) types
    /// Controls whether Option(T) fields can be absent from JSON or must be present (but can be null)
    #[serde(default)]
    pub absent_field_mode: AbsentFieldMode,

    /// Default failure message when an unknown variant is encountered during decoding
    #[serde(default)]
    pub decoder_unknown_variant_message: Option<String>,

    /// Output configuration
    #[serde(default)]
    pub output: OutputConfig,

    /// Naming configuration for generated functions
    #[serde(default)]
    pub fn_naming: FnNamingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputConfig {
    /// Directory for generated files (relative to project root)
    /// If None, appends to source files
    #[serde(default)]
    pub directory: Option<String>,

    /// File naming pattern for generated files (used when separate_encoder_decoder = false)
    /// Available placeholders: {module}, {module_snake}, {module_pascal}
    /// Default: "{module}_gloss.gleam"
    #[serde(default = "default_generated_file_naming")]
    pub generated_file_naming: String,

    /// Whether to create separate files per module
    #[serde(default = "default_separate_files")]
    pub separate_files: bool,

    /// Whether to create separate files for encoders and decoders
    #[serde(default = "default_separate_encoder_decoder")]
    pub separate_encoder_decoder: bool,

    /// File naming pattern for encoder files (used when separate_encoder_decoder = true)
    /// Available placeholders: {module}, {module_snake}, {module_pascal}
    /// Default: "encode_{module}.gleam"
    #[serde(default = "default_encode_module_naming")]
    pub encode_module_naming: String,

    /// File naming pattern for decoder files (used when separate_encoder_decoder = true)
    /// Available placeholders: {module}, {module_snake}, {module_pascal}
    /// Default: "decode_{module}.gleam"
    #[serde(default = "default_decode_module_naming")]
    pub decode_module_naming: String,
}

fn default_generated_file_naming() -> String {
    "{module}_gloss.gleam".to_string()
}

fn default_separate_files() -> bool {
    true
}

fn default_separate_encoder_decoder() -> bool {
    false
}

fn default_encode_module_naming() -> String {
    "encode_{module}.gleam".to_string()
}

fn default_decode_module_naming() -> String {
    "decode_{module}.gleam".to_string()
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            directory: None,
            generated_file_naming: default_generated_file_naming(),
            separate_files: default_separate_files(),
            separate_encoder_decoder: default_separate_encoder_decoder(),
            encode_module_naming: default_encode_module_naming(),
            decode_module_naming: default_decode_module_naming(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            field_naming_strategy: FieldNamingConvention::SnakeCase,
            absent_field_mode: AbsentFieldMode::ErrorIfAbsent,
            decoder_unknown_variant_message: None,
            output: OutputConfig::default(),
            fn_naming: FnNamingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldNamingConvention {
    /// Use snake_case for JSON field names
    SnakeCase,
    /// Use camelCase for JSON field names
    CamelCase,
}

impl Default for FieldNamingConvention {
    fn default() -> Self {
        Self::SnakeCase
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AbsentFieldMode {
    /// Option(T) fields must be present in JSON (but can be null)
    /// Use `gloss!: maybe_absent` to allow a field to be absent
    ErrorIfAbsent,

    /// Option(T) fields can be absent from JSON
    /// Use `gloss!: must_exist` to require a field to be present (but can be null)
    MaybeAbsent,
}

impl Default for AbsentFieldMode {
    fn default() -> Self {
        Self::ErrorIfAbsent
    }
}

impl Config {
    pub fn new(
        field_naming_strategy: FieldNamingConvention,
        absent_field_mode: AbsentFieldMode,
    ) -> Self {
        Self {
            field_naming_strategy,
            absent_field_mode,
            decoder_unknown_variant_message: None,
            output: OutputConfig::default(),
            fn_naming: FnNamingConfig::default(),
        }
    }

    /// Load config from gloss.toml file
    pub fn from_file(path: &Utf8PathBuf) -> Result<Self, String> {
        let content =
            fs::read_to_string(path).map_err(|e| format!("Failed to read config file: {}", e))?;

        let config: Config =
            toml::from_str(&content).map_err(|e| format!("Failed to parse config file: {}", e))?;

        Ok(config)
    }

    /// Try to load config from gloss.toml in the project directory, or use default
    pub fn load_or_default(project_path: &Utf8PathBuf) -> Self {
        let config_path = project_path.join("gloss.toml");
        if config_path.exists() {
            Self::from_file(&config_path).unwrap_or_else(|e| {
                eprintln!("Warning: {}", e);
                eprintln!("Using default configuration");
                Self::default()
            })
        } else {
            Self::default()
        }
    }

    /// Load and merge configs from project root to a specific file location
    /// Configs closer to the file override those further away
    pub fn load_cascaded(project_root: &Utf8PathBuf, file_path: &Utf8PathBuf) -> Self {
        let mut config = Self::load_or_default(project_root);

        // Find all gloss.toml files from project root to file's directory
        if let Some(file_dir) = file_path.parent() {
            let mut current_dir = file_dir;
            let mut configs_to_merge = Vec::new();

            // Collect all config files from file dir up to (but not including) project root
            loop {
                if current_dir == project_root {
                    break;
                }

                let config_path = current_dir.join("gloss.toml");
                if config_path.exists() {
                    configs_to_merge.push(config_path);
                }

                if let Some(parent) = current_dir.parent() {
                    current_dir = parent;
                } else {
                    break;
                }
            }

            // Merge configs from furthest to closest (closest wins)
            for config_path in configs_to_merge.iter().rev() {
                if let Ok(subdirectory_config) = Self::from_file(config_path) {
                    config = config.merge_with(subdirectory_config);
                }
            }
        }

        config
    }

    /// Merge another config into this one, with the other config taking precedence
    pub fn merge_with(self, other: Self) -> Self {
        Self {
            field_naming_strategy: other.field_naming_strategy, // For enums, other always wins
            absent_field_mode: other.absent_field_mode,
            decoder_unknown_variant_message: other
                .decoder_unknown_variant_message
                .or(self.decoder_unknown_variant_message),
            output: self.output.merge_with(other.output),
            fn_naming: self.fn_naming.merge_with(other.fn_naming),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FnNamingConfig {
    #[serde(default = "default_encoder_function_naming")]
    pub encoder_function_naming: String,

    #[serde(default = "default_decoder_function_naming")]
    pub decoder_function_naming: String,
}

fn default_encoder_function_naming() -> String {
    "{type_snake}_to_{backend}".to_string()
}

fn default_decoder_function_naming() -> String {
    "{type_snake}_decoder".to_string()
}

impl Default for FnNamingConfig {
    fn default() -> Self {
        Self {
            encoder_function_naming: default_encoder_function_naming(),
            decoder_function_naming: default_decoder_function_naming(),
        }
    }
}

impl FnNamingConfig {
    pub fn merge_with(self, other: Self) -> Self {
        Self {
            encoder_function_naming: if other.encoder_function_naming
                != default_encoder_function_naming()
            {
                other.encoder_function_naming
            } else {
                self.encoder_function_naming
            },
            decoder_function_naming: if other.decoder_function_naming
                != default_decoder_function_naming()
            {
                other.decoder_function_naming
            } else {
                self.decoder_function_naming
            },
        }
    }

    pub fn apply_override(self, override_cfg: &FnNamingOverride) -> Self {
        Self {
            encoder_function_naming: override_cfg
                .encoder_function_naming
                .clone()
                .unwrap_or(self.encoder_function_naming),
            decoder_function_naming: override_cfg
                .decoder_function_naming
                .clone()
                .unwrap_or(self.decoder_function_naming),
        }
    }

    pub fn render_encoder_fn_name(&self, type_name: &str, backend_identifier: &str) -> String {
        render_fn_pattern(
            &self.encoder_function_naming,
            type_name,
            Some(backend_identifier),
        )
    }

    pub fn render_decoder_fn_name(&self, type_name: &str) -> String {
        render_fn_pattern(&self.decoder_function_naming, type_name, None)
    }
}

#[derive(Debug, Clone, Default)]
pub struct FnNamingOverride {
    pub encoder_function_naming: Option<String>,
    pub decoder_function_naming: Option<String>,
}

fn render_fn_pattern(pattern: &str, type_name: &str, backend_identifier: Option<&str>) -> String {
    let mut rendered = pattern
        .replace("{type}", type_name)
        .replace("{type_snake}", &to_snake_case_name(type_name))
        .replace("{type_pascal}", &to_pascal_case_name(type_name));

    if let Some(backend) = backend_identifier {
        rendered = rendered
            .replace("{backend}", backend)
            .replace("{backend_snake}", &to_snake_case_name(backend))
            .replace("{backend_pascal}", &to_pascal_case_name(backend));
    } else {
        rendered = rendered
            .replace("{backend}", "")
            .replace("{backend_snake}", "")
            .replace("{backend_pascal}", "");
    }

    rendered
}

fn to_snake_case_name(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            for lower in ch.to_lowercase() {
                result.push(lower);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

fn to_pascal_case_name(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;

    for ch in s.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            for upper in ch.to_uppercase() {
                result.push(upper);
            }
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }

    result
}

impl OutputConfig {
    /// Merge another output config, with the other taking precedence for set fields
    pub fn merge_with(self, other: Self) -> Self {
        Self {
            directory: other.directory.or(self.directory),
            generated_file_naming: if other.generated_file_naming != default_generated_file_naming()
            {
                other.generated_file_naming
            } else {
                self.generated_file_naming
            },
            separate_files: other.separate_files, // Boolean, other wins
            separate_encoder_decoder: other.separate_encoder_decoder,
            encode_module_naming: if other.encode_module_naming != default_encode_module_naming() {
                other.encode_module_naming
            } else {
                self.encode_module_naming
            },
            decode_module_naming: if other.decode_module_naming != default_decode_module_naming() {
                other.decode_module_naming
            } else {
                self.decode_module_naming
            },
        }
    }

    /// Apply an OutputOverride onto this config (used for file/type-level overrides)
    pub fn apply_override(self, override_config: &crate::parser::OutputOverride) -> Self {
        Self {
            directory: override_config.directory.clone().or(self.directory),
            generated_file_naming: override_config
                .generated_file_naming
                .clone()
                .unwrap_or(self.generated_file_naming),
            separate_files: self.separate_files, // Keep config value
            separate_encoder_decoder: override_config
                .separate_encoder_decoder
                .unwrap_or(self.separate_encoder_decoder),
            encode_module_naming: override_config
                .encode_module_naming
                .clone()
                .unwrap_or(self.encode_module_naming),
            decode_module_naming: override_config
                .decode_module_naming
                .clone()
                .unwrap_or(self.decode_module_naming),
        }
    }
}
