# Gloss - Gleam Encoder/Decoder Generator

An external code generation tool for Gleam that automatically generates JSON encoders and decoders for custom types based on annotations.

## Features

- **Automatic Code Generation**: Generate JSON encoders/decoders from annotated Gleam types
- **Configuration File**: Use `gloss.toml` for project-wide settings
- **Separate File Generation**: Create distinct generated files per module
- **Flexible Field Naming**: Support for both `snake_case` and `camelCase` JSON field names
- **Custom Type Tags**: Customize discriminator field names (default: `"type"`)
- **Custom Field Names**: Override individual field names in JSON
- **Maybe-Absent Field Handling**: Fine-grained control over absent Option field behavior
- **Type-Safe**: Generates type-safe Gleam code that integrates with `gleam/json` and `gleam/dynamic/decode`
- **Cascading Configuration**: Combine project, directory, file, and type settings with predictable precedence
- **Custom Decoder Failures**: Override the fallback message emitted when a variant cannot be decoded
- **External Function Hooks**: Point fields at existing encoder/decoder functions and gloss will wire up the imports
- **Custom Function Names**: Pattern-based encoder/decoder naming at global, directory, file, or type scope

## Installation

```bash
cargo build --release
# The binary will be at target/release/gloss
```

## Quick Start

1. **Create `gloss.toml`** in your project root:

```toml
field_naming = "snake_case"
absent_field_mode = "error_if_absent"

[output]
separate_files = true
file_pattern = "{module}_gloss.gleam"
directory = "gen"

[fn_naming]
encoder_fn_pattern = "encode_{type_pascal}"
decoder_fn_pattern = "decode_{type_pascal}"
```

2. **Annotate your types**:

```gleam
// gloss!: encoder(json), decoder
pub type User {
  User(name: String, age: Int)
}
```

3. **Generate code**:

```bash
gloss generate
```

## CLI Usage

```bash
gloss generate [OPTIONS]
```

### Options

- `--path, -p`: Path to the Gleam project root (default: current directory)
- `--dry-run`: Preview generated code without writing files
- `--verbose, -v`: Show detailed output

### Examples

```bash
# Generate using gloss.toml configuration
gloss generate

# Dry run to preview
gloss generate --dry-run

# Verbose output
gloss generate --verbose
```

## Configuration File

Create a `gloss.toml` in your project root:

```toml
# Global field naming convention
field_naming = "snake_case"  # or "camel_case"

# How to handle absent fields for Option(T) types
# - error_if_absent: Option(T) must be present (can be null)
# - maybe_absent: Option(T) can be absent from JSON
absent_field_mode = "error_if_absent"  # default

[output]
# Whether to create separate files per module
separate_files = true

# Whether to create separate files for encoders and decoders
# When true, generates encode_{module}.gleam and decode_{module}.gleam
# When false, generates {file_pattern} with both encoders and decoders
separate_encoder_decoder = false  # default

# File naming pattern for combined files (when separate_encoder_decoder = false)
# Placeholders: {module}, {module_snake}, {module_pascal}
file_pattern = "{module}_gloss.gleam"

# Encoder file pattern (when separate_encoder_decoder = true)
encoder_pattern = "encode_{module}.gleam"

# Decoder file pattern (when separate_encoder_decoder = true)
decoder_pattern = "decode_{module}.gleam"

# Output directory (relative to project root)
# If not specified, outputs to same directory as source
directory = "gen"  # optional

[fn_naming]
# Function naming patterns (placeholders: {type}, {type_snake}, {type_pascal})
encoder_fn_pattern = "encode_{type_pascal}"
decoder_fn_pattern = "decode_{type_pascal}"
```

### Configuration hierarchy

Gloss resolves settings from the widest scope to the most specific:

1. `gloss.toml` in the project root
2. `gloss.toml` files in subdirectories between the root and the Gleam file
3. File-level annotations (`// gloss-file!:`) at the top of a Gleam source file
4. Type-level annotations (`// gloss!:`) immediately above a custom type

