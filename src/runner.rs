//! Core execution logic: compiling, caching, running scripts, and inline evaluation.

use crate::cache;
use crate::parser::{self, ScriptMeta};
use crate::{detail, fail, gap, hint, section, success};
use anyhow::{bail, Context, Result};
use colored::Colorize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use tempfile::{Builder, TempDir};

/// Specification for a dependency: name, version, and optional features.
#[derive(Debug, Clone)]
pub struct DepSpec {
    pub name: String,
    pub version: String,
    pub features: Vec<String>,
}

// ─── Public entry points ──────────────────────────────────────────────────────

/// Compile (if needed) and run a script.
pub fn run(script: &Path, args: &[String], force: bool, cranelift: bool, verbose: bool) -> Result<()> {
    let script = canonicalize(script)?;
    let hash = cache::hash_file(&script)?;

    if force || !cache::is_cache_valid(&hash) {
        compile(&script, &hash, cranelift, false, verbose)?;
    } else if verbose {
        detail!("cache hit · {}", &hash[..12]);
    }

    exec_binary(&cache::binary_path(&hash), args)
}

/// Run `cargo test` for a script.
pub fn test(script: &Path, args: &[String], verbose: bool) -> Result<()> {
    let script = canonicalize(script)?;
    let hash = cache::hash_file(&script)?;
    if !cache::is_cache_valid(&hash) {
        compile(&script, &hash, false, false, verbose)?;
    }
    let project_dir = cache::project_dir(&hash);
    let status = Command::new("cargo")
        .current_dir(project_dir)
        .arg("test")
        .args(args)
        .status()
        .context("Failed to run cargo test")?;
    std::process::exit(status.code().unwrap_or(1));
}

/// Compile a script and copy the binary to a user‑specified location.
pub fn build(script: &Path, output: &Path, release: bool, verbose: bool) -> Result<()> {
    let script = canonicalize(script)?;
    let hash = cache::hash_file(&script)?;
    compile(&script, &hash, false, release, verbose)?;

    let src = {
        let base = if release {
            cache::project_dir(&hash).join("target/release/script")
        } else {
            cache::binary_path(&hash)
        };
        platform_binary(base)
    };
    let output = platform_binary(output.to_path_buf());
    fs::copy(&src, &output).with_context(|| {
        format!("could not copy binary {} → {}", src.display(), output.display())
    })?;
    set_executable(&output)?;

    if verbose {
        gap!();
        success!("binary ready  {}", crate::ui::hi(&output.display().to_string()));
        #[cfg(windows)]
        hint!("run it with   .\\{}", output.display());
        #[cfg(not(windows))]
        hint!("run it with   ./{}", output.display());
    }
    Ok(())
}

/// Create a new script file from a built‑in template.
pub fn new_script(path: &Path) -> Result<()> {
    let path = if path.extension().map_or(false, |ext| ext == "rs") {
        path.to_path_buf()
    } else {
        path.with_extension("rs")
    };
    if path.exists() {
        bail!("file already exists: {}", path.display());
    }
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("script");
    let template = format!(
        "#!/usr/bin/env rfast\n\
         /*\n\
         [dependencies]\n\
         # add crates here, e.g.:\n\
         # colored = \"2.0\"\n\
         */\n\
         \n\
         fn main() {{\n\
         \x20   println!(\"Hello from {name}!\");\n\
         }}\n"
    );
    fs::write(&path, &template)?;
    set_executable(&path)?;
    #[cfg(windows)]
    {
        let bat = path.with_extension("bat");
        let rs = path.file_name().unwrap_or_default().to_string_lossy();
        let bat_content = format!(
            "@echo off\n\
             rfast run \"%~dp0{rs}\" %*\n"
        );
        fs::write(&bat, bat_content)?;
        detail!("launcher  {}", bat.display());
    }
    gap!();
    section!("new script");
    detail!("created   {}", path.display());
    gap!();
    #[cfg(windows)]
    {
        hint!("run it with   rfast {}", path.display());
        hint!("  or via bat  {}.bat", name);
    }
    #[cfg(not(windows))]
    {
        hint!("run it with   rfast {}", path.display());
        hint!("  or directly ./{}", path.display());
    }
    hint!("add a dep     rfast add <crate> {}", path.display());
    Ok(())
}

