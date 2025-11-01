use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use clap::{Parser, Subcommand};
use gloss_core::Config;
use std::fs;

#[derive(Parser)]
#[command(name = "gloss")]
#[command(about = "Generate JSON encoders/decoders for Gleam types", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate encoders/decoders for marked types in a Gleam project
    Generate {
        /// Path to the Gleam project root
        #[arg(short, long, default_value = ".")]
        path: String,

        /// Dry run - print generated code without writing files
        #[arg(long)]
        dry_run: bool,

        /// Show verbose output
        #[arg(short, long)]
        verbose: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Generate {
            path,
            dry_run,
            verbose,
        } => {
            let project_path = Utf8PathBuf::from(path);

            // Load config from gloss.toml or use defaults
            let config = Config::load_or_default(&project_path);

            if verbose {
                println!("Scanning Gleam project at: {}", project_path);
                println!("Configuration:");
                println!("  Field naming: {:?}", config.field_naming);
                println!("  Absent field mode: {:?}", config.absent_field_mode);
                println!("  Separate files: {}", config.output.separate_files);
                println!("  File pattern: {}", config.output.file_pattern);
                if let Some(ref dir) = config.output.directory {
                    println!("  Output directory: {}", dir);
                }
                println!();
            }

            let generated = gloss_core::generate_for_project(&project_path, &config)
                .context("Failed to generate encoders/decoders")?;

            if generated.is_empty() {
                println!("No types found with gloss!: annotations.");
                println!();
                println!("To generate encoders/decoders, add annotations to your custom types:");
                println!("  // gloss!: encoder(json), decoder");
                println!("  pub type MyType {{");
                println!("    MyType(");
                println!("      field: String,");
                println!("      // gloss!: maybe_absent");
                println!("      maybe_absent_field: Option(Int),");
                println!("    )");
                println!("  }}");
                println!();
                println!("See gloss.toml for configuration options.");
                return Ok(());
            }

            if verbose {
                println!("Generated code for {} module(s)", generated.len());
                println!();
            }

            write_generated_outputs(&project_path, generated, dry_run, verbose)?;

            if dry_run {
                println!("\n✓ Dry run complete. No files were modified.");
            } else {
                println!("\n✓ Code generation complete!");
            }
        }
    }

    Ok(())
}

fn write_generated_outputs(
    project_path: &Utf8PathBuf,
    generated: std::collections::HashMap<Utf8PathBuf, Vec<gloss_core::GeneratedCode>>,
    dry_run: bool,
    verbose: bool,
) -> Result<()> {
    for (source_file, groups) in generated {
        let module_name = source_file.file_stem().unwrap_or("unknown").to_string();

        let mut inline_groups = Vec::new();

        for gen_code in groups {
            if gen_code.output_config.separate_files {
                if gen_code.output_config.separate_encoder_decoder {
                    write_group_separate_encoder_decoder(
                        project_path,
                        &source_file,
                        &module_name,
                        &gen_code,
                        dry_run,
                        verbose,
                    )?;
                } else {
                    write_group_separate_file(
                        project_path,
                        &source_file,
                        &module_name,
                        &gen_code,
                        dry_run,
                        verbose,
                    )?;
                }
            } else {
                inline_groups.push(gen_code);
            }
        }

        if !inline_groups.is_empty() {
            write_inline_groups(&source_file, inline_groups, dry_run, verbose)?;
        }
    }

    Ok(())
}

fn write_group_separate_file(
    project_path: &Utf8PathBuf,
    source_file: &Utf8PathBuf,
    module_name: &str,
    gen_code: &gloss_core::GeneratedCode,
    dry_run: bool,
    verbose: bool,
) -> Result<()> {
    let code = gen_code.get_combined_code(true, true);
    let output_filename = apply_file_pattern(&gen_code.output_config.file_pattern, module_name);
    let output_path = resolve_output_path(
        project_path,
        source_file,
        &gen_code.output_config.directory,
        gen_code.path_mode,
        &output_filename,
    );

    if verbose || dry_run {
        println!("Module: {}", module_name);
        println!("Source: {}", source_file);
        println!("Output: {}", output_path);
        println!("{}", "=".repeat(80));
    }

    if dry_run {
        println!("{}\n", code);
    } else {
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .context(format!("Failed to create directory: {}", parent))?;
        }

        fs::write(&output_path, code).context(format!("Failed to write to {}", output_path))?;

        if verbose {
            println!("✓ Written to: {}\n", output_path);
        }
    }

    Ok(())
}

