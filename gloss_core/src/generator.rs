use crate::backend::EncoderBackend;
use crate::config::{AbsentFieldMode, Config, FieldNamingConvention};
use crate::parser::{
    ConstructorInfo, CustomTypeInfo, EncoderType, FieldInfo, FieldMarker, TypeExpression,
};
use crate::Result;
use crate::{find_type_entry, module_alias, GlossError, ImportEntry, TypeLookup, TypeRegistry};
use std::collections::{BTreeMap, HashSet};

pub(crate) struct DecoderOutput {
    pub code: String,
    pub uses_option_helpers: bool,
}

pub(crate) fn generate_decoder(
    type_info: &CustomTypeInfo,
    config: &Config,
    registry: &TypeRegistry,
    imports: &mut BTreeMap<String, ImportEntry>,
    type_lookup: &TypeLookup,
    unknown_variant_message: Option<&str>,
) -> Result<DecoderOutput> {
    let type_name = &type_info.name;
    let decoder_name = config.fn_naming.render_decoder_fn_name(type_name);

    // Determine encoding mode based on constructors and type_info settings
    let mode = determine_encoding_mode(&type_info.constructors, type_info);

    let field_naming = type_info
        .field_naming_strategy
        .unwrap_or(config.field_naming_strategy);

    let mut uses_option_helpers = false;

    let body = if type_info.constructors.len() == 1 {
        // Single constructor
        generate_single_constructor_decoder(
            &type_info.constructors[0],
            mode,
            field_naming,
            config,
            registry,
            imports,
            &type_info.module_path,
            0,
            &mut uses_option_helpers,
        )?
    } else {
        // Multiple constructors
        let type_tag_field = type_info.type_tag_field.as_deref().unwrap_or("type");
        let default_value_expr =
            default_value_for_type(type_info, type_lookup, imports, &mut uses_option_helpers);
        let expected_variants = format_expected_variants(&type_info.constructors);
        let expected_message =
            format_unknown_variant_message(type_name, unknown_variant_message, &expected_variants);
        generate_multi_constructor_decoder(
            &type_info.constructors,
            mode,
            field_naming,
            type_tag_field,
            config,
            registry,
            imports,
            &type_info.module_path,
            &default_value_expr,
            &expected_message,
            &mut uses_option_helpers,
        )?
    };

    Ok(DecoderOutput {
        code: format!(
            "pub fn {}() -> decode.Decoder({}) {}",
            decoder_name, type_name, body
        ),
        uses_option_helpers,
    })
}