/// Inject a dependency into an existing script's metadata block.
pub fn add_dep(script: &Path, krate: &str, version: &str) -> Result<()> {
    let source = fs::read_to_string(script)?;
    let new_dep = format!("{} = \"{}\"", krate, version);
    let (shebang, rest) = if source.starts_with("#!") {
        let newline = source.find('\n').unwrap_or(source.len());
        (Some(&source[..newline + 1]), source[newline + 1..].as_ref())
    } else {
        (None, source.as_str())
    };
    if let Some(start) = rest.find("/*") {
        if rest[..start].trim().is_empty() {
            if let Some(end_rel) = rest[start..].find("*/") {
                let block_start = start;
                let block_end = start + end_rel + 2;
                let before = &rest[..block_start];
                let inside = &rest[block_start + 2..block_end - 2];
                let after = &rest[block_end..];
                let deps_pos = inside.find("[dependencies]");
                let new_inside = if let Some(pos) = deps_pos {
                    let line_end = inside[pos..].find('\n').unwrap_or(inside[pos..].len());
                    let deps_line_end = pos + line_end;
                    let mut new_inside = String::new();
                    new_inside.push_str(&inside[..deps_line_end]);
                    new_inside.push('\n');
                    new_inside.push_str(&new_dep);
                    new_inside.push_str(&inside[deps_line_end..]);
                    new_inside
                } else {
                    format!("[dependencies]\n{}\n{}", new_dep, inside)
                };
                let new_rest = format!("{}/*{}*/{}", before, new_inside, after);
                let new_content = if let Some(sh) = shebang {
                    format!("{}{}", sh, new_rest)
                } else {
                    new_rest
                };
                fs::write(script, new_content)?;
                gap!();
                success!("added  {} = \"{}\"", krate, version);
                detail!("in  {}", script.display());
                hint!("run rfast {} to compile with the new dep", script.display());
                return Ok(());
            }
        }
    }
    let new_block = format!("/*\n[dependencies]\n{}\n*/\n\n", new_dep);
    let new_content = if let Some(sh) = shebang {
        format!("{}{}{}", sh, new_block, rest)
    } else {
        format!("{}{}", new_block, rest)
    };
    fs::write(script, new_content)?;
    gap!();
    success!("added  {} = \"{}\"", krate, version);
    detail!("in  {}", script.display());
    hint!("run rfast {} to compile with the new dep", script.display());
    Ok(())
}

// ─── Core compile pipeline ────────────────────────────────────────────────────

fn compile(script: &Path, hash: &str, cranelift: bool, release: bool, verbose: bool) -> Result<()> {
    let project_dir = cache::project_dir(hash);
    let src_dir = project_dir.join("src");
    if verbose {
        gap!();
        section!("compiling  {}", script.display());
        detail!("cache  {}", cache::short_cache_path(hash));
        gap!();
    }
    let source = fs::read_to_string(script)
        .with_context(|| format!("cannot read {}", script.display()))?;
    let meta = parser::parse_meta(&source)?;
    fs::create_dir_all(&src_dir)?;
    fs::write(project_dir.join("Cargo.toml"), generate_cargo_toml(&meta, cranelift))?;
    fs::write(src_dir.join("main.rs"), strip_shebang(&source))?;
    if cranelift {
        setup_cranelift(&project_dir)?;
    }
    let start = Instant::now();
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&project_dir)
        .arg("build")
        .args(if release { &["--release"][..] } else { &[] });
    if !verbose {
        cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    }
    let status = cmd.status().context("could not run `cargo` — is Rust installed?")?;
    if !status.success() {
        cache::invalidate(hash);
        if verbose {
            gap!();
            fail!("compilation failed");
        }
        bail!("cargo exited with a non-zero status");
    }
    cache::write_stamp(hash)?;
    if verbose {
        gap!();
        success!("compiled in {:.2}s", start.elapsed().as_secs_f64());
    }
    Ok(())
}

