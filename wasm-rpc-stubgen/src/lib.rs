// Copyright 2024 Golem Cloud
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

mod cargo;
mod compilation;
mod make;
mod rust;
mod stub;
mod wit;

use crate::cargo::generate_cargo_toml;
use crate::compilation::compile;
use crate::rust::generate_stub_source;
use crate::stub::StubDefinition;
use crate::wit::{copy_wit_files, generate_stub_wit, verify_action, WitAction};
use anyhow::{anyhow, Context};
use clap::Parser;
use fs_extra::dir::CopyOptions;
use golem_wasm_ast::analysis::{AnalysedExport, AnalysisContext, AnalysisFailure};
use golem_wasm_ast::component::Component;
use golem_wasm_ast::IgnoreAllButMetadata;
use heck::ToSnakeCase;
use std::fs;
use std::path::PathBuf;
use tempdir::TempDir;
use wasm_compose::config::Dependency;

#[derive(Parser, Debug)]
#[command(name = "wasm-rpc-stubgen")]
#[command(bin_name = "wasm-rpc-stubgen")]
pub enum Command {
    Generate(GenerateArgs),
    Build(BuildArgs),
    AddStubDependency(AddStubDependencyArgs),
    Compose(ComposeArgs),
    InitializeWorkspace(InitializeWorkspaceArgs),
}

/// Generate a Rust RPC stub crate for a WASM component
#[derive(clap::Args, Debug)]
#[command(version, about, long_about = None)]
pub struct GenerateArgs {
    #[clap(short, long)]
    pub source_wit_root: PathBuf,
    #[clap(short, long)]
    pub dest_crate_root: PathBuf,
    #[clap(short, long)]
    pub world: Option<String>,
    #[clap(long, default_value = "0.0.1")]
    pub stub_crate_version: String,
    #[clap(long)]
    pub wasm_rpc_path_override: Option<String>,
}

/// Build an RPC stub for a WASM component
#[derive(clap::Args, Debug)]
#[command(version, about, long_about = None)]
pub struct BuildArgs {
    #[clap(short, long)]
    pub source_wit_root: PathBuf,
    #[clap(long)]
    pub dest_wasm: PathBuf,
    #[clap(long)]
    pub dest_wit_root: PathBuf,
    #[clap(short, long)]
    pub world: Option<String>,
    #[clap(long, default_value = "0.0.1")]
    pub stub_crate_version: String,
    #[clap(long)]
    pub wasm_rpc_path_override: Option<String>,
}

/// Adds a generated stub as a dependency to another WASM component
#[derive(clap::Args, Debug)]
#[command(version, about, long_about = None)]
pub struct AddStubDependencyArgs {
    #[clap(short, long)]
    pub stub_wit_root: PathBuf,
    #[clap(short, long)]
    pub dest_wit_root: PathBuf,
    #[clap(short, long)]
    pub overwrite: bool,
    #[clap(short, long)]
    pub update_cargo_toml: bool,
}

/// Compose a WASM component with a generated stub WASM
#[derive(clap::Args, Debug)]
#[command(version, about, long_about = None)]
pub struct ComposeArgs {
    #[clap(long)]
    pub source_wasm: PathBuf,
    #[clap(long, required = true)]
    pub stub_wasm: Vec<PathBuf>,
    #[clap(long)]
    pub dest_wasm: PathBuf,
}

/// Initializes a Golem-specific cargo-make configuration in a Cargo workspace for automatically
/// generating stubs and composing results.
#[derive(clap::Args, Debug)]
#[command(version, about, long_about = None)]
pub struct InitializeWorkspaceArgs {
    /// List of subprojects to be called via RPC
    #[clap(long, required = true)]
    pub targets: Vec<String>,
    /// List of subprojects using the generated stubs for calling remote workers
    #[clap(long, required = true)]
    pub callers: Vec<String>,

    #[clap(long)]
    pub wasm_rpc_path_override: Option<String>,
}

pub fn generate(args: GenerateArgs) -> anyhow::Result<()> {
    let stub_def = StubDefinition::new(
        &args.source_wit_root,
        &args.dest_crate_root,
        &args.world,
        &args.stub_crate_version,
        &args.wasm_rpc_path_override,
    )
    .context("Failed to gather information for the stub generator")?;

    generate_stub_wit(&stub_def).context("Failed to generate the stub wit file")?;
    copy_wit_files(&stub_def).context("Failed to copy the dependent wit files")?;
    stub_def
        .verify_target_wits()
        .context("Failed to resolve the result WIT root")?;
    generate_cargo_toml(&stub_def).context("Failed to generate the Cargo.toml file")?;
    generate_stub_source(&stub_def).context("Failed to generate the stub Rust source")?;
    Ok(())
}

