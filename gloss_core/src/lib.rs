mod backend;
mod config;
mod generator;
mod parser;

pub use backend::{BackendRegistry, EncoderBackend, EncoderBackendRef, JsonEncoderBackend};
pub use config::{AbsentFieldMode, Config, FieldNamingConvention, OutputConfig};
pub use parser::{
    parse_gleam_files, CustomTypeInfo, EncoderType, FieldInfo, FieldMarker, FileConfig,
    OutputOverride, PathMode,
};

use camino::Utf8PathBuf;
use generator::{generate_decoder, generate_encoder};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GlossError {
    #[error("Failed to parse Gleam file: {0}")]
    ParseError(String),

    #[error("Failed to read file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Gleam compilation error: {0}")]
    GleamError(String),

    #[error("Generation error: {0}")]
    GenerationError(String),
}

pub type Result<T> = std::result::Result<T, GlossError>;

/// Generated code for a single type
#[derive(Debug, Clone)]
pub struct TypeCode {
    pub type_name: String,
    pub module_path: String,
    pub constructors: Vec<String>,
    pub decoder: Option<String>,
    pub encoder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImportEntry {
    pub module_path: String,
    pub alias: String,
    pub values: std::collections::BTreeSet<String>,
    pub types: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct TypeRegistryEntry {
    pub(crate) module_path: String,
    pub(crate) generates_decoder: bool,
    pub(crate) decoder_fn_name: Option<String>,
    pub(crate) encoder_fn_names: BTreeMap<String, String>,
}

pub(crate) type TypeRegistry = HashMap<String, HashMap<String, TypeRegistryEntry>>;
pub(crate) type TypeLookup = HashMap<(String, String), CustomTypeInfo>;

/// Generated code organized by type, preserving order
pub struct GeneratedCode {
    /// Ordered list of types with their encoder/decoder code
    pub types: Vec<TypeCode>,
    /// Effective output configuration after cascading and overrides
    pub output_config: OutputConfig,
    /// Path mode for this file (project-relative or file-relative)
    pub path_mode: PathMode,
    /// Additional module imports required by generated code
    pub custom_imports: BTreeMap<String, ImportEntry>,
    /// Encoder backends used for this group, if any encoders were generated
    pub encoder_backends: BTreeMap<String, EncoderBackendRef>,
    /// Whether any decoder in this group uses option helpers
    pub decoder_uses_option_helpers: bool,
}

impl Clone for GeneratedCode {
    fn clone(&self) -> Self {
        Self {
            types: self.types.clone(),
            output_config: self.output_config.clone(),
            path_mode: self.path_mode,
            custom_imports: self.custom_imports.clone(),
            encoder_backends: self
                .encoder_backends
                .iter()
                .map(|(name, backend)| (name.clone(), Arc::clone(backend)))
                .collect(),
            decoder_uses_option_helpers: self.decoder_uses_option_helpers,
        }
    }
}

impl std::fmt::Debug for GeneratedCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeneratedCode")
            .field("types", &self.types)
            .field("output_config", &self.output_config)
            .field("path_mode", &self.path_mode)
            .field("custom_imports", &self.custom_imports)
            .field(
                "encoder_backends",
                &self.encoder_backends.keys().cloned().collect::<Vec<_>>(),
            )
            .field(
                "decoder_uses_option_helpers",
                &self.decoder_uses_option_helpers,
            )
            .finish()
    }
}

pub(crate) fn module_alias(module_path: &str) -> String {
    let mut alias = module_path
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>();
    if alias.is_empty() {
        alias = "module".to_string();
    }
    alias
}

impl ImportEntry {
    pub(crate) fn new(module_path: &str, alias: String) -> Self {
        Self {
            module_path: module_path.to_string(),
            alias,
            values: BTreeSet::new(),
            types: BTreeSet::new(),
        }
    }
}