pub(crate) fn generate_encoder(
    type_info: &CustomTypeInfo,
    encoder_type: EncoderType,
    config: &Config,
    registry: &TypeRegistry,
    imports: &mut BTreeMap<String, ImportEntry>,
    backend: &dyn EncoderBackend,
) -> Result<String> {
    let type_name = &type_info.name;
    let function_name = to_snake_case(type_name);
    let backend_identifier = encoder_type.identifier();
    let mut encoder_name = config
        .fn_naming
        .render_encoder_fn_name(type_name, backend_identifier);
    let uses_backend_placeholder = config
        .fn_naming
        .encoder_function_naming
        .contains("{backend");
    let mut unique_backend_ids: Vec<&'static str> = Vec::new();
    for enc in &type_info.encoders {
        let id = enc.identifier();
        if !unique_backend_ids.contains(&id) {
            unique_backend_ids.push(id);
        }
    }
    if unique_backend_ids.len() > 1 && !uses_backend_placeholder {
        encoder_name = format!("{}_{}", encoder_name, backend_identifier);
    }
    let arg_name = function_name.clone();
    // Determine encoding mode based on constructors and type_info settings
    let mode = determine_encoding_mode(&type_info.constructors, type_info);

    let field_naming = type_info
        .field_naming_strategy
        .unwrap_or(config.field_naming_strategy);
    let type_tag_field = type_info.type_tag_field.as_deref().unwrap_or("type");

    let body = if type_info.constructors.len() == 1 {
        // Single constructor
        generate_single_constructor_encoder(
            &type_info.constructors[0],
            &arg_name,
            mode,
            field_naming,
            type_tag_field,
            encoder_type,
            backend,
            registry,
            imports,
            &type_info.module_path,
            2,
        )?
    } else {
        // Multiple constructors
        generate_multi_constructor_encoder(
            &type_info.constructors,
            &arg_name,
            mode,
            field_naming,
            type_tag_field,
            encoder_type,
            backend,
            registry,
            imports,
            &type_info.module_path,
        )?
    };

    Ok(format!(
        "pub fn {}({}: {}) -> {} {{\n{}\n}}",
        encoder_name,
        arg_name,
        type_name,
        backend.return_type(),
        body
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EncodingMode {
    PlainString,
    ObjectWithTypeTag,
    ObjectWithNoTypeTag,
}

fn determine_encoding_mode(
    constructors: &[ConstructorInfo],
    type_info: &CustomTypeInfo,
) -> EncodingMode {
    // If type tags are disabled, don't use them
    if type_info.disable_type_tag {
        return EncodingMode::ObjectWithNoTypeTag;
    }

    match constructors {
        [constructor] if constructor.fields.is_empty() => EncodingMode::PlainString,
        [_constructor] => EncodingMode::ObjectWithNoTypeTag,
        constructors if constructors.iter().all(|c| c.fields.is_empty()) => {
            EncodingMode::PlainString
        }
        _ => EncodingMode::ObjectWithTypeTag,
    }
}

fn generate_single_constructor_decoder(
    constructor: &ConstructorInfo,
    mode: EncodingMode,
    field_naming: FieldNamingConvention,
    config: &Config,
    registry: &TypeRegistry,
    imports: &mut BTreeMap<String, ImportEntry>,
    current_module_path: &str,
    nesting: usize,
    uses_option_helpers: &mut bool,
) -> Result<String> {
    let constructor_name = &constructor.name;

    if mode == EncodingMode::PlainString {
        return Ok(format!("{{\n  decode.success({})\n}}", constructor_name));
    }

    if constructor.fields.is_empty() {
        return Ok(format!("{{\n  decode.success({})\n}}", constructor_name));
    }

    let mut field_decoders = Vec::new();
    for field in &constructor.fields {
        let field_decoder = generate_field_decoder(
            field,
            field_naming,
            config,
            registry,
            imports,
            current_module_path,
            nesting + 2,
            uses_option_helpers,
        )?;
        field_decoders.push(field_decoder);
    }

    let field_names: Vec<String> = constructor
        .fields
        .iter()
        .map(|f| format!("{}:", f.label))
        .collect();

    let decoders = field_decoders.join("\n");
    let indent = " ".repeat(nesting);

    Ok(format!(
        "{{\n{}\n{}  decode.success({}({}))\n{}}}",
        decoders,
        indent,
        constructor_name,
        field_names.join(", "),
        indent
    ))
}

fn generate_multi_constructor_decoder(
    constructors: &[ConstructorInfo],
    mode: EncodingMode,
    field_naming: FieldNamingConvention,
    type_tag_field: &str,
    config: &Config,
    registry: &TypeRegistry,
    imports: &mut BTreeMap<String, ImportEntry>,
    current_module_path: &str,
    default_value_expr: &str,
    expected_message: &str,
    uses_option_helpers: &mut bool,
) -> Result<String> {
    let discriminant = if mode == EncodingMode::PlainString {
        "use variant <- decode.then(decode.string)".to_string()
    } else {
        format!(
            r#"use variant <- decode.field("{}", decode.string)"#,
            type_tag_field
        )
    };

    let mut cases = Vec::new();
    for constructor in constructors {
        let tag = to_snake_case(&constructor.name);
        let body = generate_single_constructor_decoder(
            constructor,
            mode,
            field_naming,
            config,
            registry,
            imports,
            current_module_path,
            4,
            uses_option_helpers,
        )?;
        cases.push(format!(r#"    "{}" -> {}"#, tag, body.trim()));
    }

    let cases_str = cases.join("\n");
    let failure_message = escape_gleam_string(expected_message);

    Ok(format!(
        r#"{{
  {}
  case variant {{
{}
    _ -> decode.failure({}, "{}")
  }}
}}"#,
        discriminant, cases_str, default_value_expr, failure_message,
    ))
}

fn generate_field_decoder(
    field: &FieldInfo,
    field_naming: FieldNamingConvention,
    config: &Config,
    registry: &TypeRegistry,
    imports: &mut BTreeMap<String, ImportEntry>,
    current_module_path: &str,
    nesting: usize,
    uses_option_helpers: &mut bool,
) -> Result<String> {
    // Use custom name if provided, otherwise convert using naming convention
    let json_field_name = match &field.custom_name {
        Some(name) => name.clone(),
        None => convert_field_name(&field.label, field_naming),
    };
    let type_decoder = generate_type_decoder(
        &field.type_expr,
        field.decoder_with.as_deref(),
        registry,
        imports,
        current_module_path,
    )?;
    let indent = " ".repeat(nesting);

    // Determine if field should be optional or required
    let is_optional_field = match field.marker {
        FieldMarker::Optional => true,
        FieldMarker::Required => false,
        FieldMarker::Default => match config.absent_field_mode {
            AbsentFieldMode::MaybeAbsent => field.is_option,
            AbsentFieldMode::ErrorIfAbsent => false,
        },
    };

    if is_optional_field {
        // Field can be absent - use optional_field
        *uses_option_helpers = true;
        Ok(format!(
            r#"{}use {} <- decode.optional_field("{}", option.None, {})"#,
            indent, field.label, json_field_name, type_decoder
        ))
    } else {
        // Field must be present - use field
        Ok(format!(
            r#"{}use {} <- decode.field("{}", {})"#,
            indent, field.label, json_field_name, type_decoder
        ))
    }
}

fn generate_type_decoder(
    type_expr: &TypeExpression,
    override_fn: Option<&str>,
    registry: &TypeRegistry,
    imports: &mut BTreeMap<String, ImportEntry>,
    current_module_path: &str,
) -> Result<String> {
    if let Some(override_path) = override_fn {
        return resolve_decoder_override(override_path, imports, current_module_path);
    }

    match type_expr {
        TypeExpression::Constructor {
            module,
            name,
            arguments,
        } => {
            let name_str = name.as_str();

            if name_str == "Option"
                && module.as_deref() == Some("gleam/option")
                && !arguments.is_empty()
            {
                let inner = generate_type_decoder(
                    &arguments[0],
                    None,
                    registry,
                    imports,
                    current_module_path,
                )?;
                return Ok(format!("decode.optional({})", inner));
            }

            if name_str == "List" && !arguments.is_empty() {
                let inner = generate_type_decoder(
                    &arguments[0],
                    None,
                    registry,
                    imports,
                    current_module_path,
                )?;
                return Ok(format!("decode.list({})", inner));
            }

            match name_str {
                "String" => Ok("decode.string".to_string()),
                "Int" => Ok("decode.int".to_string()),
                "Float" => Ok("decode.float".to_string()),
                "Bool" => Ok("decode.bool".to_string()),
                _ => {
                    if let Some(entry) = find_type_entry(
                        registry,
                        module.as_deref(),
                        name,
                        current_module_path,
                    ) {
                        if !entry.generates_decoder {
                            return Err(GlossError::GenerationError(format!(
                                "Decoder requested for type `{}` but gloss is not generating one. Provide `decoder_with` override.",
                                name
                            )));
                        }

                        let decoder_name = entry
                            .decoder_fn_name
                            .as_ref()
                            .cloned()
                            .unwrap_or_else(|| format!("{}_decoder", to_snake_case(name)));
                        if entry.module_path == current_module_path {
                            Ok(format!("{}()", decoder_name))
                        } else {
                            let alias = ensure_import(imports, &entry.module_path);
                            Ok(format!("{}.{decoder_name}()", alias, decoder_name = decoder_name))
                        }
                    } else {
                        Err(GlossError::GenerationError(format!(
                            "Unable to determine decoder for type `{}`. Add a gloss annotation for that type or specify `decoder_with`.",
                            name
                        )))
                    }
                }
            }
        }
        TypeExpression::Var(name) => Err(GlossError::GenerationError(format!(
            "Cannot derive decoder for generic field `{}`. Provide `decoder_with` override.",
            name
        ))),
        TypeExpression::Tuple(_) | TypeExpression::Function { .. } | TypeExpression::Hole => Err(
            GlossError::GenerationError(
                "Cannot derive decoder for complex type expression. Provide `decoder_with` override.".to_string(),
            ),
        ),
    }
}

fn generate_single_constructor_encoder(
    constructor: &ConstructorInfo,
    arg_name: &str,
    mode: EncodingMode,
    field_naming: FieldNamingConvention,
    type_tag_field: &str,
    encoder_type: EncoderType,
    backend: &dyn EncoderBackend,
    registry: &TypeRegistry,
    imports: &mut BTreeMap<String, ImportEntry>,
    current_module_path: &str,
    nesting: usize,
) -> Result<String> {
    let indent = " ".repeat(nesting);

    if mode == EncodingMode::PlainString {
        let tag = to_snake_case(&constructor.name);
        return Ok(format!(
            r#"{}{}"#,
            indent,
            backend.encode_string_literal(&tag)
        ));
    }

    if constructor.fields.is_empty() {
        return Ok(backend.encode_empty_object(&indent));
    }

    // Unpack fields
    let field_labels: Vec<String> = constructor
        .fields
        .iter()
        .map(|f| format!("{}:", f.label))
        .collect();

    let unpacking = if !field_labels.is_empty() {
        format!(
            "{}let {}({}) = {}\n",
            indent,
            constructor.name,
            field_labels.join(", "),
            arg_name
        )
    } else {
        String::new()
    };

    // Generate field encoders
    let mut field_encoders: Vec<(String, String)> = Vec::new();

    if mode == EncodingMode::ObjectWithTypeTag {
        let tag = to_snake_case(&constructor.name);
        field_encoders.push((
            type_tag_field.to_string(),
            backend.encode_string_literal(&tag),
        ));
    }

    for field in &constructor.fields {
        let json_field_name = match &field.custom_name {
            Some(name) => name.clone(),
            None => convert_field_name(&field.label, field_naming),
        };
        let encoder = generate_type_encoder(
            &field.label,
            &field.type_expr,
            field.encoder_with.as_deref(),
            registry,
            imports,
            current_module_path,
            backend,
            encoder_type,
        )?;
        field_encoders.push((json_field_name, encoder));
    }

    let object_expr = backend.encode_object(&indent, &field_encoders, &indent);

    Ok(format!("{}{}", unpacking, object_expr))
}

fn generate_multi_constructor_encoder(
    constructors: &[ConstructorInfo],
    arg_name: &str,
    mode: EncodingMode,
    field_naming: FieldNamingConvention,
    type_tag_field: &str,
    encoder_type: EncoderType,
    backend: &dyn EncoderBackend,
    registry: &TypeRegistry,
    imports: &mut BTreeMap<String, ImportEntry>,
    current_module_path: &str,
) -> Result<String> {
    let mut cases = Vec::new();

    for constructor in constructors {
        let constructor_name = &constructor.name;

        let field_labels: Vec<String> = constructor
            .fields
            .iter()
            .map(|f| format!("{}:", f.label))
            .collect();

        let pattern = if field_labels.is_empty() {
            constructor_name.clone()
        } else {
            format!("{}({})", constructor_name, field_labels.join(", "))
        };

        // Generate encoder without unpacking since we unpack in the pattern
        let encoder = generate_constructor_encoder_body(
            constructor,
            mode,
            field_naming,
            type_tag_field,
            encoder_type,
            backend,
            registry,
            imports,
            current_module_path,
            4,
        )?;

        cases.push(format!("    {} -> {}", pattern, encoder.trim()));
    }

    let cases_str = cases.join("\n");

    Ok(format!("  case {} {{\n{}\n  }}", arg_name, cases_str))
}

fn generate_constructor_encoder_body(
    constructor: &ConstructorInfo,
    mode: EncodingMode,
    field_naming: FieldNamingConvention,
    type_tag_field: &str,
    encoder_type: EncoderType,
    backend: &dyn EncoderBackend,
    registry: &TypeRegistry,
    imports: &mut BTreeMap<String, ImportEntry>,
    current_module_path: &str,
    nesting: usize,
) -> Result<String> {
    if mode == EncodingMode::PlainString {
        let tag = to_snake_case(&constructor.name);
        return Ok(backend.encode_string_literal(&tag));
    }

    let indent = " ".repeat(nesting);

    if constructor.fields.is_empty() {
        return Ok(backend.encode_empty_object(&indent));
    }

    let mut field_encoders: Vec<(String, String)> = Vec::new();

    if mode == EncodingMode::ObjectWithTypeTag {
        let tag = to_snake_case(&constructor.name);
        field_encoders.push((
            type_tag_field.to_string(),
            backend.encode_string_literal(&tag),
        ));
    }

    for field in &constructor.fields {
        let json_field_name = match &field.custom_name {
            Some(name) => name.clone(),
            None => convert_field_name(&field.label, field_naming),
        };
        let encoder = generate_type_encoder(
            &field.label,
            &field.type_expr,
            field.encoder_with.as_deref(),
            registry,
            imports,
            current_module_path,
            backend,
            encoder_type,
        )?;
        field_encoders.push((json_field_name, encoder));
    }

    Ok(backend.encode_object(&indent, &field_encoders, &indent))
}

fn generate_type_encoder(
    var_name: &str,
    type_expr: &TypeExpression,
    override_fn: Option<&str>,
    registry: &TypeRegistry,
    imports: &mut BTreeMap<String, ImportEntry>,
    current_module_path: &str,
    backend: &dyn EncoderBackend,
    encoder_type: EncoderType,
) -> Result<String> {
    if let Some(override_path) = override_fn {
        return resolve_encoder_override(override_path, var_name, imports, current_module_path);
    }

    match type_expr {
        TypeExpression::Constructor {
            module,
            name,
            arguments,
        } => {
            let name_str = name.as_str();

            if name_str == "Option"
                && module.as_deref() == Some("gleam/option")
                && !arguments.is_empty()
            {
                // Placeholder inner encoder until optional helpers support richer expressions
                return Ok(backend.encode_nullable(var_name, "_"));
            }

            if name_str == "List" && !arguments.is_empty() {
                return Ok(backend.encode_array(var_name, "_"));
            }

            match name_str {
                "String" => Ok(backend.encode_string(var_name)),
                "Int" => Ok(backend.encode_int(var_name)),
                "Float" => Ok(backend.encode_float(var_name)),
                "Bool" => Ok(backend.encode_bool(var_name)),
                _ => {
                    if let Some(entry) = find_type_entry(
                        registry,
                        module.as_deref(),
                        name,
                        current_module_path,
                    ) {
                        let backend_id = encoder_type.identifier();
                        if !entry.encoder_fn_names.contains_key(backend_id) {
                            return Err(GlossError::GenerationError(format!(
                                "Encoder requested for type `{}` with backend `{}` but gloss is not generating one. Provide `encoder_with` override.",
                                name, backend_id
                            )));
                        }

                        let encoder_name = entry
                            .encoder_fn_names
                            .get(backend_id)
                            .cloned()
                            .unwrap_or_else(|| {
                                format!("{}_to_{}", to_snake_case(name), backend_id)
                            });

                        if entry.module_path == current_module_path {
                            Ok(format!("{}({})", encoder_name, var_name))
                        } else {
                            let alias = ensure_import(imports, &entry.module_path);
                            Ok(format!(
                                "{}.{encoder_name}({})",
                                alias,
                                var_name,
                                encoder_name = encoder_name
                            ))
                        }
                    } else {
                        Err(GlossError::GenerationError(format!(
                            "Unable to determine encoder for type `{}`. Add a gloss annotation for that type or specify `encoder_with`.",
                            name
                        )))
                    }
                }
            }
        }
        TypeExpression::Var(name) => Err(GlossError::GenerationError(format!(
            "Cannot derive encoder for generic field `{}`. Provide `encoder_with` override.",
            name
        ))),
        TypeExpression::Tuple(_) | TypeExpression::Function { .. } | TypeExpression::Hole => Err(
            GlossError::GenerationError(
                "Cannot derive encoder for complex type expression. Provide `encoder_with` override.".to_string(),
            ),
        ),
    }
}

#[derive(Debug)]
struct FunctionReference {
    module_path: Option<String>,
    function: String,
}

fn resolve_decoder_override(
    value: &str,
    imports: &mut BTreeMap<String, ImportEntry>,
    current_module_path: &str,
) -> Result<String> {
    let reference = parse_function_reference(value)?;
    let path = render_function_path(&reference, imports, current_module_path);
    Ok(format!("{}()", path))
}

fn resolve_encoder_override(
    value: &str,
    argument: &str,
    imports: &mut BTreeMap<String, ImportEntry>,
    current_module_path: &str,
) -> Result<String> {
    let reference = parse_function_reference(value)?;
    let path = render_function_path(&reference, imports, current_module_path);
    Ok(format!("{}({})", path, argument))
}

fn parse_function_reference(value: &str) -> Result<FunctionReference> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(GlossError::GenerationError(
            "Function reference cannot be empty".to_string(),
        ));
    }

    let (module_path, function) = match trimmed.rsplit_once('.') {
        Some((module_part, function_part)) if !function_part.is_empty() => {
            let module = module_part.trim();
            let function = function_part.trim();
            if module.is_empty() {
                (None, function.to_string())
            } else {
                (Some(module.to_string()), function.to_string())
            }
        }
        _ => (None, trimmed.to_string()),
    };

    if function.is_empty() {
        return Err(GlossError::GenerationError(format!(
            "Invalid function reference: `{}`",
            value
        )));
    }

    Ok(FunctionReference {
        module_path,
        function,
    })
}

fn render_function_path(
    reference: &FunctionReference,
    imports: &mut BTreeMap<String, ImportEntry>,
    current_module_path: &str,
) -> String {
    match &reference.module_path {
        Some(module_path) if module_path != current_module_path => {
            let alias = ensure_import(imports, module_path);
            format!("{}.{}", alias, reference.function)
        }
        _ => reference.function.clone(),
    }
}

fn ensure_import(imports: &mut BTreeMap<String, ImportEntry>, module_path: &str) -> String {
    ensure_import_entry(imports, module_path).alias.clone()
}

fn ensure_import_entry<'a>(
    imports: &'a mut BTreeMap<String, ImportEntry>,
    module_path: &str,
) -> &'a mut ImportEntry {
    use std::collections::btree_map::Entry;

    match imports.entry(module_path.to_string()) {
        Entry::Occupied(entry) => entry.into_mut(),
        Entry::Vacant(entry) => {
            let alias = module_alias(module_path);
            entry.insert(ImportEntry::new(module_path, alias))
        }
    }
}