pub async fn build(args: BuildArgs) -> anyhow::Result<()> {
    let target_root = TempDir::new("wasm-rpc-stubgen")?;

    let stub_def = StubDefinition::new(
        &args.source_wit_root,
        target_root.path(),
        &args.world,
        &args.stub_crate_version,
        &args.wasm_rpc_path_override,
    )
    .context("Failed to gather information for the stub generator")?;

    generate_stub_wit(&stub_def).context("Failed to generate the stub wit file")?;
    copy_wit_files(&stub_def).context("Failed to copy the dependent wit files")?;
    stub_def
        .verify_target_wits()
        .context("Failed to resolve the result WIT root")?;
    generate_cargo_toml(&stub_def).context("Failed to generate the Cargo.toml file")?;
    generate_stub_source(&stub_def).context("Failed to generate the stub Rust source")?;

    compile(target_root.path())
        .await
        .context("Failed to compile the generated stub")?;

    let wasm_path = target_root
        .path()
        .join("target")
        .join("wasm32-wasi")
        .join("release")
        .join(format!(
            "{}.wasm",
            stub_def.target_crate_name()?.to_snake_case()
        ));
    if let Some(parent) = args.dest_wasm.parent() {
        fs::create_dir_all(parent)
            .context("Failed to create parent directory of the target WASM file")?;
    }
    fs::copy(wasm_path, &args.dest_wasm)
        .context("Failed to copy the WASM file to the destination")?;

    fs::create_dir_all(&args.dest_wit_root)
        .context("Failed to create the target WIT root directory")?;

    fs_extra::dir::copy(
        target_root.path().join("wit"),
        &args.dest_wit_root,
        &CopyOptions::new().content_only(true).overwrite(true),
    )
    .context("Failed to copy the generated WIT files to the destination")?;

    Ok(())
}

pub fn add_stub_dependency(args: AddStubDependencyArgs) -> anyhow::Result<()> {
    let source_deps = wit::get_dep_dirs(&args.stub_wit_root)?;

    let main_wit = args.stub_wit_root.join("_stub.wit");
    let main_wit_package_name = wit::get_package_name(&main_wit)?;

    let mut actions = Vec::new();
    for source_dir in source_deps {
        actions.push(WitAction::CopyDepDir { source_dir })
    }
    actions.push(WitAction::CopyDepWit {
        source_wit: main_wit,
        dir_name: format!(
            "{}_{}",
            main_wit_package_name.namespace, main_wit_package_name.name
        ),
    });

    let mut proceed = true;
    for action in &actions {
        if !verify_action(action, &args.dest_wit_root, args.overwrite)? {
            eprintln!("Cannot {action} because the destination already exists with a different content. Use --overwrite to force.");
            proceed = false;
        }
    }

    if proceed {
        for action in &actions {
            action.perform(&args.dest_wit_root)?;
        }
    }

    if let Some(target_parent) = args.dest_wit_root.parent() {
        let target_cargo_toml = target_parent.join("Cargo.toml");
        if target_cargo_toml.exists()
            && target_cargo_toml.is_file()
            && cargo::is_cargo_component_toml(&target_cargo_toml).is_ok()
        {
            if !args.update_cargo_toml {
                eprintln!("Warning: the newly copied dependencies have to be added to {}. Use the --update-cargo-toml flag to update it automatically.", target_cargo_toml.to_string_lossy());
            } else {
                let mut names = Vec::new();
                for action in actions {
                    names.push(action.get_dep_dir_name()?);
                }
                cargo::add_dependencies_to_cargo_toml(&target_cargo_toml, &names)?;
            }
        } else if args.update_cargo_toml {
            return Err(anyhow!(
                "Cannot update {:?} file because it does not exist or is not a file",
                target_cargo_toml
            ));
        }
    } else if args.update_cargo_toml {
        return Err(anyhow!("Cannot update the Cargo.toml file because parent directory of the destination WIT root does not exist."));
    }

    Ok(())
}

pub fn compose(args: ComposeArgs) -> anyhow::Result<()> {
    let mut config = wasm_compose::config::Config::default();

    for stub_wasm in &args.stub_wasm {
        let stub_bytes = fs::read(stub_wasm)?;
        let stub_component = Component::<IgnoreAllButMetadata>::from_bytes(&stub_bytes)
            .map_err(|err| anyhow!(err))?;

        let state = AnalysisContext::new(stub_component);
        let stub_exports = state.get_top_level_exports().map_err(|err| match err {
            AnalysisFailure::Failed(msg) => anyhow!(msg),
        })?;

        for export in stub_exports {
            if let AnalysedExport::Instance(instance) = export {
                config.dependencies.insert(
                    instance.name.clone(),
                    Dependency {
                        path: stub_wasm.clone(),
                    },
                );
            }
        }
    }

    let composer = wasm_compose::composer::ComponentComposer::new(&args.source_wasm, &config);
    let result = composer.compose()?;
    println!("Writing composed component to {:?}", args.dest_wasm);
    fs::write(&args.dest_wasm, result).context("Failed to write the composed component")?;
    Ok(())
}

pub fn initialize_workspace(
    args: InitializeWorkspaceArgs,
    stubgen_command: &str,
    stubgen_prefix: &[&str],
) -> anyhow::Result<()> {
    make::initialize_workspace(
        &args.targets,
        &args.callers,
        args.wasm_rpc_path_override,
        stubgen_command,
        stubgen_prefix,
    )
}