fn write_group_separate_encoder_decoder(
    project_path: &Utf8PathBuf,
    source_file: &Utf8PathBuf,
    module_name: &str,
    gen_code: &gloss_core::GeneratedCode,
    dry_run: bool,
    verbose: bool,
) -> Result<()> {
    let decoder_code = gen_code.get_decoder_code(true, true);
    if !decoder_code.trim().is_empty() {
        let decoder_filename =
            apply_file_pattern(&gen_code.output_config.decoder_pattern, module_name);
        let decoder_path = resolve_output_path(
            project_path,
            source_file,
            &gen_code.output_config.directory,
            gen_code.path_mode,
            &decoder_filename,
        );

        if verbose || dry_run {
            println!("Module: {} (decoder)", module_name);
            println!("Source: {}", source_file);
            println!("Output: {}", decoder_path);
            println!("{}", "=".repeat(80));
        }

        if dry_run {
            println!("{}\n", decoder_code);
        } else {
            if let Some(parent) = decoder_path.parent() {
                fs::create_dir_all(parent)
                    .context(format!("Failed to create directory: {}", parent))?;
            }
            fs::write(&decoder_path, decoder_code)
                .context(format!("Failed to write to {}", decoder_path))?;
            if verbose {
                println!("✓ Written to: {}\n", decoder_path);
            }
        }
    }

    let encoder_code = gen_code.get_encoder_code(true, true);
    if !encoder_code.trim().is_empty() {
        let encoder_filename =
            apply_file_pattern(&gen_code.output_config.encoder_pattern, module_name);
        let encoder_path = resolve_output_path(
            project_path,
            source_file,
            &gen_code.output_config.directory,
            gen_code.path_mode,
            &encoder_filename,
        );

        if verbose || dry_run {
            println!("Module: {} (encoder)", module_name);
            println!("Source: {}", source_file);
            println!("Output: {}", encoder_path);
            println!("{}", "=".repeat(80));
        }

        if dry_run {
            println!("{}\n", encoder_code);
        } else {
            if let Some(parent) = encoder_path.parent() {
                fs::create_dir_all(parent)
                    .context(format!("Failed to create directory: {}", parent))?;
            }
            fs::write(&encoder_path, encoder_code)
                .context(format!("Failed to write to {}", encoder_path))?;
            if verbose {
                println!("✓ Written to: {}\n", encoder_path);
            }
        }
    }

    Ok(())
}

fn write_inline_groups(
    file_path: &Utf8PathBuf,
    mut groups: Vec<gloss_core::GeneratedCode>,
    dry_run: bool,
    verbose: bool,
) -> Result<()> {
    if groups.is_empty() {
        return Ok(());
    }

    let mut combined = groups.remove(0);
    for group in groups {
        combined.types.extend(group.types);
        for (module_path, entry) in group.custom_imports {
            match combined.custom_imports.entry(module_path) {
                std::collections::btree_map::Entry::Occupied(mut existing) => {
                    let existing_entry = existing.get_mut();
                    existing_entry.values.extend(entry.values.into_iter());
                    existing_entry.types.extend(entry.types.into_iter());
                }
                std::collections::btree_map::Entry::Vacant(vacant) => {
                    vacant.insert(entry);
                }
            }
        }
    }

    let code = combined.get_combined_code(true, false);
    if verbose || dry_run {
        println!("File: {}", file_path);
        println!("{}", "=".repeat(80));
    }

    if dry_run {
        println!("{}\n", code);
    } else {
        let existing_content =
            fs::read_to_string(file_path).context(format!("Failed to read {}", file_path))?;

        let marker = "\n\n// ========== Generated by gloss ==========\n\n";
        let new_content =
            if existing_content.contains("// ========== Generated by gloss ==========") {
                let parts: Vec<&str> = existing_content.split(marker).collect();
                format!("{}{}{}", parts[0], marker, code)
            } else {
                format!("{}{}{}", existing_content, marker, code)
            };

        fs::write(file_path, new_content).context(format!("Failed to write to {}", file_path))?;

        if verbose {
            println!("✓ Appended to: {}\n", file_path);
        }
    }

    Ok(())
}

fn apply_file_pattern(pattern: &str, module_name: &str) -> String {
    pattern
        .replace("{module}", module_name)
        .replace("{module_snake}", &to_snake_case(module_name))
        .replace("{module_pascal}", &to_pascal_case(module_name))
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

fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;

    for ch in s.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_uppercase().next().unwrap());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }

    result
}

/// Resolve output path based on path mode and configuration
fn resolve_output_path(
    project_root: &Utf8PathBuf,
    source_file: &Utf8PathBuf,
    output_dir: &Option<String>,
    path_mode: gloss_core::PathMode,
    filename: &str,
) -> Utf8PathBuf {
    use gloss_core::PathMode;

    if let Some(ref dir) = output_dir {
        let (mode, clean_dir) = normalize_directory(dir, path_mode);

        let base_dir = match mode {
            PathMode::ProjectRelative => project_root.clone(),
            PathMode::FileRelative => source_file.parent().unwrap().to_path_buf(),
        };

        if clean_dir.is_empty() {
            base_dir.join(filename)
        } else {
            base_dir.join(clean_dir).join(filename)
        }
    } else {
        // No directory specified, put next to source file
        source_file.parent().unwrap().join(filename)
    }
}

fn normalize_directory(
    dir: &str,
    default_mode: gloss_core::PathMode,
) -> (gloss_core::PathMode, String) {
    use gloss_core::PathMode;

    if dir.starts_with("@/") {
        (
            PathMode::ProjectRelative,
            dir.trim_start_matches("@/").to_string(),
        )
    } else if dir.starts_with('@') {
        let trimmed = dir.trim_start_matches('@').trim_start_matches('/');
        (PathMode::ProjectRelative, trimmed.to_string())
    } else if dir.starts_with('/') {
        (
            PathMode::ProjectRelative,
            dir.trim_start_matches('/').to_string(),
        )
    } else if dir.starts_with("./") {
        (
            PathMode::FileRelative,
            dir.trim_start_matches("./").to_string(),
        )
    } else {
        (default_mode, dir.to_string())
    }
}
