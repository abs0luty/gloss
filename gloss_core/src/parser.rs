use crate::config::{FieldNamingConvention, FnNamingOverride};
use crate::{GlossError, Result};
use camino::{Utf8Path, Utf8PathBuf};
use gleam_core::ast::{self, AssignName};
use gleam_core::warning::WarningEmitter;
use regex::Regex;
use std::collections::{BTreeSet, HashMap};

/// Output configuration that can be specified at file or type level
#[derive(Debug, Clone, Default)]
pub struct OutputOverride {
    /// Output directory (can be relative to file or project root)
    pub directory: Option<String>,
    /// Whether to separate encoders and decoders
    pub separate_encoder_decoder: Option<bool>,
    /// Encoder file naming rule
    pub encode_module_naming: Option<String>,
    /// Decoder file naming rule
    pub decode_module_naming: Option<String>,
    /// Combined file naming rule
    pub generated_file_naming: Option<String>,
}

/// Specifies whether a path is relative to project root or file directory
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathMode {
    /// Relative to the file's directory (default)
    FileRelative,
    /// Relative to the project root
    ProjectRelative,
}

impl OutputOverride {
    /// Determine path mode based on directory string
    /// @/path or /path -> ProjectRelative
    /// ./path or path -> FileRelative
    pub fn path_mode(&self) -> PathMode {
        if let Some(ref dir) = self.directory {
            if dir.starts_with('@') || dir.starts_with('/') {
                return PathMode::ProjectRelative;
            }
        }
        PathMode::FileRelative
    }

    /// Get the clean directory path (without mode prefixes)
    pub fn clean_directory(&self) -> Option<String> {
        self.directory.as_ref().map(|dir| {
            if dir.starts_with('@') {
                dir.strip_prefix("@/")
                    .or(dir.strip_prefix('@'))
                    .unwrap_or(dir)
            } else if dir.starts_with('/') {
                dir.strip_prefix('/').unwrap_or(dir)
            } else {
                dir.strip_prefix("./").unwrap_or(dir)
            }
            .to_string()
        })
    }
}

#[derive(Debug, Clone)]
pub struct CustomTypeInfo {
    pub name: String,
    pub constructors: Vec<ConstructorInfo>,
    pub encoders: Vec<EncoderType>,
    pub generate_decoder: bool,
    pub field_naming_strategy: Option<FieldNamingConvention>,
    pub module_name: String,
    pub module_path: String,
    pub type_tag_field: Option<String>, // Custom type tag field name (default: "type")
    pub disable_type_tag: bool,         // If true, don't use type tags
    pub output_override: Option<OutputOverride>, // Type-level output configuration
    pub unknown_variant_message: Option<String>,
    pub fn_naming_override: Option<FnNamingOverride>,
    pub option_availability: OptionAvailability,
}

#[derive(Debug, Clone, Default)]
pub struct OptionAvailability {
    pub unqualified: bool,
    pub aliases: BTreeSet<String>,
}