fn format_expected_variants(constructors: &[ConstructorInfo]) -> String {
    let mut tags: Vec<String> = constructors
        .iter()
        .map(|constructor| to_snake_case(&constructor.name))
        .collect();
    tags.sort();
    tags.dedup();

    match tags.len() {
        0 => "value".to_string(),
        1 => tags[0].clone(),
        _ => format!("one of {}", tags.join(", ")),
    }
}

fn default_value_for_type(
    type_info: &CustomTypeInfo,
    type_lookup: &TypeLookup,
    imports: &mut BTreeMap<String, ImportEntry>,
    uses_option_helpers: &mut bool,
) -> String {
    let mut visited = HashSet::new();
    build_default_for_custom_type(
        &type_info.module_path,
        &type_info.name,
        &type_info.module_path,
        type_lookup,
        imports,
        &mut visited,
        uses_option_helpers,
    )
    .unwrap_or_else(|| panic_default_message(&format!("{}", type_info.name)))
}

fn build_default_for_custom_type(
    target_module: &str,
    type_name: &str,
    context_module: &str,
    type_lookup: &TypeLookup,
    imports: &mut BTreeMap<String, ImportEntry>,
    visited: &mut HashSet<(String, String)>,
    uses_option_helpers: &mut bool,
) -> Option<String> {
    let key = (target_module.to_string(), type_name.to_string());
    if !visited.insert(key.clone()) {
        return None;
    }

    let type_info = type_lookup.get(&key)?;
    let constructor = type_info.constructors.first()?;
    let expression = build_constructor_expression(
        constructor,
        target_module,
        context_module,
        type_lookup,
        imports,
        visited,
        uses_option_helpers,
    );
    visited.remove(&key);
    Some(expression)
}