Values defined later in the list override earlier ones. This mirrors CSS-style specificity and lets you tune output on a directory, file, or individual type without repeating global defaults.

### Field Naming

- `snake_case`: Converts `user_name` → `"user_name"`
- `camel_case`: Converts `user_name` → `"userName"`

### Absent Field Mode

Controls how `Option(T)` fields are handled when absent from JSON:

- **`error_if_absent`** (default): `Option(T)` fields **must be present** in JSON (but can be `null`)
  - Use `// gloss!: maybe_absent` on specific fields to allow them to be absent
- **`maybe_absent`**: `Option(T)` fields **can be absent** from JSON
  - Use `// gloss!: must_exist` on specific fields to require them to be present

### File Patterns

Use placeholders in `file_pattern`:
- `{module}`: Original module name (e.g., `user_types`)
- `{module_snake}`: snake_case (e.g., `user_types`)
- `{module_pascal}`: PascalCase (e.g., `UserTypes`)

Examples:
- `"{module}_gloss.gleam"` → `user_types_gloss.gleam`
- `"{module_pascal}_gloss.gleam"` → `UserTypes_gloss.gleam`
- `"{module}.gen.gleam"` → `user_types.gen.gleam`

### Function Naming

Use `[fn_naming]` to control the generated encoder/decoder function names. Patterns support the same placeholders used elsewhere:

- `{type}` → original type name (e.g., `UserProfile`)
- `{type_snake}` → snake_case (`user_profile`)
- `{type_pascal}` → PascalCase (`UserProfile`)

You can override the patterns at any scope:

- Project-wide via `gloss.toml` in the `fn_naming` table
- Directory-level with `// gloss-file!: encoder_fn = "...", decoder_fn = "..."`
- Type-level with `// gloss!: encoder_fn = "...", decoder_fn = "..."`

## Annotations

### Type Annotations

```gleam
// gloss!: encoder(json), decoder
pub type User {
  User(name: String, age: Int)
}
```

**Options:**
- `encoder(json)`: Generate JSON encoder function
- `decoder`: Generate dynamic decoder function
- `snake_case`: Override to use snake_case for this type
- `camelCase`: Override to use camelCase for this type
- `type_tag = "field_name"`: Customize type discriminator field (default: `"type"`)
- `no_type_tag`: Disable type tags for multi-variant types
- `unknown_variant_message = "..."`: Customise the decoder failure message when no constructor matches (`{type}` is replaced with the Gleam type name)
- Output overrides (`output_dir`, `file_pattern`, `encoder_pattern`, `decoder_pattern`, `separate_encoder_decoder`) mirror the keys in `gloss.toml`
- `encoder_fn = "pattern"`, `decoder_fn = "pattern"`: Override function names using the same placeholders as `[fn_naming]`

### Field Annotations

```gleam
// gloss!: encoder(json), decoder
pub type Voice {
  Voice(
    // gloss!: maybe_absent
    mime_type: Option(String),
    // gloss!: rename = "customName"
    file_name: String,
  )
}
```

**Field Options:**
- `maybe_absent`: Field can be absent from JSON, decoded as `option.None` (also accepts the older `optional` keyword)
- `must_exist`: Field must be present (even if null) (also accepts the older `required` keyword)
- `rename = "name"`: Use custom JSON field name
- `decoder_with = "module.function"`: Call a specific decoder function for this field (gloss will add the import)
- `encoder_with = "module.function"`: Call a specific encoder function for this field

### External encoder/decoder functions

When a field relies on a type that gloss does not generate, point to the existing helpers:

```gleam
// gloss!: decoder_with = "profile/codec.profile_decoder", encoder_with = "profile/codec.profile_to_json"
profile: profile.Profile
```