fn build_type_registry(
    custom_types: &HashMap<Utf8PathBuf, (FileConfig, Vec<CustomTypeInfo>)>,
) -> TypeRegistry {
    let mut registry: TypeRegistry = HashMap::new();

    for (_file_path, (_file_config, types)) in custom_types.iter() {
        for type_info in types {
            registry
                .entry(type_info.module_path.clone())
                .or_insert_with(HashMap::new)
                .insert(
                    type_info.name.clone(),
                    TypeRegistryEntry {
                        module_path: type_info.module_path.clone(),
                        generates_decoder: type_info.generate_decoder,
                        decoder_fn_name: None,
                        encoder_fn_names: BTreeMap::new(),
                    },
                );
        }
    }

    registry
}

fn build_type_lookup(
    custom_types: &HashMap<Utf8PathBuf, (FileConfig, Vec<CustomTypeInfo>)>,
) -> TypeLookup {
    let mut lookup = HashMap::new();

    for (_file_path, (_file_config, types)) in custom_types.iter() {
        for type_info in types {
            lookup.insert(
                (type_info.module_path.clone(), type_info.name.clone()),
                type_info.clone(),
            );
        }
    }

    lookup
}

fn has_generated_encoders(
    custom_types: &HashMap<Utf8PathBuf, (FileConfig, Vec<CustomTypeInfo>)>,
) -> bool {
    custom_types
        .values()
        .any(|(_file_config, types)| types.iter().any(|type_info| !type_info.encoders.is_empty()))
}

fn ensure_backend_dependencies(
    project_root: &Utf8PathBuf,
    backend: &dyn EncoderBackend,
) -> Result<()> {
    let required = backend.required_packages();
    if required.is_empty() {
        return Ok(());
    }

    let gleam_toml_path = project_root.join("gleam.toml");
    if !gleam_toml_path.exists() {
        return Err(GlossError::GenerationError(format!(
            "Generating encoders requires the `{}` dependency, but {} was not found",
            required.join("`, `"),
            gleam_toml_path
        )));
    }

    let manifest = std::fs::read_to_string(&gleam_toml_path).map_err(|error| {
        GlossError::GenerationError(format!("Failed to read {}: {}", gleam_toml_path, error))
    })?;

    let parsed: toml::Value = toml::from_str(&manifest).map_err(|error| {
        GlossError::GenerationError(format!("Failed to parse {}: {}", gleam_toml_path, error))
    })?;

    for package in required {
        let present = dependency_table_contains(&parsed, "dependencies", package)
            || dependency_table_contains(&parsed, "dev-dependencies", package);

        if !present {
            return Err(GlossError::GenerationError(format!(
                "Generating encoders requires the `{}` dependency. Add `{} = \"~> 1\"` (or your preferred version) to gleam.toml.",
                package, package
            )));
        }
    }

    Ok(())
}

fn dependency_table_contains(parsed: &toml::Value, section: &str, crate_name: &str) -> bool {
    parsed
        .get(section)
        .and_then(|entry| entry.as_table())
        .map_or(false, |table| table.contains_key(crate_name))
}

pub(crate) fn find_type_entry<'a>(
    registry: &'a TypeRegistry,
    module_hint: Option<&str>,
    type_name: &str,
    current_module_path: &str,
) -> Option<&'a TypeRegistryEntry> {
    if let Some(hint) = module_hint {
        if let Some(types) = registry.get(hint) {
            if let Some(entry) = types.get(type_name) {
                return Some(entry);
            }
        }

        for (module_path, types) in registry {
            if module_path == hint || module_path.split('/').last() == Some(hint) {
                if let Some(entry) = types.get(type_name) {
                    return Some(entry);
                }
            }
        }
    } else if let Some(types) = registry.get(current_module_path) {
        if let Some(entry) = types.get(type_name) {
            return Some(entry);
        }
    }

    None
}