fn build_constructor_expression(
    constructor: &ConstructorInfo,
    constructor_module: &str,
    context_module: &str,
    type_lookup: &TypeLookup,
    imports: &mut BTreeMap<String, ImportEntry>,
    visited: &mut HashSet<(String, String)>,
    uses_option_helpers: &mut bool,
) -> String {
    let prefix = if constructor_module == context_module {
        constructor.name.clone()
    } else {
        let alias = ensure_import(imports, constructor_module);
        format!("{}.{}", alias, constructor.name)
    };

    if constructor.fields.is_empty() {
        prefix
    } else {
        let field_values: Vec<String> = constructor
            .fields
            .iter()
            .map(|field| {
                let value = default_value_for_type_expr(
                    &field.type_expr,
                    constructor_module,
                    context_module,
                    type_lookup,
                    imports,
                    visited,
                    uses_option_helpers,
                );
                constructor_argument(field, value)
            })
            .collect();
        format!("{}({})", prefix, field_values.join(", "))
    }
}

fn default_value_for_type_expr(
    type_expr: &TypeExpression,
    current_module: &str,
    context_module: &str,
    type_lookup: &TypeLookup,
    imports: &mut BTreeMap<String, ImportEntry>,
    visited: &mut HashSet<(String, String)>,
    uses_option_helpers: &mut bool,
) -> String {
    match type_expr {
        TypeExpression::Constructor {
            module,
            name,
            arguments,
        } => {
            let module_path = module
                .as_ref()
                .map(|m| m.clone())
                .unwrap_or_else(|| current_module.to_string());

            match name.as_str() {
                "String" => "\"\"".to_string(),
                "Int" => "0".to_string(),
                "Float" => "0.0".to_string(),
                "Bool" => "False".to_string(),
                "List" if arguments.len() == 1 => "[]".to_string(),
                "Option" if arguments.len() == 1 && module.as_deref() == Some("gleam/option") => {
                    *uses_option_helpers = true;
                    "option.None".to_string()
                }
                _ => {
                    let display_name = if module_path == context_module {
                        name.clone()
                    } else {
                        format!("{}.{name}", module_path.replace('/', "."), name = name)
                    };

                    build_default_for_custom_type(
                        &module_path,
                        name,
                        context_module,
                        type_lookup,
                        imports,
                        visited,
                        uses_option_helpers,
                    )
                    .unwrap_or_else(|| panic_default_message(&display_name))
                }
            }
        }
        TypeExpression::Tuple(elements) => {
            let values: Vec<String> = elements
                .iter()
                .map(|elem| {
                    default_value_for_type_expr(
                        elem,
                        current_module,
                        context_module,
                        type_lookup,
                        imports,
                        visited,
                        uses_option_helpers,
                    )
                })
                .collect();
            format!("#({})", values.join(", "))
        }
        TypeExpression::Function { .. } => panic_default_message("function"),
        TypeExpression::Var(_) => panic_default_message("type variable"),
        TypeExpression::Hole => panic_default_message("type hole"),
    }
}

