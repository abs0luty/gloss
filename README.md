# Gloss

Gloss generates Gleam encoders and decoders from annotations so you can focus on modelling data instead of wiring JSON boilerplate. It understands project‑level configuration, file/type overrides, external helper functions, and now supports pluggable encoder backends.

## Requirements

- Rust toolchain (for building the CLI)
- Gleam toolchain (`gleam` command) – the CLI formats generated files with `gleam format`
- Your project must depend on `gleam/json`; the generator refuses to emit encoders until the package is listed in `gleam.toml`

## Install

```bash
cargo build --release
# binary at target/release/gloss
```

## Quick Start

1. **Declare configuration** (`gloss.toml`)

   ```toml
   field_naming_strategy = "snake_case"
   absent_field_mode = "maybe_absent"

   [output]
   separate_files = true
   generated_file_naming = "{module}_gloss.gleam"
   directory = "gen"

   [fn_naming]
   encoder_function_naming = "encode_{type_snake}"
   decoder_function_naming = "decode_{type_snake}"
   ```

2. **Annotate types**

   ```gleam
   // gloss!: encoder(json), decoder, camelCase, type_tag = "kind"
   pub type Message {
     Text(content: String)
     Image(url: String, width: Int, height: Int)
   }
   ```

3. **Generate code**

   ```bash
   gloss generate --path .
   ```

   Output:

   ```
   src/message.gleam → gen/message_gloss.gleam
   ```

   The CLI formats every generated file with `gleam format`.

## CLI

```bash
gloss generate [OPTIONS]
```

Options:
- `-p, --path <PATH>`: project root (default `.`)
- `--dry-run`: print code without touching files
- `-v, --verbose`: show decisions and file paths

## Configuration Reference

All configuration lives in `gloss.toml`. Settings cascade: project root → subdirectories → `// gloss-file!` → `// gloss!` per type.

### Global keys

| Key | Values | Purpose |
| --- | --- | --- |
| `field_naming_strategy` | `snake_case` / `camel_case` | Default JSON field naming |
| `absent_field_mode` | `error_if_absent` / `maybe_absent` | How `Option(T)` behaves when missing |
| `decoder_unknown_variant_message` | string | Default fallback error message for unknown constructors |

### `[output]` block

| Key | Description |
| --- | --- |
| `separate_files` | Generate dedicated files instead of appending to source |
| `separate_encoder_decoder` | Split encoders and decoders into two files |
| `generated_file_naming` | Pattern for combined output (placeholders: `{module}`, `{module_snake}`, `{module_pascal}`) |
| `encode_module_naming` / `decode_module_naming` | Patterns when `separate_encoder_decoder = true` |
| `directory` | Where to place generated files. Prefix with `@/` for project root, `./` for file-relative |

Example:

```toml
[output]
separate_files = true
separate_encoder_decoder = true
directory = "@/gloss"
encode_module_naming = "encode_{module}.gleam"
decode_module_naming = "decode_{module}.gleam"
```

### `[fn_naming]` block

Control generated function names using `{type}`, `{type_snake}`, and `{type_pascal}` placeholders.

```toml
[fn_naming]
encoder_function_naming = "{type_pascal}_encode"
decoder_function_naming = "{type_pascal}_decode"
```

### Custom Encoder Backends

Encoders are implemented through the `gloss_core::EncoderBackend` trait. The CLI uses `JsonEncoderBackend`, but you can plug in any runtime by providing your own implementation:

```rust
use gloss_core::{generate_for_project, EncoderBackend};
use std::sync::Arc;

struct YamlEncoder;

impl EncoderBackend for YamlEncoder {
    fn name(&self) -> &str { "yaml" }
    fn module_imports(&self) -> Vec<String> { vec!["import my/yaml".into()] }
    fn return_type(&self) -> String { "yaml.Value".into() }
    // implement the remaining helpers (objects, primitives, arrays, nullable, etc.)
}

let backend = Arc::new(YamlEncoder);
let generated = generate_for_project(&project_root, backend)?;
```

Each method returns the string expression Gloss should emit for a particular construct. This keeps encoder generation abstract enough to support JSON, YAML, TOML, or any custom target API.

## Configuration Cascade

1. `gloss.toml` at project root
2. `gloss.toml` in subdirectories (closest file wins)
3. `// gloss-file!: ...` per source file
4. `// gloss!: ...` per type

Every layer can override the same keys, just like CSS specificity. Example:

```toml
# root/gloss.toml
[output]
directory = "gen"
generated_file_naming = "{module}_gloss.gleam"

# src/admin/gloss.toml
[output]
directory = "@/admin_gen"
separate_encoder_decoder = true
```

If you add in-file overrides:

```gleam
// gloss-file!: output_dir = "./local_gen", generated_file_naming = "{module}_local.gleam"
```

Gloss uses the most specific value for that file/type.

## File-Level Overrides (`// gloss-file!`)

Put the directive before type definitions in a file:

```gleam
// gloss-file!: output_dir = "@/clients", generated_file_naming = "{module}_clients.gleam"
// gloss-file!: encoder_fn = "file_encode_{type}", decoder_fn = "file_decode_{type}"
```

Supported keys mirror `gloss.toml`: `output_dir`, `separate_encoder_decoder`, `generated_file_naming`, `encode_module_naming`, `decode_module_naming`, plus `encoder_fn` / `decoder_fn` pattern overrides and `unknown_variant_message`.