Gloss imports `profile/codec` (aliased to `profile_codec`) and calls the supplied functions. If the referenced type is generated by gloss, the generator automatically imports the module that owns the type and reuses its generated helpers (`user.user_decoder()`, `user.user_to_json(...)`, and so on).

## Examples

### Simple Type

```gleam
// gloss!: encoder(json), decoder
pub type User {
  User(
    name: String,
    age: Int,
  )
}
```

**Generated Code:**

```gleam
import gleam/dynamic/decode
import gleam/json

pub fn user_decoder() -> decode.Decoder(User) {
  use name <- decode.field("name", decode.string)
  use age <- decode.field("age", decode.int)
  decode.success(User(name:, age:))
}

pub fn user_to_json(user: User) -> json.Json {
  let User(name:, age:) = user
  json.object([
    #("name", json.string(name)),
    #("age", json.int(age)),
  ])
}
```

### Type with camelCase

```gleam
// gloss!: encoder(json), decoder, camelCase
pub type User {
  User(
    user_name: String,
    user_age: Int,
  )
}
```

**Generated JSON uses camelCase:**

```json
{
  "userName": "Alice",
  "userAge": 30
}
```

### Enum Type

```gleam
// gloss!: encoder(json), decoder
pub type Status {
  Active
  Inactive
  Pending
}
```

**Generated JSON:**

```json
"active"  // or "inactive", "pending"
```

### Tagged Union with Custom Type Tag

```gleam
// gloss!: encoder(json), decoder, type_tag = "kind"
pub type Message {
  Text(content: String)
  Image(url: String, width: Int, height: Int)
  Video(url: String, duration: Int)
}
```

**Generated JSON uses "kind" instead of "type":**

```json
{
  "kind": "image",
  "url": "https://example.com/image.png",
  "width": 800,
  "height": 600
}
```

### Custom Field Names

```gleam
// gloss!: encoder(json), decoder
pub type Product {
  Product(
    // gloss!: rename = "productName"
    name: String,
    // gloss!: rename = "productPrice"
    price: Float,
  )
}
```

**Generated JSON:**

```json
{
  "productName": "Widget",
  "productPrice": 19.99
}
```

### Maybe-Absent Fields

```gleam
// gloss!: encoder(json), decoder
pub type Voice {
  Voice(
    // gloss!: maybe_absent - can be absent from JSON
    mime_type: Option(String),
    // Required but nullable
    file_size: Option(Int),
  )
}
```

**Decoder behavior:**
- `mime_type`: Uses `decode.optional_field` - can be absent from JSON, decoded as `option.None`
- `file_size`: Uses `decode.field` - must be present but can be `null`

**Accepted JSON:**

```json
// Valid - both present
{"mime_type": "image/png", "file_size": 1024}

// Valid - mime_type absent, file_size null
{"file_size": null}

// Invalid - file_size absent
{"mime_type": "image/png"}  // ❌ Error: missing field "file_size"
```

## Advanced Features

### Separate File Generation

Configure in `gloss.toml`:

```toml
[output]
separate_files = true
file_pattern = "{module}_gloss.gleam"
directory = "gen"
```

Result:
```
src/user.gleam       → gen/user_gloss.gleam
src/product.gleam    → gen/product_gloss.gleam
```

### Separate Encoder/Decoder Files

You can generate encoders and decoders into separate files for better organization:

```toml
[output]
separate_files = true
separate_encoder_decoder = true
encoder_pattern = "encode_{module}.gleam"
decoder_pattern = "decode_{module}.gleam"
directory = "gen"
```

Result:
```
src/user.gleam → gen/encode_user.gleam  (contains only encoders)
               → gen/decode_user.gleam  (contains only decoders)
```

**Benefits:**
- Cleaner separation of concerns
- Smaller import footprint (decoders don't import `gleam/json`, encoders don't import `gleam/dynamic/decode`)
- Easier to tree-shake unused code
- Better for large projects with many types