/// File-level configuration
#[derive(Debug, Clone, Default)]
pub struct FileConfig {
    pub output_override: Option<OutputOverride>,
    pub unknown_variant_message: Option<String>,
    pub fn_naming_override: Option<FnNamingOverride>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EncoderType {
    Json,
}

impl EncoderType {
    pub fn identifier(self) -> &'static str {
        match self {
            EncoderType::Json => "json",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConstructorInfo {
    pub name: String,
    pub fields: Vec<FieldInfo>,
}

#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub label: String,
    pub type_: String,
    pub type_expr: TypeExpression,
    pub is_option: bool,
    pub marker: FieldMarker,
    pub custom_name: Option<String>, // Custom JSON field name
    pub decoder_with: Option<String>,
    pub encoder_with: Option<String>,
}

#[derive(Debug, Clone)]
pub enum TypeExpression {
    Constructor {
        module: Option<String>,
        name: String,
        arguments: Vec<TypeExpression>,
    },
    Tuple(Vec<TypeExpression>),
    Function {
        arguments: Vec<TypeExpression>,
        return_type: Box<TypeExpression>,
    },
    Var(String),
    Hole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldMarker {
    /// Field is required (not optional, cannot be absent) — use `must_exist`
    Required,
    /// Field is optional (can be absent) - decoded as Option(None) if missing — use `maybe_absent`
    Optional,
    /// Default behavior (determined by config)
    Default,
}

/// Parse Gleam source files and extract custom types marked for code generation
pub fn parse_gleam_files(
    root_path: &Utf8PathBuf,
) -> Result<HashMap<Utf8PathBuf, (FileConfig, Vec<CustomTypeInfo>)>> {
    let src_dir = root_path.join("src");
    let mut custom_types_by_file = HashMap::new();

    // Find all .gleam files
    let gleam_files = find_gleam_files(&src_dir)?;

    for file_path in gleam_files {
        let source = std::fs::read_to_string(&file_path)?;

        let relative_path = file_path.strip_prefix(&src_dir).map_err(|_| {
            GlossError::ParseError(format!("Failed to determine module path for {}", file_path))
        })?;
        let module_path = relative_path.with_extension("").to_string();

        let (file_config, types) = parse_file(&file_path, &module_path, &source)?;

        if !types.is_empty() {
            custom_types_by_file.insert(file_path, (file_config, types));
        }
    }

    Ok(custom_types_by_file)
}

fn find_gleam_files(dir: &Utf8Path) -> Result<Vec<Utf8PathBuf>> {
    let mut files = Vec::new();

    if !dir.exists() {
        return Ok(files);
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = Utf8PathBuf::try_from(entry.path())
            .map_err(|e| GlossError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        if path.is_dir() {
            files.extend(find_gleam_files(&path)?);
        } else if path.extension() == Some("gleam") {
            files.push(path);
        }
    }

    Ok(files)
}

fn parse_file(
    file_path: &Utf8Path,
    module_path: &str,
    source: &str,
) -> Result<(FileConfig, Vec<CustomTypeInfo>)> {
    // Parse file-level configuration
    let file_config = parse_file_level_config(source);

    // Parse using Gleam's parser
    let warnings = WarningEmitter::null();
    let parsed = gleam_core::parse::parse_module(file_path.to_path_buf(), source, &warnings)
        .map_err(|e| GlossError::ParseError(format!("{:?}", e)))?;

    let mut custom_types = Vec::new();

    // Extract module name from file path
    let module_name = file_path.file_stem().unwrap_or("unknown").to_string();
    let option_availability = compute_option_availability(&parsed.module)?;

    // Look for custom types with @gloss annotations in comments
    for definition in &parsed.module.definitions {
        if let ast::TargetedDefinition {
            definition: ast::UntypedDefinition::CustomType(custom_type),
            ..
        } = definition
        {
            let info = extract_custom_type_info(
                custom_type,
                source,
                &module_name,
                module_path,
                &option_availability,
            )?;
            if !info.encoders.is_empty() || info.generate_decoder {
                custom_types.push(info);
            }
        }
    }

    Ok((file_config, custom_types))
}

fn compute_option_availability(
    module: &ast::Module<(), ast::TargetedDefinition>,
) -> Result<OptionAvailability> {
    let mut availability = OptionAvailability::default();
    let mut other_unqualified_sources: BTreeSet<String> = BTreeSet::new();

    for definition in &module.definitions {
        if let ast::TargetedDefinition {
            definition: ast::UntypedDefinition::Import(import),
            ..
        } = definition
        {
            let module_path = import.module.to_string();

            if module_path == "gleam/option" {
                let alias = import
                    .as_name
                    .as_ref()
                    .and_then(|(assign, _)| match assign {
                        AssignName::Variable(name) => Some(name.to_string()),
                        _ => None,
                    })
                    .unwrap_or_else(|| {
                        import
                            .module
                            .split('/')
                            .next_back()
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "option".to_string())
                    });
                availability.aliases.insert(alias);
            }

            for unqualified in &import.unqualified_types {
                if unqualified.name.as_str() == "Option" {
                    if module_path == "gleam/option" {
                        availability.unqualified = true;
                    } else {
                        other_unqualified_sources.insert(module_path.clone());
                    }
                }
            }
        }
    }

    if availability.unqualified && !other_unqualified_sources.is_empty() {
        return Err(GlossError::GenerationError(
            "Conflicting imports for `Option`. gloss requires `Option` to come from `gleam/option` when using optional fields."
                .to_string(),
        ));
    }

    if !availability.unqualified && other_unqualified_sources.is_empty() {
        // Assume built-in Option when nothing else is imported unqualified.
        availability.unqualified = true;
    }

    Ok(availability)
}

fn extract_custom_type_info(
    custom_type: &ast::UntypedCustomType,
    source: &str,
    module_name: &str,
    module_path: &str,
    option_availability: &OptionAvailability,
) -> Result<CustomTypeInfo> {
    // Check for gloss!: annotations in the doc comment
    let annotations = if let Some((_, doc)) = &custom_type.documentation {
        parse_gloss_annotations(doc)
    } else {
        // Look for comments before the type definition
        let type_start = custom_type.location.start as usize;
        let doc = extract_comment_before(source, type_start);
        parse_gloss_annotations(&doc)
    };

    let constructors = custom_type
        .constructors
        .iter()
        .map(|c| extract_constructor_info(c, source, option_availability))
        .collect::<Result<Vec<_>>>()?;

    Ok(CustomTypeInfo {
        name: custom_type.name.to_string(),
        constructors,
        encoders: annotations.encoders,
        generate_decoder: annotations.generate_decoder,
        field_naming_strategy: annotations.field_naming_strategy,
        type_tag_field: annotations.type_tag_field,
        disable_type_tag: annotations.disable_type_tag,
        module_path: module_path.to_string(),
        module_name: module_name.to_string(),
        output_override: annotations.output_override,
        unknown_variant_message: annotations.unknown_variant_message,
        fn_naming_override: annotations.fn_naming_override,
        option_availability: option_availability.clone(),
    })
}

fn extract_constructor_info(
    constructor: &ast::RecordConstructor<()>,
    source: &str,
    option_availability: &OptionAvailability,
) -> Result<ConstructorInfo> {
    let fields = constructor
        .arguments
        .iter()
        .map(|arg| extract_field_info(arg, source, option_availability))
        .collect::<Result<Vec<_>>>()?;

    Ok(ConstructorInfo {
        name: constructor.name.to_string(),
        fields,
    })
}

fn extract_field_info(
    arg: &ast::RecordConstructorArg<()>,
    source: &str,
    option_availability: &OptionAvailability,
) -> Result<FieldInfo> {
    let label = arg
        .label
        .as_ref()
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| "_unlabeled".to_string());

