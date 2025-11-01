use std::fs;

use camino::Utf8PathBuf;
use gloss_core::{generate_for_project, BackendRegistry, PathMode};
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
fn cascading_configuration_respects_hierarchy() {
    let temp = tempdir().expect("temp dir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf8 path");

    write_gleam_manifest(&root);

    // Global config (project root)
    fs::write(
        root.join("gloss.toml"),
        r#"
field_naming_strategy = "snake_case"
absent_field_mode = "error_if_absent"

[output]
directory = "global_gen"
separate_files = true
separate_encoder_decoder = false
generated_file_naming = "{module}_root.gleam"
encode_module_naming = "root_encode_{module}.gleam"
decode_module_naming = "root_decode_{module}.gleam"
"#,
    )
    .expect("write global gloss.toml");

    // Subdirectory config
    let api_dir = root.join("src/api");
    fs::create_dir_all(&api_dir).expect("create api dir");
    fs::write(
        api_dir.join("gloss.toml"),
        r#"
[output]
directory = "@/api_gen"
separate_encoder_decoder = true
encode_module_naming = "api_encode_{module}.gleam"
decode_module_naming = "api_decode_{module}.gleam"
"#,
    )
    .expect("write api gloss.toml");

    // File without override (inherits directory config)
    fs::write(
        api_dir.join("account.gleam"),
        r#"
// gloss!: encoder(json), decoder
pub type Account {
  Account(id: String)
}
"#,
    )
    .expect("write account file");

    // File with file-level override
    fs::write(
        api_dir.join("user.gleam"),
        r#"
// gloss-file!: output_dir = "./file_gen", separate_encoder_decoder = false, encode_module_naming = "file_encode_{module}.gleam", decode_module_naming = "file_decode_{module}.gleam", generated_file_naming = "file_combined_{module}.gleam"

// gloss!: encoder(json), decoder
pub type User {
  User(id: String)
}
"#,
    )
    .expect("write user file");

    let registry = BackendRegistry::new();
    let generated = generate_for_project(&root, &registry).expect("generate project");

    let account_path = api_dir.join("account.gleam");
    let user_path = api_dir.join("user.gleam");

    let account_groups = generated
        .get(&account_path)
        .expect("account module generated");
    assert_eq!(
        account_groups.len(),
        1,
        "expected single group for account module"
    );
    let account_code = &account_groups[0];
    assert_eq!(
        account_code.output_config.directory.as_deref(),
        Some("@/api_gen")
    );
    assert!(account_code.output_config.separate_files);
    assert!(account_code.output_config.separate_encoder_decoder);
    assert_eq!(
        account_code.output_config.encode_module_naming,
        "api_encode_{module}.gleam"
    );
    assert_eq!(
        account_code.output_config.decode_module_naming,
        "api_decode_{module}.gleam"
    );
    assert_eq!(
        account_code.output_config.generated_file_naming,
        "{module}_root.gleam"
    );
    assert_eq!(account_code.path_mode, PathMode::ProjectRelative);

    let user_groups = generated.get(&user_path).expect("user module generated");
    assert_eq!(
        user_groups.len(),
        1,
        "expected single group for user module"
    );
    let user_code = &user_groups[0];
    assert_eq!(
        user_code.output_config.directory.as_deref(),
        Some("./file_gen")
    );
    assert!(user_code.output_config.separate_files);
    assert!(!user_code.output_config.separate_encoder_decoder);
    assert_eq!(
        user_code.output_config.encode_module_naming,
        "file_encode_{module}.gleam"
    );
    assert_eq!(
        user_code.output_config.decode_module_naming,
        "file_decode_{module}.gleam"
    );
    assert_eq!(
        user_code.output_config.generated_file_naming,
        "file_combined_{module}.gleam"
    );
    assert_eq!(user_code.path_mode, PathMode::FileRelative);
}

#[test]
fn type_level_overrides_apply_per_type() {
    let temp = tempdir().expect("temp dir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf8 path");

    write_gleam_manifest(&root);

    fs::write(
        root.join("gloss.toml"),
        r#"
[output]
directory = "@/global_gen"
separate_files = true
generated_file_naming = "{module}_global.gleam"
"#,
    )
    .expect("write root gloss.toml");

    let team_dir = root.join("src/team");
    fs::create_dir_all(&team_dir).expect("create team dir");

    fs::write(
        team_dir.join("members.gleam"),
        r#"
// gloss-file!: output_dir = "@/team_file_gen", decode_module_naming = "file_decode_{module}.gleam"

// gloss!: encoder(json), decoder
pub type Member {
  Member(name: String)
}

// gloss!: encoder(json), decoder, output_dir = "./local_override", encode_module_naming = "local_encode_{module}.gleam", decode_module_naming = "local_decode_{module}.gleam"
pub type Leader {
  Leader(name: String)
}
"#,
    )
    .expect("write members file");

    let registry = BackendRegistry::new();
    let generated = generate_for_project(&root, &registry).expect("generate project");

    let members_path = team_dir.join("members.gleam");
    let groups = generated
        .get(&members_path)
        .expect("members module generated");
    assert_eq!(
        groups.len(),
        2,
        "expected two output groups for members module"
    );

    let member_group = groups
        .iter()
        .find(|group| group.types.iter().any(|t| t.type_name == "Member"))
        .expect("member group");
    assert_eq!(
        member_group.output_config.directory.as_deref(),
        Some("@/team_file_gen")
    );
    assert_eq!(
        member_group.output_config.decode_module_naming,
        "file_decode_{module}.gleam"
    );
    assert_eq!(member_group.path_mode, PathMode::ProjectRelative);

    let leader_group = groups
        .iter()
        .find(|group| group.types.iter().any(|t| t.type_name == "Leader"))
        .expect("leader group");
    assert_eq!(
        leader_group.output_config.directory.as_deref(),
        Some("./local_override")
    );
    assert_eq!(
        leader_group.output_config.encode_module_naming,
        "local_encode_{module}.gleam"
    );
    assert_eq!(
        leader_group.output_config.decode_module_naming,
        "local_decode_{module}.gleam"
    );
    assert_eq!(leader_group.path_mode, PathMode::FileRelative);
}