fn constructor_argument(field: &FieldInfo, value: String) -> String {
    if field.label.starts_with("_unlabeled") {
        value
    } else {
        format!("{}: {}", field.label, value)
    }
}

fn panic_default_message(subject: &str) -> String {
    format!(
        "panic(\"{}\")",
        escape_gleam_string(&format!("No default value for {}", subject))
    )
}

fn format_unknown_variant_message(
    type_name: &str,
    override_message: Option<&str>,
    default_expected: &str,
) -> String {
    override_message
        .map(|template| template.replace("{type}", type_name))
        .unwrap_or_else(|| default_expected.to_string())
}

fn convert_field_name(field_name: &str, naming: FieldNamingConvention) -> String {
    match naming {
        FieldNamingConvention::SnakeCase => field_name.to_string(),
        FieldNamingConvention::CamelCase => to_camel_case(field_name),
    }
}

fn escape_gleam_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(ch.to_lowercase().next().unwrap());
        } else {
            result.push(ch);
        }
    }
    result
}

fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;

    for (i, ch) in s.chars().enumerate() {
        if ch == '_' {
            capitalize_next = true;
        } else if i == 0 {
            result.push(ch.to_lowercase().next().unwrap());
        } else if capitalize_next {
            result.push(ch.to_uppercase().next().unwrap());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }

    result
}