    let type_str = type_ast_to_string(&arg.ast);
    let type_expr = type_ast_to_expression(&arg.ast, option_availability);
    let is_option = matches!(
        &type_expr,
        TypeExpression::Constructor {
            module: Some(module_path),
            name,
            ..
        } if name == "Option" && module_path == "gleam/option"
    );

    // Look for marker comments before the field
    let annotations = if let Some((_, doc)) = &arg.doc {
        parse_field_annotations(doc)
    } else {
        // Try to extract comment from source
        let field_start = arg.location.start as usize;
        let comment = extract_comment_before(source, field_start);
        parse_field_annotations(&comment)
    };

    Ok(FieldInfo {
        label,
        type_: type_str,
        type_expr,
        is_option,
        marker: annotations.marker,
        custom_name: annotations.custom_name,
        decoder_with: annotations.decoder_with,
        encoder_with: annotations.encoder_with,
    })
}

#[derive(Debug, Default)]
struct GlossAnnotations {
    encoders: Vec<EncoderType>,
    generate_decoder: bool,
    field_naming_strategy: Option<FieldNamingConvention>,
    type_tag_field: Option<String>,
    disable_type_tag: bool,
    output_override: Option<OutputOverride>,
    unknown_variant_message: Option<String>,
    fn_naming_override: Option<FnNamingOverride>,
}

fn parse_gloss_annotations(text: &str) -> GlossAnnotations {
    let mut annotations = GlossAnnotations::default();

    // Look for new syntax: gloss!: encoder(json), decoder
    let gloss_re = Regex::new(r"gloss!:\s*(.+)").unwrap();

    for cap in gloss_re.captures_iter(text) {
        if let Some(args) = cap.get(1) {
            let args_str = args.as_str();

            // Parse encoder(json)
            if args_str.contains("encoder(json)") || args_str.contains("encoder(JSON)") {
                annotations.encoders.push(EncoderType::Json);
            }

            // Parse decoder
            if args_str.contains("decoder") {
                annotations.generate_decoder = true;
            }

            // Parse snake_case or camelCase
            if args_str.contains("snake_case") {
                annotations.field_naming_strategy = Some(FieldNamingConvention::SnakeCase);
            }
            if args_str.contains("camelCase") {
                annotations.field_naming_strategy = Some(FieldNamingConvention::CamelCase);
            }

            // Parse type_tag = "field_name"
            let tag_re = Regex::new(r#"type_tag\s*=\s*"([^"]+)""#).unwrap();
            if let Some(tag_cap) = tag_re.captures(args_str) {
                if let Some(tag_name) = tag_cap.get(1) {
                    annotations.type_tag_field = Some(tag_name.as_str().to_string());
                }
            }

            // Parse no_type_tag
            if args_str.contains("no_type_tag") {
                annotations.disable_type_tag = true;
            }

            // Parse output configuration overrides
            if let Some(output_override) = parse_output_override(args_str) {
                annotations.output_override = Some(output_override);
            }

            if let Some(message) = parse_unknown_variant_message(args_str) {
                annotations.unknown_variant_message = Some(message);
            }

            if let Some(naming_override) = parse_fn_naming_override(args_str) {
                annotations.fn_naming_override = Some(naming_override);
            }
        }
    }

    annotations
}

#[derive(Debug)]
struct FieldAnnotations {
    marker: FieldMarker,
    custom_name: Option<String>,
    decoder_with: Option<String>,
    encoder_with: Option<String>,
}

impl Default for FieldAnnotations {
    fn default() -> Self {
        Self {
            marker: FieldMarker::Default,
            custom_name: None,
            decoder_with: None,
            encoder_with: None,
        }
    }
}

fn parse_field_annotations(text: &str) -> FieldAnnotations {
    let mut annotations = FieldAnnotations::default();

    // Look for gloss!: annotations
    let gloss_re = Regex::new(r"gloss!:\s*(.+)").unwrap();

    for cap in gloss_re.captures_iter(text) {
        if let Some(args) = cap.get(1) {
            let args_str = args.as_str();

            // Parse optional/required markers
            if args_str.contains("maybe_absent") || args_str.contains("optional") {
                annotations.marker = FieldMarker::Optional;
            } else if args_str.contains("must_exist")
                || args_str.contains("required")
                || args_str.contains("error_if_absent")
            {
                annotations.marker = FieldMarker::Required;
            }

            // Parse rename = "custom_name"
            let rename_re = Regex::new(r#"rename\s*=\s*"([^"]+)""#).unwrap();
            if let Some(rename_cap) = rename_re.captures(args_str) {
                if let Some(name) = rename_cap.get(1) {
                    annotations.custom_name = Some(name.as_str().to_string());
                }
            }

            // Parse decoder_with = "module.function"
            let decoder_with_re = Regex::new(r#"decoder_with\s*=\s*"([^"]+)""#).unwrap();
            if let Some(cap) = decoder_with_re.captures(args_str) {
                if let Some(value) = cap.get(1) {
                    annotations.decoder_with = Some(value.as_str().to_string());
                }
            }

            // Parse encoder_with = "module.function"
            let encoder_with_re = Regex::new(r#"encoder_with\s*=\s*"([^"]+)""#).unwrap();
            if let Some(cap) = encoder_with_re.captures(args_str) {
                if let Some(value) = cap.get(1) {
                    annotations.encoder_with = Some(value.as_str().to_string());
                }
            }
        }
    }

    annotations
}

/// Parse output configuration override from annotation string
/// Handles: output_dir = "@/gen", separate_encoder_decoder = true, etc.
fn parse_output_override(args_str: &str) -> Option<OutputOverride> {
    let mut override_config = OutputOverride::default();
    let mut has_config = false;

    // Parse output_dir = "path"
    let dir_re = Regex::new(r#"output_dir\s*=\s*"([^"]+)""#).unwrap();
    if let Some(cap) = dir_re.captures(args_str) {
        if let Some(dir) = cap.get(1) {
            override_config.directory = Some(dir.as_str().to_string());
            has_config = true;
        }
    }

    // Parse separate_encoder_decoder = true/false
    let sep_re = Regex::new(r"separate_encoder_decoder\s*=\s*(true|false)").unwrap();
    if let Some(cap) = sep_re.captures(args_str) {
        if let Some(val) = cap.get(1) {
            override_config.separate_encoder_decoder = Some(val.as_str() == "true");
            has_config = true;
        }
    }

    // Parse encode_module_naming = "pattern"
    let enc_pattern_re = Regex::new(r#"encode_module_naming\s*=\s*"([^"]+)""#).unwrap();
    if let Some(cap) = enc_pattern_re.captures(args_str) {
        if let Some(pattern) = cap.get(1) {
            override_config.encode_module_naming = Some(pattern.as_str().to_string());
            has_config = true;
        }
    }

    // Parse decode_module_naming = "pattern"
    let dec_pattern_re = Regex::new(r#"decode_module_naming\s*=\s*"([^"]+)""#).unwrap();
    if let Some(cap) = dec_pattern_re.captures(args_str) {
        if let Some(pattern) = cap.get(1) {
            override_config.decode_module_naming = Some(pattern.as_str().to_string());
            has_config = true;
        }
    }

    // Parse generated_file_naming = "pattern"
    let file_pattern_re = Regex::new(r#"generated_file_naming\s*=\s*"([^"]+)""#).unwrap();
    if let Some(cap) = file_pattern_re.captures(args_str) {
        if let Some(pattern) = cap.get(1) {
            override_config.generated_file_naming = Some(pattern.as_str().to_string());
            has_config = true;
        }
    }

    if has_config {
        Some(override_config)
    } else {
        None
    }
}

fn parse_unknown_variant_message(args_str: &str) -> Option<String> {
    let unknown_variant_re = Regex::new(r#"unknown_variant_message\s*=\s*"([^"]+)""#).unwrap();
    if let Some(cap) = unknown_variant_re.captures(args_str) {
        if let Some(msg) = cap.get(1) {
            return Some(msg.as_str().to_string());
        }
    }

    None
}

fn parse_fn_naming_override(args_str: &str) -> Option<FnNamingOverride> {
    let mut override_cfg = FnNamingOverride::default();
    let mut has_value = false;

    let encoder_re = Regex::new(r#"encoder_fn\s*=\s*"([^"]+)""#).unwrap();
    if let Some(cap) = encoder_re.captures(args_str) {
        if let Some(pattern) = cap.get(1) {
            override_cfg.encoder_function_naming = Some(pattern.as_str().to_string());
            has_value = true;
        }
    }

    let decoder_re = Regex::new(r#"decoder_fn\s*=\s*"([^"]+)""#).unwrap();
    if let Some(cap) = decoder_re.captures(args_str) {
        if let Some(pattern) = cap.get(1) {
            override_cfg.decoder_function_naming = Some(pattern.as_str().to_string());
            has_value = true;
        }
    }

    if has_value {
        Some(override_cfg)
    } else {
        None
    }
}

/// Parse file-level configuration from // gloss-file!: annotations
fn parse_file_level_config(source: &str) -> FileConfig {
    let mut file_config = FileConfig::default();

    // Look for // gloss-file!: annotations (single-line comment)
    let file_gloss_re = Regex::new(r"//\s*gloss-file!:\s*(.+)").unwrap();

    for cap in file_gloss_re.captures_iter(source) {
        if let Some(args) = cap.get(1) {
            let args_str = args.as_str();
            if let Some(output_override) = parse_output_override(args_str) {
                file_config.output_override = Some(output_override);
            }
            if let Some(message) = parse_unknown_variant_message(args_str) {
                file_config.unknown_variant_message = Some(message);
            }
            if let Some(naming_override) = parse_fn_naming_override(args_str) {
                file_config.fn_naming_override = Some(naming_override);
            }
        }
    }

    file_config
}

fn extract_comment_before(source: &str, position: usize) -> String {
    let before = &source[..position.min(source.len())];
    let lines: Vec<&str> = before.lines().collect();

    let mut comment = String::new();
    for line in lines.iter().rev() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            comment.insert_str(0, &format!("{}\n", trimmed.trim_start_matches("//").trim()));
        } else if trimmed.is_empty() {
            continue;
        } else {
            break;
        }
    }

    comment
}

fn type_ast_to_expression(
    type_ast: &ast::TypeAst,
    option_availability: &OptionAvailability,
) -> TypeExpression {
    match type_ast {
        ast::TypeAst::Constructor(c) => {
            let module_alias = c.module.as_ref().map(|(module, _)| module.to_string());
            let is_standard_option = c.name == "Option"
                && match &module_alias {
                    Some(alias) => option_availability.aliases.contains(alias),
                    None => option_availability.unqualified,
                };
            let resolved_module = if is_standard_option {
                Some("gleam/option".to_string())
            } else {
                module_alias.clone()
            };

            TypeExpression::Constructor {
                module: resolved_module,
                name: c.name.to_string(),
                arguments: c
                    .arguments
                    .iter()
                    .map(|arg| type_ast_to_expression(arg, option_availability))
                    .collect(),
            }
        }
        ast::TypeAst::Tuple(t) => TypeExpression::Tuple(
            t.elements
                .iter()
                .map(|elem| type_ast_to_expression(elem, option_availability))
                .collect(),
        ),
        ast::TypeAst::Fn(f) => TypeExpression::Function {
            arguments: f
                .arguments
                .iter()
                .map(|arg| type_ast_to_expression(arg, option_availability))
                .collect(),
            return_type: Box::new(type_ast_to_expression(&f.return_, option_availability)),
        },
        ast::TypeAst::Var(v) => TypeExpression::Var(v.name.to_string()),
        ast::TypeAst::Hole { .. } => TypeExpression::Hole,
    }
}

fn type_ast_to_string(type_ast: &ast::TypeAst) -> String {
    match type_ast {
        ast::TypeAst::Constructor(c) => {
            if c.arguments.is_empty() {
                c.name.to_string()
            } else {
                let args = c
                    .arguments
                    .iter()
                    .map(|a| type_ast_to_string(a))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({})", c.name, args)
            }
        }
        ast::TypeAst::Tuple(t) => {
            let elements = t
                .elements
                .iter()
                .map(|e| type_ast_to_string(e))
                .collect::<Vec<_>>()
                .join(", ");
            format!("#({})", elements)
        }
        ast::TypeAst::Fn(f) => {
            let args = f
                .arguments
                .iter()
                .map(|a| type_ast_to_string(a))
                .collect::<Vec<_>>()
                .join(", ");
            format!("fn({}) -> {}", args, type_ast_to_string(&f.return_))
        }
        ast::TypeAst::Var(v) => v.name.to_string(),
        ast::TypeAst::Hole { .. } => "_".to_string(),
    }
}