**Generated `decode_user.gleam`:**
```gleam
import gleam/dynamic/decode
import gleam/option

pub fn user_decoder() -> decode.Decoder(User) {
  use name <- decode.field("name", decode.string)
  use age <- decode.field("age", decode.int)
  decode.success(User(name:, age:))
}
```

**Generated `encode_user.gleam`:**
```gleam
import gleam/json

pub fn user_to_json(user: User) -> json.Json {
  let User(name:, age:) = user
  json.object([
    #("name", json.string(name)),
    #("age", json.int(age)),
  ])
}
```

### Inline Generation

```toml
[output]
separate_files = false
```

Result: Generated code is appended to source files with a marker comment.

### Configuration Modes

#### Optional Field Mode: error_if_absent (default)

```toml
absent_field_mode = "error_if_absent"
```

```gleam
pub type Example {
  Example(
    field: Option(String),  // Must be present, can be null
    // gloss!: maybe_absent
    seen: Option(String),  // Can be absent
  )
}
```

#### Optional Field Mode: maybe_absent

```toml
absent_field_mode = "maybe_absent"
```

```gleam
pub type Example {
  Example(
    field: Option(String),  // Can be absent
    // gloss!: must_exist
    confirmed: Option(String),  // Must be present, can be null
  )
}
```

## Project Structure

```
gloss/
├── gloss_core/     # Core library with parser and generator logic
├── gloss_cli/      # CLI application
└── test_project/   # Example project for testing
```

## Dependencies

The generated code requires these Gleam packages:

- `gleam_json`: For JSON encoding
- `gleam_stdlib`: For `gleam/dynamic/decode` and `gleam/option`

## Complete Example

**Input** (`src/api.gleam`):

```gleam
import gleam/option.{type Option}

// gloss!: encoder(json), decoder, camelCase, type_tag = "messageType"
pub type ApiMessage {
  UserCreated(
    // gloss!: rename = "userId"
    id: String,
    user_name: String,
  )
  UserUpdated(
    id: String,
    // gloss!: maybe_absent
    email: Option(String),
  )
}
```

**Configuration** (`gloss.toml`):

```toml
field_naming = "snake_case"
absent_field_mode = "error_if_absent"

[output]
separate_files = true
file_pattern = "{module}_gloss.gleam"
directory = "gloss"
```

**Generated** (`gloss/api_gloss.gleam`):

```gleam
import gleam/dynamic/decode
import gleam/json
import gleam/option

pub fn api_message_decoder() -> decode.Decoder(ApiMessage) {
  use variant <- decode.field("messageType", decode.string)
  case variant {
    "user_created" -> {
      use id <- decode.field("userId", decode.string)
      use user_name <- decode.field("userName", decode.string)
      decode.success(UserCreated(id:, user_name:))
    }
    "user_updated" -> {
      use id <- decode.field("id", decode.string)
      use email <- decode.optional_field("email", option.None, decode.optional(decode.string))
      decode.success(UserUpdated(id:, email:))
    }
    _ -> decode.failure(
      UserCreated(id: "", user_name: ""),
      "one of user_created, user_updated",
    )
  }
}

pub fn api_message_to_json(api_message: ApiMessage) -> json.Json {
  case api_message {
    UserCreated(id:, user_name:) -> json.object([
      #("messageType", json.string("user_created")),
      #("userId", json.string(id)),
      #("userName", json.string(user_name)),
    ])
    UserUpdated(id:, email:) -> json.object([
      #("messageType", json.string("user_updated")),
      #("id", json.string(id)),
      #("email", json.nullable(email, _)),
    ])
  }
}
```

## Troubleshooting

### Config file not found

Gloss will use default configuration if `gloss.toml` is not found. Create it in your project root for custom settings.

### Generated files not appearing

Check your `gloss.toml` output configuration. If `separate_files = false`, code is appended to source files with a marker comment.