impl GeneratedCode {
    /// Get all decoder code (in order)
    pub fn get_decoder_code(&self, has_imports: bool, include_type_imports: bool) -> String {
        let mut code = String::new();

        // Add header comment
        code.push_str(&generate_header_comment());
        code.push_str("\n\n");

        if has_imports {
            let import_map = self.build_import_map(include_type_imports);
            let import_block = generate_imports(
                true,
                self.decoder_uses_option_helpers,
                false,
                None,
                &import_map,
            );
            if !import_block.is_empty() {
                code.push_str(&import_block);
                code.push_str("\n\n");
            }
        }

        for type_code in &self.types {
            if let Some(ref decoder) = type_code.decoder {
                code.push_str(decoder);
                code.push_str("\n\n");
            }
        }

        code
    }

    /// Get all encoder code (in order)
    pub fn get_encoder_code(&self, has_imports: bool, include_type_imports: bool) -> String {
        let mut code = String::new();

        // Add header comment
        code.push_str(&generate_header_comment());
        code.push_str("\n\n");

        if has_imports {
            let import_map = self.build_import_map(include_type_imports);
            let import_block = generate_imports(
                false,
                false,
                true,
                if self.encoder_backends.is_empty() {
                    None
                } else {
                    Some(&self.encoder_backends)
                },
                &import_map,
            );
            if !import_block.is_empty() {
                code.push_str(&import_block);
                code.push_str("\n\n");
            }
        }

        for type_code in &self.types {
            if let Some(ref encoder) = type_code.encoder {
                code.push_str(encoder);
                code.push_str("\n\n");
            }
        }

        code
    }

    /// Get combined code (decoder + encoder for each type, in order)
    pub fn get_combined_code(&self, has_imports: bool, include_type_imports: bool) -> String {
        let mut code = String::new();

        // Add header comment
        code.push_str(&generate_header_comment());
        code.push_str("\n\n");

        let has_decoder = self.types.iter().any(|t| t.decoder.is_some());
        let has_encoder = self.types.iter().any(|t| t.encoder.is_some());

        if has_imports && (has_decoder || has_encoder) {
            let import_map = self.build_import_map(include_type_imports);
            let import_block = generate_imports(
                has_decoder,
                self.decoder_uses_option_helpers,
                has_encoder,
                if has_encoder && !self.encoder_backends.is_empty() {
                    Some(&self.encoder_backends)
                } else {
                    None
                },
                &import_map,
            );
            if !import_block.is_empty() {
                code.push_str(&import_block);
                code.push_str("\n\n");
            }
        }

        for type_code in &self.types {
            if let Some(ref decoder) = type_code.decoder {
                code.push_str(decoder);
                code.push_str("\n\n");
            }
            if let Some(ref encoder) = type_code.encoder {
                code.push_str(encoder);
                code.push_str("\n\n");
            }
        }

        code
    }

    fn build_import_map(&self, include_type_imports: bool) -> BTreeMap<String, ImportEntry> {
        let mut imports = self.custom_imports.clone();

        if include_type_imports {
            for type_code in &self.types {
                add_type_import(
                    &mut imports,
                    &type_code.module_path,
                    &type_code.type_name,
                    &type_code.constructors,
                );
            }
        }

        imports
    }
}