## Type Annotations (`// gloss!`)

Use immediately above a type. Available flags:

| Flag | Effect |
| --- | --- |
| `encoder(json)` | Generate an encoder using the configured backend (`gleam/json` today) |
| `decoder` | Generate a decoder |
| `snake_case` / `camelCase` | Override field naming strategy for this type |
| `type_tag = "field"` | Specify the variant tag field name |
| `no_type_tag` | Disable auto-tagging (for single-field enums) |
| `output_dir = "./gen"` | Override output directory |
| `generated_file_naming = "..."` | Override file name pattern |
| `encode_module_naming` / `decode_module_naming` | Override split file names |
| `unknown_variant_message = "..."` | Customise the fallback error |
| `encoder_fn = "pattern"` / `decoder_fn = "pattern"` | Override function names |

Example:

```gleam
// gloss!: encoder(json), decoder, camelCase,
//         output_dir = "@/api",
//         generated_file_naming = "{module}_api.gleam",
//         encoder_fn = "encode_{type_pascal}",
//         decoder_fn = "decode_{type_pascal}",
//         unknown_variant_message = "Unknown {type} tag"
pub type ApiMessage {
  Text(content: String)
  Image(url: String, width: Int, height: Int)
}
```

## Field Annotations

Place directly above constructor arguments.

| Annotation | Meaning |
| --- | --- |
| `maybe_absent` | Allow field to be missing; decoder uses `option.None` |
| `must_exist` | Require presence even when global mode allows omission |
| `rename = "jsonName"` | Custom JSON field name |
| `decoder_with = "module.function"` | Use external decoder; Gloss imports the module |
| `encoder_with = "module.function"` | Use external encoder for this field |

Example:

```gleam
pub type Profile {
  Profile(
    // gloss!: rename = "userId"
    id: String,
    // gloss!: maybe_absent
    nickname: Option(String),
    // gloss!: decoder_with = "uuid/decoder.parse", encoder_with = "uuid/encoder.to_string"
    uuid: uuid.Uuid,
  )
}
```

## Absent Field Strategy

- `error_if_absent` (default): missing `Option(T)` fields are an error unless `maybe_absent` is present.
- `maybe_absent`: missing `Option(T)` becomes `option.None`. Use `must_exist` to require specific fields.

Unit tests cover both strategies in `gloss_core/tests/absent_field.rs`.

## Type Tags and Unknown Variants

- `type_tag = "kind"` customises the discriminator field.
- `no_type_tag` writes plain records.
- `unknown_variant_message = "Unknown {type} value"` lets you set a precise decoder failure message (`{type}` is replaced with the Gleam type name).

## External Functions

Use `decoder_with` / `encoder_with` to call existing helpers.

```gleam
// gloss!: encoder(json), decoder
pub type Wrapper {
  Wrapper(
    // gloss!: decoder_with = "profile/codec.profile_decoder",
    //          encoder_with = "profile/codec.profile_to_json"
    profile: profile.Profile
  )
}
```

Gloss adds `import profile/codec as profile_codec` and calls the functions with the current alias.

## Decoder Backend Customisation

The CLI exposes `JsonEncoderBackend::default()`; replace it with your own backend before calling `generate_for_project` if you need a different format. Generated code will import whatever modules your backend declares and call the functions you output in the trait methods.

## Generated Files and Formatting

- Separate outputs go wherever `output.directory` (and overrides) point.
- Inline generation appends a marker section to the source file.
- After writing, the CLI runs `gleam format <file>` so generated code matches Gleam style.

## Safety Checks

Running `gloss generate` fails fast when:

- `gleam/json` is missing from `[dependencies]` or `[dev-dependencies]`
- The Gleam formatter command cannot be executed

## End-to-End Example

Input (`src/example.gleam`):

```gleam
import gleam/option.{type Option}

// gloss!: encoder(json), decoder, camelCase
pub type User {
  User(
    id: String,
    // gloss!: rename = "displayName"
    name: String,
    // gloss!: maybe_absent
    email: Option(String),
  )
}
```

Config (`gloss.toml`):

```toml
field_naming_strategy = "snake_case"

[output]
separate_files = true
directory = "gen"
generated_file_naming = "{module}_gloss.gleam"
```

Generated (`gen/example_gloss.gleam`):

```gleam
// This file was generated by gloss
// https://github.com/abs0luty/gloss
//
// Do not modify this file directly.
// Any changes will be overwritten when gloss regenerates this file.

import gleam/dynamic/decode
import gleam/json
import gleam/option

pub fn decode_user() -> decode.Decoder(User) {
  {
    use id <- decode.field("id", decode.string)
    use name <- decode.field("displayName", decode.string)
    use email <- decode.optional_field("email", option.None, decode.optional(decode.string))
    decode.success(User(id:, name:, email:))
  }
}

pub fn encode_user(user: User) -> json.Json {
  let User(id:, name:, email:) = user
  json.object([
    #("id", json.string(id)),
    #("displayName", json.string(name)),
    #("email", json.nullable(email, _)),
  ])
}
```

## Development

Run the test suite:

```bash
cargo test
```

The tests exercise cascading configuration, overrides, dependency guards, and naming patterns.

---

Happy generating! If you add a new encoder backend or discover a missing override, contributions are welcome.