// ─── Platform helpers ─────────────────────────────────────────────────────────

fn platform_binary(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        if path.extension().is_none() {
            path.with_extension("exe")
        } else {
            path
        }
    }
    #[cfg(not(windows))]
    {
        path
    }
}

fn set_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
    }
    #[cfg(windows)]
    let _ = path;
    Ok(())
}

#[cfg(unix)]
fn exec_binary(binary: &Path, args: &[String]) -> Result<()> {
    use std::os::unix::process::CommandExt;
    gap!();
    let err = Command::new(binary).args(args).exec();
    Err(err).with_context(|| format!("could not exec {}", binary.display()))
}

#[cfg(windows)]
fn exec_binary(binary: &Path, args: &[String]) -> Result<()> {
    let binary = platform_binary(binary.to_path_buf());
    gap!();
    let status = Command::new(&binary)
        .args(args)
        .status()
        .with_context(|| format!("could not run {}", binary.display()))?;
    std::process::exit(status.code().unwrap_or(1));
}

// ─── Misc helpers ─────────────────────────────────────────────────────────────

fn generate_cargo_toml(meta: &ScriptMeta, cranelift: bool) -> String {
    let profile = if cranelift {
        "\n[profile.dev]\nopt-level = 0\ndebug = false\n"
    } else {
        ""
    };
    let features = if !meta.features.is_empty() {
        format!("\n[features]\n{}", meta.features)
    } else {
        String::new()
    };
    let deps_cleaned = meta
        .dependencies
        .lines()
        .map(|line| {
            if let Some(pos) = line.find("//") {
                &line[..pos]
            } else {
                line
            }
        })
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let deps_section = if deps_cleaned.is_empty() {
        String::new()
    } else {
        format!("\n{}", deps_cleaned)
    };
    format!(
        "[package]\nname = \"script\"\nversion = \"0.1.0\"\nedition = \"{}\"\n{}{}{}\n",
        meta.edition, deps_section, features, profile
    )
}

fn setup_cranelift(project_dir: &Path) -> Result<()> {
    let dir = project_dir.join(".cargo");
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join("config.toml"),
        "[unstable]\ncodegen-backend = true\n\n[profile.dev]\ncodegen-backend = \"cranelift\"\n",
    )?;
    Ok(())
}

fn canonicalize(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("file not found: {}", path.display()))
}

fn strip_shebang(s: &str) -> &str {
    if s.starts_with("#!") {
        s.find('\n').map(|p| &s[p + 1..]).unwrap_or("")
    } else {
        s
    }
}

// ─── Eval with caching ──────────────────────────────────────────────────────

fn eval_hash(code: &str, deps: &[DepSpec], imports: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code.as_bytes());
    for dep in deps {
        hasher.update(dep.name.as_bytes());
        hasher.update(dep.version.as_bytes());
        for feat in &dep.features {
            hasher.update(feat.as_bytes());
        }
    }
    for imp in imports {
        hasher.update(imp.as_bytes());
    }
    hex::encode(hasher.finalize().as_slice())
}

fn eval_cache_dir(hash: &str) -> PathBuf {
    cache::root_dir().join("eval").join(hash)
}

