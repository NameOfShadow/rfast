//! `rfast` – Run Rust files like scripts, with instant caching and zero boilerplate.
//!
//! This is the main entry point of the `rfast` binary. It parses command‑line
//! arguments and dispatches to the appropriate subcommand or evaluates inline code.
//!
//! For full usage, run `rfast --help`.

mod cache;
mod parser;
mod runner;
mod ui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ColorChoice, ArgAction};
use clap_complete::{generate, Shell};
use std::path::PathBuf;
use colored::Colorize;
use std::io;
use crate::runner::DepSpec;

/// Print a beautifully formatted help message using the UI macros.
fn print_beautiful_help() {
    gap!();
    section!("rfast – Run Rust files like scripts");
    detail!("instant caching, inline dependencies, zero boilerplate");
    gap!();

    section!("Usage");
    detail!("rfast [OPTIONS] [SCRIPT] [ARGS]... [COMMAND]");
    gap!();

    section!("Commands");
    detail!("run          Run a script (compile if needed, then execute)");
    detail!("build        Build a script into a standalone binary");
    detail!("new          Create a new script from a template");
    detail!("add          Add a dependency to a script");
    detail!("cache        Show build‑cache information");
    detail!("clear        Clear the entire build cache");
    detail!("completions  Generate shell completion script");
    detail!("help         Print this help");
    gap!();

    section!("Arguments");
    detail!("[SCRIPT]   Path to a script file (shorthand: `rfast <script.rs>`)");
    detail!("[ARGS]...  Additional arguments passed to the script or eval code");
    gap!();

    section!("Options");
    detail!("-e, --eval <CODE>      Execute inline Rust code");
    detail!("-d, --dep <SPEC>       Add dependencies (crate, crate=version, or crate=version,feat1,feat2)");
    detail!("-i, --import <IMPORT>  Import an item (only for `-e`)");
    detail!("-v, --verbose          Show compilation logs and cache hits");
    detail!("-f, --force            Force recompilation, ignoring cache");
    detail!("    --test             Run `cargo test` on the script");
    detail!("-h, --help             Print this help");
    detail!("-V, --version          Print version");
    gap!();

    hint!("For more information, visit https://github.com/NameOfShadow/rfast");
    gap!();
}

/// Command‑line interface for `rfast`.
#[derive(Parser, Debug)]
#[command(name = "rfast", version, about, long_about = None, color = ColorChoice::Auto)]
struct Cli {
    /// Optional subcommand (run, build, new, add, cache, clear, completions)
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to a script file (shorthand: `rfast <script.rs>`)
    script: Option<PathBuf>,

    /// Execute inline Rust code (e.g. `rfast -e 'println!("Hello")'`)
    #[arg(short, long, value_name = "CODE")]
    eval: Option<String>,

    /// Add dependencies. Can be used multiple times.
    /// Format: `crate`, `crate=version`, or `crate=version,feat1,feat2`
    #[arg(short = 'd', long = "dep", value_name = "SPEC", action = ArgAction::Append)]
    deps: Vec<String>,

    /// Import items into the eval code (e.g. `-i colored::Colorize`). Can be used multiple times.
    #[arg(short = 'i', long = "import", value_name = "IMPORT", action = ArgAction::Append)]
    imports: Vec<String>,

    /// Verbose mode – show compilation logs and cache hits.
    #[arg(short, long)]
    verbose: bool,

    /// Force recompilation, ignoring cache (global, overrides subcommand `--force`).
    #[arg(short, long, global = true)]
    force: bool,

    /// Run tests inside the script (cargo test) instead of executing.
    #[arg(long, global = true)]
    test: bool,

    /// Additional arguments passed to the script or eval code.
    #[arg(trailing_var_arg = true)]
    args: Vec<String>,
}

/// Available subcommands for `rfast`.
#[derive(Subcommand, Debug)]
enum Commands {
    /// Run a script (compile if needed, then execute).
    Run {
        /// Path to the `.rs` script.
        script: PathBuf,
        /// Arguments to pass to the script.
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
        /// Force recompilation even if the cache is fresh (local override).
        #[arg(long, short)]
        force: bool,
        /// Use the Cranelift backend (requires nightly).
        #[arg(long)]
        cranelift: bool,
    },

