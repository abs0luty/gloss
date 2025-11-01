use std::fs;

use camino::Utf8PathBuf;
use gloss_core::{generate_for_project, BackendRegistry};
use tempfile::tempdir;

fn write_gleam_manifest(root: &Utf8PathBuf) {
    fs::write(
        root.join("gleam.toml"),
        r#"[project]
name = "app"
version = "1.0.0"

[dependencies]
"gleam/json" = "~> 1.0"
"#,
    )
    .expect("write gleam.toml");
}

#[test]
fn unknown_variant_message_overrides() {
    let temp = tempdir().expect("temp dir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf8 path");

    write_gleam_manifest(&root);

    fs::create_dir_all(root.join("src")).expect("create src dir");

    fs::write(
        root.join("src/events.gleam"),
        r#"
// gloss-file!: unknown_variant_message = "Unknown {type} variant encountered"

// gloss!: encoder(json), decoder
pub type Event {
  EventA(count: Int)
  EventB(label: String)
}
"#,
    )
    .expect("write events module");

    let registry = BackendRegistry::new();
    let generated = generate_for_project(&root, &registry).expect("generate project");

    let events_path = root.join("src/events.gleam");
    let groups = generated
        .get(&events_path)
        .expect("events module generated");
    assert_eq!(groups.len(), 1);

    let decoder_code = groups[0].get_decoder_code(true, false);
    assert!(decoder_code.contains("decode.failure("));
    assert!(!decoder_code.contains("todo as"));
    assert!(decoder_code.contains("\"Unknown Event variant encountered\""));
}

#[test]
fn field_level_overrides_use_external_functions() {
    let temp = tempdir().expect("temp dir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf8 path");

    write_gleam_manifest(&root);

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");

    fs::write(
        src_dir.join("profile.gleam"),
        r#"
pub type Profile {
  Profile(id: Int)
}
"#,
    )
    .expect("write profile module");

    fs::write(
        src_dir.join("wrapper.gleam"),
        r#"
import profile

// gloss!: encoder(json), decoder
pub type Wrapper {
  Wrapper(
    // gloss!: decoder_with = "profile/codec.profile_decoder", encoder_with = "profile/codec.profile_to_json"
    profile: profile.Profile
  )
}
"#,
    )
    .expect("write wrapper module");

    let registry = BackendRegistry::new();
    let generated = generate_for_project(&root, &registry).expect("generate project");

    let wrapper_path = src_dir.join("wrapper.gleam");
    let groups = generated
        .get(&wrapper_path)
        .expect("wrapper module generated");
    assert_eq!(groups.len(), 1);

    let group = &groups[0];
    let decoder_code = group.get_decoder_code(true, false);
    assert!(decoder_code.contains("import profile/codec as profile_codec"));
    assert!(decoder_code.contains("profile_codec.profile_decoder()"));

    let encoder_code = group.get_encoder_code(true, false);
    assert!(encoder_code.contains("profile_codec.profile_to_json(profile)"));

    assert!(group
        .custom_imports
        .values()
        .any(|entry| entry.module_path == "profile/codec"));
}

#[test]
fn generated_type_dependencies_are_imported() {
    let temp = tempdir().expect("temp dir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf8 path");

    write_gleam_manifest(&root);

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");

    fs::write(
        src_dir.join("user.gleam"),
        r#"
// gloss!: encoder(json), decoder
pub type User {
  User(id: Int)
}
"#,
    )
    .expect("write user module");

    fs::write(
        src_dir.join("order.gleam"),
        r#"
import user

// gloss!: encoder(json), decoder
pub type Order {
  Order(owner: user.User)
}
"#,
    )
    .expect("write order module");

    let registry = BackendRegistry::new();
    let generated = generate_for_project(&root, &registry).expect("generate project");

    let order_path = src_dir.join("order.gleam");
    let groups = generated.get(&order_path).expect("order module generated");
    assert_eq!(groups.len(), 1);

    let group = &groups[0];
    let decoder_code = group.get_decoder_code(true, false);
    assert!(decoder_code.contains("import user"));
    assert!(decoder_code.contains("user.user_decoder()"));

    let encoder_code = group.get_encoder_code(true, false);
    assert!(encoder_code.contains("user.user_to_json(owner)"));

    assert!(group
        .custom_imports
        .values()
        .any(|entry| entry.module_path == "user"));
}