/// Main entry point for generating encoders/decoders
pub fn generate_for_project(
    root_path: &Utf8PathBuf,
    registry: &BackendRegistry,
) -> Result<HashMap<Utf8PathBuf, Vec<GeneratedCode>>> {
    let mut custom_types = parse_gleam_files(root_path)?;

    if has_generated_encoders(&custom_types) {
        let mut used_encoders = std::collections::HashSet::new();
        for (_file, (_cfg, types)) in custom_types.iter() {
            for type_info in types {
                for encoder_type in &type_info.encoders {
                    used_encoders.insert(*encoder_type);
                }
            }
        }

        for encoder_type in used_encoders {
            let backend = registry.get(encoder_type).ok_or_else(|| {
                GlossError::GenerationError(format!(
                    "No encoder backend registered for {:?}",
                    encoder_type
                ))
            })?;
            ensure_backend_dependencies(root_path, backend.as_ref())?;
        }
    }
    let mut type_registry = build_type_registry(&custom_types);
    let type_lookup = build_type_lookup(&custom_types);
    let mut outputs = HashMap::new();

    // Precompute generated function names for all types so cross-file references can use them.
    for (file_path, (file_config, types)) in custom_types.iter() {
        let cascaded_config = Config::load_cascaded(root_path, file_path);
        let mut effective_config = cascaded_config.clone();

        if let Some(ref naming_override) = file_config.fn_naming_override {
            effective_config.fn_naming = effective_config.fn_naming.apply_override(naming_override);
        }

        for type_info in types {
            let mut type_config = effective_config.clone();
            if let Some(ref naming_override) = type_info.fn_naming_override {
                type_config.fn_naming = type_config.fn_naming.apply_override(naming_override);
            }

            if let Some(entry) = type_registry
                .get_mut(&type_info.module_path)
                .and_then(|m| m.get_mut(&type_info.name))
            {
                entry.decoder_fn_name = if type_info.generate_decoder {
                    Some(
                        type_config
                            .fn_naming
                            .render_decoder_fn_name(&type_info.name),
                    )
                } else {
                    None
                };
                entry.encoder_fn_names.clear();
                let mut unique_backends: Vec<&'static str> = Vec::new();
                for encoder_type in &type_info.encoders {
                    let backend_id = encoder_type.identifier();
                    if !unique_backends.contains(&backend_id) {
                        unique_backends.push(backend_id);
                    }
                }
                let multi_backend = unique_backends.len() > 1;
                let uses_backend_placeholder = type_config
                    .fn_naming
                    .encoder_function_naming
                    .contains("{backend");
                for backend_id in unique_backends {
                    let mut fn_name = type_config
                        .fn_naming
                        .render_encoder_fn_name(&type_info.name, backend_id);
                    if multi_backend && !uses_backend_placeholder {
                        fn_name = format!("{}_{}", fn_name, backend_id);
                    }
                    entry
                        .encoder_fn_names
                        .insert(backend_id.to_string(), fn_name);
                }
            }
        }
    }

    #[derive(Clone)]
    struct TypeContext {
        config: Config,
        path_mode: PathMode,
        unknown_message: Option<String>,
    }

    for (file_path, (file_config, types)) in custom_types.drain() {
        // Load cascaded config for this file (global + subdirectories)
        let cascaded_config = Config::load_cascaded(root_path, &file_path);
        let mut effective_config = cascaded_config.clone();
        let mut file_unknown_message = effective_config.decoder_unknown_variant_message.clone();

        // Determine default path mode from effective config (global/subdirectory)
        let mut path_mode = effective_config
            .output
            .directory
            .as_ref()
            .map(|dir| infer_path_mode(dir, PathMode::ProjectRelative))
            .unwrap_or(PathMode::FileRelative);

        // Apply file-level overrides if present
        if let Some(ref file_override) = file_config.output_override {
            if let Some(ref override_dir) = file_override.directory {
                path_mode = infer_path_mode(override_dir, file_override.path_mode());
            }

            effective_config.output = effective_config.output.apply_override(file_override);

            if file_override.directory.is_none() {
                if let Some(ref dir) = effective_config.output.directory {
                    path_mode = infer_path_mode(dir, path_mode);
                }
            }
        }

        if let Some(ref naming_override) = file_config.fn_naming_override {
            effective_config.fn_naming = effective_config.fn_naming.apply_override(naming_override);
        }

        if let Some(ref message) = file_config.unknown_variant_message {
            file_unknown_message = Some(message.clone());
        }

        let mut type_contexts: Vec<TypeContext> = Vec::with_capacity(types.len());

        for type_info in types.iter() {
            let mut type_config = effective_config.clone();
            let mut type_path_mode = path_mode;

            if let Some(ref type_override) = type_info.output_override {
                if let Some(ref override_dir) = type_override.directory {
                    type_path_mode = infer_path_mode(override_dir, type_override.path_mode());
                }

                type_config.output = type_config.output.apply_override(type_override);

                if type_override.directory.is_none() {
                    if let Some(ref dir) = type_config.output.directory {
                        type_path_mode = infer_path_mode(dir, type_path_mode);
                    }
                }
            } else if let Some(ref dir) = type_config.output.directory {
                type_path_mode = infer_path_mode(dir, type_path_mode);
            }

            if let Some(ref naming_override) = type_info.fn_naming_override {
                type_config.fn_naming = type_config.fn_naming.apply_override(naming_override);
            }

            let mut unknown_message = file_unknown_message.clone();
            if let Some(ref message) = type_info.unknown_variant_message {
                unknown_message = Some(message.clone());
            }

            type_config.decoder_unknown_variant_message = unknown_message.clone();

            type_contexts.push(TypeContext {
                config: type_config,
                path_mode: type_path_mode,
                unknown_message,
            });
        }

        let mut file_outputs: Vec<GeneratedCode> = Vec::new();

        for (type_info, ctx) in types.into_iter().zip(type_contexts.into_iter()) {
            let mut decoder = None;
            let mut encoder = None;
            let mut type_imports: BTreeMap<String, ImportEntry> = BTreeMap::new();
            let mut encoder_backends: BTreeMap<String, EncoderBackendRef> = BTreeMap::new();
            let mut decoder_uses_option_helpers = false;

            let type_config = ctx.config;
            let type_path_mode = ctx.path_mode;
            let unknown_message = ctx.unknown_message;

            // Generate decoder if requested
            if type_info.generate_decoder {
                let decoder_output = generate_decoder(
                    &type_info,
                    &type_config,
                    &type_registry,
                    &mut type_imports,
                    &type_lookup,
                    unknown_message.as_deref(),
                )?;
                decoder_uses_option_helpers = decoder_output.uses_option_helpers;
                decoder = Some(decoder_output.code);
            }

            // Generate encoder if requested (combine all encoder types into one)
            if !type_info.encoders.is_empty() {
                let mut encoder_code = String::new();
                for encoder_type in &type_info.encoders {
                    let backend_arc = registry
                        .get(*encoder_type)
                        .ok_or_else(|| {
                            GlossError::GenerationError(format!(
                                "No encoder backend registered for {:?}",
                                encoder_type
                            ))
                        })?
                        .clone();

                    let backend_name = backend_arc.name().to_string();
                    encoder_backends
                        .entry(backend_name)
                        .or_insert_with(|| backend_arc.clone());

                    encoder_code.push_str(&generate_encoder(
                        &type_info,
                        *encoder_type,
                        &type_config,
                        &type_registry,
                        &mut type_imports,
                        backend_arc.as_ref(),
                    )?);
                    encoder_code.push_str("\n\n");
                }
                encoder = Some(encoder_code.trim_end().to_string());
            }
            // Only add if we generated something
            if decoder.is_some() || encoder.is_some() {
                let type_output_config = type_config.output.clone();
                let type_code = TypeCode {
                    type_name: type_info.name.clone(),
                    module_path: type_info.module_path.clone(),
                    constructors: type_info
                        .constructors
                        .iter()
                        .map(|ctor| ctor.name.clone())
                        .collect(),
                    decoder,
                    encoder,
                };

                if let Some(existing) = file_outputs.iter_mut().find(|output| {
                    output.path_mode == type_path_mode && output.output_config == type_output_config
                }) {
                    existing.types.push(type_code);
                    merge_imports(&mut existing.custom_imports, type_imports);
                    for (name, backend) in &encoder_backends {
                        existing
                            .encoder_backends
                            .entry(name.clone())
                            .or_insert_with(|| backend.clone());
                    }
                    existing.decoder_uses_option_helpers |= decoder_uses_option_helpers;
                    if existing.decoder_uses_option_helpers {
                        ensure_no_option_alias_conflict(&existing.custom_imports)?;
                    }
                } else {
                    if decoder_uses_option_helpers {
                        ensure_no_option_alias_conflict(&type_imports)?;
                    }
                    file_outputs.push(GeneratedCode {
                        types: vec![type_code],
                        output_config: type_output_config,
                        path_mode: type_path_mode,
                        custom_imports: type_imports,
                        encoder_backends: encoder_backends.clone(),
                        decoder_uses_option_helpers: decoder_uses_option_helpers,
                    });
                }
            }
        }

        if !file_outputs.is_empty() {
            outputs.insert(file_path, file_outputs);
        }
    }

    Ok(outputs)
}