    /// Build a script into a standalone binary.
    Build {
        /// Path to the `.rs` script.
        script: PathBuf,
        /// Output path for the binary (default: `./script`).
        #[arg(long, short, default_value = "./script")]
        output: PathBuf,
        /// Optimised release build (slower compile, faster binary).
        #[arg(long)]
        release: bool,
    },

    /// Create a new script from a template.
    New {
        /// Output filename (e.g. `hello.rs`).
        file: PathBuf,
    },

    /// Add a dependency to a script.
    Add {
        /// Crate name (e.g. `serde`).
        crate_name: String,
        /// Path to the `.rs` script.
        script: PathBuf,
        /// Crate version (default: `"*"`).
        #[arg(long, short, default_value = "*")]
        version: String,
    },

    /// Show build‑cache information.
    Cache,

    /// Clear the entire build cache.
    #[command(alias = "clean")]
    Clear,

    /// Generate shell completion script for the given shell.
    Completions {
        /// Shell to generate completions for (e.g. `bash`, `zsh`, `fish`, `powershell`).
        shell: Shell,
    },

    /// Print this beautiful help message.
    Help,
}

/// Entry point of the `rfast` binary.
///
/// Parses arguments, expands dependencies with features, and dispatches to
/// the appropriate handler: inline `-e` code, `--test`, subcommands, or script shorthand.
fn main() -> Result<()> {
    // Intercept --help and -h to show our custom help
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_beautiful_help();
        return Ok(());
    }

    let cli = Cli::parse();

    // Expand dependencies into DepSpec structs
    let mut dep_specs = Vec::new();
    for spec in cli.deps {
        if let Some((name, rest)) = spec.split_once('=') {
            let name = name.trim().to_string();
            if let Some((ver, feat_str)) = rest.split_once(',') {
                let version = ver.trim().to_string();
                let features = feat_str.split(',').map(|f| f.trim().to_string()).collect();
                dep_specs.push(DepSpec { name, version, features });
            } else {
                let version = rest.trim().to_string();
                dep_specs.push(DepSpec { name, version, features: vec![] });
            }
        } else {
            let name = spec.trim().to_string();
            dep_specs.push(DepSpec { name, version: "*".to_string(), features: vec![] });
        }
    }

    // Handle --test mode
    if cli.test {
        let script = cli.script.context("Usage: rfast --test <script.rs> [args...]")?;
        return runner::test(&script, &cli.args, cli.verbose);
    }

    // Special case: -d <crate> <script.rs> without -e or subcommand
    if !dep_specs.is_empty() && cli.eval.is_none() && cli.command.is_none() {
        let script = cli.script.context("Usage: rfast -d <crate> <script.rs>")?;
        let first = &dep_specs[0];
        return runner::add_dep(&script, &first.name, &first.version);
    }

    // Eval mode
    if let Some(code) = cli.eval {
        return runner::eval(&code, &dep_specs, &cli.imports, &cli.args, cli.verbose, cli.force);
    }

    match cli.command {
        Some(Commands::Run { script, args, force: sub_force, cranelift }) => {
            let force = cli.force || sub_force;
            runner::run(&script, &args, force, cranelift, cli.verbose)
        }
        Some(Commands::Build { script, output, release }) => {
            runner::build(&script, &output, release, cli.verbose)
        }
        Some(Commands::New { file }) => runner::new_script(&file),
        Some(Commands::Add { crate_name, script, version }) => {
            runner::add_dep(&script, &crate_name, &version)
        }
        Some(Commands::Cache) => cache::info(),
        Some(Commands::Clear) => cache::clear(),
        Some(Commands::Completions { shell }) => {
            let mut cmd = <Cli as clap::CommandFactory>::command();
            generate(shell, &mut cmd, "rfast", &mut io::stdout());
            Ok(())
        }
        Some(Commands::Help) => {
            print_beautiful_help();
            Ok(())
        }
        None => {
            let script = cli.script.context(
                "Usage: rfast <script.rs> [args...]\n\
                 \n\
                 Commands:\n\
                 \x20 rfast new <file.rs>          create a new script\n\
                 \x20 rfast add <crate> <file.rs>  add a dependency\n\
                 \x20 rfast build <file.rs>        build a standalone binary\n\
                 \x20 rfast cache                  show cache info\n\
                 \x20 rfast clear                  clear the cache",
            )?;
            runner::run(&script, &cli.args, cli.force, false, cli.verbose)
        }
    }
}