pub fn eval(
    code: &str,
    deps: &[DepSpec],
    imports: &[String],
    args: &[String],
    verbose: bool,
    force: bool,
) -> Result<()> {
    let hash = eval_hash(code, deps, imports);
    let cache_dir = eval_cache_dir(&hash);
    let binary_path = if cfg!(windows) {
        cache_dir.join("eval.exe")
    } else {
        cache_dir.join("eval")
    };
    if !force && binary_path.exists() {
        if verbose {
            detail!("cache hit · {}", &hash[..12]);
        }
        let status = Command::new(&binary_path).args(args).status()?;
        if status.success() {
            return Ok(());
        } else {
            bail!("Execution failed with exit code: {:?}", status.code());
        }
    }
    if verbose {
        gap!();
        section!("compiling eval");
        detail!("hash  {}", &hash[..12]);
        gap!();
    }
    let start = Instant::now();
    if deps.is_empty() {
        compile_eval_rustc(code, imports, &cache_dir, &binary_path, verbose)?;
    } else {
        compile_eval_cargo(code, deps, imports, &cache_dir, &binary_path, verbose)?;
    }
    if verbose {
        gap!();
        success!("compiled in {:.2}s", start.elapsed().as_secs_f64());
    }
    let status = Command::new(&binary_path).args(args).status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("Execution failed with exit code: {:?}", status.code())
    }
}

fn compile_eval_rustc(
    code: &str,
    imports: &[String],
    cache_dir: &Path,
    binary_path: &Path,
    verbose: bool,
) -> Result<()> {
    let mut source = String::new();
    if !imports.is_empty() {
        source.push_str("#[allow(unused_imports)]\n");
        for imp in imports {
            source.push_str(&format!("use {};\n", imp));
        }
        source.push('\n');
    }
    if code.trim_start().starts_with("fn main") {
        source.push_str(code);
    } else {
        source.push_str(&format!("fn main() {{\n    {}\n}}", code));
    }
    let mut temp_file = Builder::new()
        .prefix("rfast_eval_")
        .suffix(".rs")
        .tempfile()?;
    temp_file.write_all(source.as_bytes())?;
    let temp_path = temp_file.path();
    fs::create_dir_all(cache_dir)?;
    let mut cmd = Command::new("rustc");
    cmd.arg(temp_path).arg("-o").arg(binary_path);
    if !verbose {
        cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    }
    let output = cmd.output()?;
    if !output.status.success() {
        if verbose {
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        }
        bail!("Compilation failed");
    }
    Ok(())
}

fn compile_eval_cargo(
    code: &str,
    deps: &[DepSpec],
    imports: &[String],
    cache_dir: &Path,
    binary_path: &Path,
    verbose: bool,
) -> Result<()> {
    let mut source = String::new();
    if !imports.is_empty() {
        source.push_str("#[allow(unused_imports)]\n");
        for imp in imports {
            source.push_str(&format!("use {};\n", imp));
        }
        source.push('\n');
    }
    if code.trim_start().starts_with("fn main") {
        source.push_str(code);
    } else {
        source.push_str(&format!("fn main() {{\n    {}\n}}", code));
    }
    let temp_dir = TempDir::new()?;
    let project_path = temp_dir.path();
    let src_dir = project_path.join("src");
    fs::create_dir_all(&src_dir)?;
    fs::write(src_dir.join("main.rs"), source)?;

    let mut dependencies = String::new();
    for dep in deps {
        if dep.features.is_empty() {
            dependencies.push_str(&format!("{} = \"{}\"\n", dep.name, dep.version));
        } else {
            let features_str = dep.features.iter().map(|f| format!("\"{}\"", f)).collect::<Vec<_>>().join(", ");
            dependencies.push_str(&format!("{} = {{ version = \"{}\", features = [{}] }}\n", dep.name, dep.version, features_str));
        }
    }
    let cargo_toml = format!(
        r#"[package]
name = "rfast-eval"
version = "0.1.0"
edition = "2024"

[dependencies]
{}
"#,
        dependencies
    );
    fs::write(project_path.join("Cargo.toml"), cargo_toml)?;

    let mut cmd = Command::new("cargo");
    cmd.current_dir(project_path)
        .arg("build")
        .arg("--release");
    if !verbose {
        cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    }
    let status = cmd.status().context("Failed to run cargo")?;
    if !status.success() {
        bail!("Cargo build failed");
    }
    let bin_name = if cfg!(windows) { "rfast-eval.exe" } else { "rfast-eval" };
    let built_binary = project_path.join("target/release").join(bin_name);
    fs::create_dir_all(cache_dir)?;
    fs::copy(&built_binary, binary_path)?;
    Ok(())
}