fn infer_path_mode(directory: &str, default_mode: PathMode) -> PathMode {
    if directory.starts_with('@') || directory.starts_with('/') {
        PathMode::ProjectRelative
    } else if directory.starts_with("./") {
        PathMode::FileRelative
    } else {
        default_mode
    }
}

/// Generate header comment for generated files
fn generate_header_comment() -> String {
    "// This file was generated by gloss\n\
     // https://github.com/abs0luty/gloss\n\
     //\n\
     // Do not modify this file directly.\n\
     // Any changes will be overwritten when gloss regenerates this file."
        .to_string()
}

fn ensure_no_option_alias_conflict(imports: &BTreeMap<String, ImportEntry>) -> Result<()> {
    for entry in imports.values() {
        if entry.alias == "option" && entry.module_path != "gleam/option" {
            return Err(GlossError::GenerationError(format!(
                "Cannot generate code because import alias `option` is already used for `{}`. \
                 Rename the conflicting import or its alias before running gloss.",
                entry.module_path
            )));
        }
    }
    Ok(())
}

/// Generate necessary imports based on what's being generated
fn generate_imports(
    has_decoder: bool,
    decoder_uses_option_helpers: bool,
    has_encoder: bool,
    encoder_backends: Option<&BTreeMap<String, EncoderBackendRef>>,
    custom_imports: &BTreeMap<String, ImportEntry>,
) -> String {
    let mut imports: Vec<String> = Vec::new();

    if has_decoder {
        imports.push("import gleam/dynamic/decode".to_string());
        if decoder_uses_option_helpers {
            imports.push("import gleam/option".to_string());
        }
    }

    if has_encoder {
        if let Some(backends) = encoder_backends {
            for backend in backends.values() {
                imports.extend(backend.module_imports());
            }
        }
    }

    // Deduplicate and sort
    imports.sort();
    imports.dedup();

    for entry in custom_imports.values() {
        let mut line = format!("import {}", entry.module_path);

        let mut exposures: Vec<String> = Vec::new();
        for ty in &entry.types {
            exposures.push(format!("type {}", ty));
        }
        for value in &entry.values {
            exposures.push(value.clone());
        }

        if !exposures.is_empty() {
            line.push_str(".{");
            line.push_str(&exposures.join(", "));
            line.push('}');
        }

        let default_alias = entry
            .module_path
            .rsplit('/')
            .next()
            .unwrap_or(&entry.module_path);
        if entry.alias != default_alias {
            line.push_str(&format!(" as {}", entry.alias));
        }

        imports.push(line);
    }

    imports.join("\n")
}

fn merge_imports(target: &mut BTreeMap<String, ImportEntry>, src: BTreeMap<String, ImportEntry>) {
    for (module_path, entry) in src {
        target
            .entry(module_path.clone())
            .and_modify(|existing| {
                existing.values.extend(entry.values.clone());
                existing.types.extend(entry.types.clone());
            })
            .or_insert(entry);
    }
}

fn add_type_import(
    imports: &mut BTreeMap<String, ImportEntry>,
    module_path: &str,
    type_name: &str,
    constructors: &[String],
) {
    let alias = module_alias(module_path);
    let entry = imports
        .entry(module_path.to_string())
        .or_insert_with(|| ImportEntry::new(module_path, alias));

    entry.types.insert(type_name.to_string());
    for constructor in constructors {
        entry.values.insert(constructor.clone());
    }
}
