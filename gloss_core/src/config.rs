use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Global default field naming convention
    #[serde(default)]
    pub field_naming: FieldNamingConvention,

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
    #[serde(default = "default_file_pattern")]
    pub file_pattern: String,

    /// Whether to create separate files per module
    #[serde(default = "default_separate_files")]
    pub separate_files: bool,

    /// Whether to create separate files for encoders and decoders
    #[serde(default = "default_separate_encoder_decoder")]
    pub separate_encoder_decoder: bool,

    /// File naming pattern for encoder files (used when separate_encoder_decoder = true)
    /// Available placeholders: {module}, {module_snake}, {module_pascal}
    /// Default: "encode_{module}.gleam"
    #[serde(default = "default_encoder_pattern")]
    pub encoder_pattern: String,

    /// File naming pattern for decoder files (used when separate_encoder_decoder = true)
    /// Available placeholders: {module}, {module_snake}, {module_pascal}
    /// Default: "decode_{module}.gleam"
    #[serde(default = "default_decoder_pattern")]
    pub decoder_pattern: String,
}

fn default_file_pattern() -> String {
    "{module}_gloss.gleam".to_string()
}

fn default_separate_files() -> bool {
    true
}

fn default_separate_encoder_decoder() -> bool {
    false
}

fn default_encoder_pattern() -> String {
    "encode_{module}.gleam".to_string()
}

fn default_decoder_pattern() -> String {
    "decode_{module}.gleam".to_string()
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            directory: None,
            file_pattern: default_file_pattern(),
            separate_files: default_separate_files(),
            separate_encoder_decoder: default_separate_encoder_decoder(),
            encoder_pattern: default_encoder_pattern(),
            decoder_pattern: default_decoder_pattern(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            field_naming: FieldNamingConvention::SnakeCase,
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
    pub fn new(field_naming: FieldNamingConvention, absent_field_mode: AbsentFieldMode) -> Self {
        Self {
            field_naming,
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
            field_naming: other.field_naming, // For enums, other always wins
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
    #[serde(default = "default_encoder_fn_pattern")]
    pub encoder_fn_pattern: String,

    #[serde(default = "default_decoder_fn_pattern")]
    pub decoder_fn_pattern: String,
}

fn default_encoder_fn_pattern() -> String {
    "{type_snake}_to_json".to_string()
}

fn default_decoder_fn_pattern() -> String {
    "{type_snake}_decoder".to_string()
}

impl Default for FnNamingConfig {
    fn default() -> Self {
        Self {
            encoder_fn_pattern: default_encoder_fn_pattern(),
            decoder_fn_pattern: default_decoder_fn_pattern(),
        }
    }
}

impl FnNamingConfig {
    pub fn merge_with(self, other: Self) -> Self {
        Self {
            encoder_fn_pattern: if other.encoder_fn_pattern != default_encoder_fn_pattern() {
                other.encoder_fn_pattern
            } else {
                self.encoder_fn_pattern
            },
            decoder_fn_pattern: if other.decoder_fn_pattern != default_decoder_fn_pattern() {
                other.decoder_fn_pattern
            } else {
                self.decoder_fn_pattern
            },
        }
    }

    pub fn apply_override(self, override_cfg: &FnNamingOverride) -> Self {
        Self {
            encoder_fn_pattern: override_cfg
                .encoder_fn_pattern
                .clone()
                .unwrap_or(self.encoder_fn_pattern),
            decoder_fn_pattern: override_cfg
                .decoder_fn_pattern
                .clone()
                .unwrap_or(self.decoder_fn_pattern),
        }
    }

    pub fn render_encoder_fn_name(&self, type_name: &str) -> String {
        render_fn_pattern(&self.encoder_fn_pattern, type_name)
    }

    pub fn render_decoder_fn_name(&self, type_name: &str) -> String {
        render_fn_pattern(&self.decoder_fn_pattern, type_name)
    }
}

#[derive(Debug, Clone, Default)]
pub struct FnNamingOverride {
    pub encoder_fn_pattern: Option<String>,
    pub decoder_fn_pattern: Option<String>,
}

fn render_fn_pattern(pattern: &str, type_name: &str) -> String {
    pattern
        .replace("{type}", type_name)
        .replace("{type_snake}", &to_snake_case_name(type_name))
        .replace("{type_pascal}", &to_pascal_case_name(type_name))
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
            file_pattern: if other.file_pattern != default_file_pattern() {
                other.file_pattern
            } else {
                self.file_pattern
            },
            separate_files: other.separate_files, // Boolean, other wins
            separate_encoder_decoder: other.separate_encoder_decoder,
            encoder_pattern: if other.encoder_pattern != default_encoder_pattern() {
                other.encoder_pattern
            } else {
                self.encoder_pattern
            },
            decoder_pattern: if other.decoder_pattern != default_decoder_pattern() {
                other.decoder_pattern
            } else {
                self.decoder_pattern
            },
        }
    }

    /// Apply an OutputOverride onto this config (used for file/type-level overrides)
    pub fn apply_override(self, override_config: &crate::parser::OutputOverride) -> Self {
        Self {
            directory: override_config.directory.clone().or(self.directory),
            file_pattern: override_config
                .file_pattern
                .clone()
                .unwrap_or(self.file_pattern),
            separate_files: self.separate_files, // Keep config value
            separate_encoder_decoder: override_config
                .separate_encoder_decoder
                .unwrap_or(self.separate_encoder_decoder),
            encoder_pattern: override_config
                .encoder_pattern
                .clone()
                .unwrap_or(self.encoder_pattern),
            decoder_pattern: override_config
                .decoder_pattern
                .clone()
                .unwrap_or(self.decoder_pattern),
        }
    }
}
