//! Parsing of embedded metadata (dependencies, features, edition) from script files.
//!
//! `rfast` supports multiple metadata formats:
//! * Rfast block comment (`/* ... */`) – preferred, must be right after shebang.
//! * Rfast line comments (`//!`) – consecutive lines starting with `//!`.
//! * Cargo‑script code block (`//! ```cargo ... ````) inside a doc comment.
//! * Cargo‑script short comment (`// cargo-deps: name="version", ...`) at the top.
//! * Cargo‑play dependencies (`//# crate = "version"`) – simple dependency declarations.
//!
//! Inside any of these you can write TOML with sections `[dependencies]`, `[features]` and `edition`.
//! The dependencies section may contain simple inline tables or multi‑line subtables
//! (e.g. `[dependencies.reqwest]`). The raw text of the dependencies is preserved
//! exactly as written, so any valid `Cargo.toml` syntax is supported.

use anyhow::{bail, Result};

/// Metadata extracted from a script's embedded TOML block.
#[derive(Debug, Default, Clone)]
pub struct ScriptMeta {
    /// Raw text of the `[dependencies]` section (including header and any subtables).
    pub dependencies: String,
    /// The `[features]` section, serialised back to TOML.
    pub features: String,
    /// Rust edition (e.g. `"2024"`). Defaults to `"2024"`.
    pub edition: String,
}

/// Parse metadata from a script source file.
///
/// Tries all supported comment styles in order of priority:
/// 1. cargo-script code block (`//! ```cargo ... ````)
/// 2. rfast block comment (`/* ... */`)
/// 3. cargo-script short comment (`// cargo-deps: ...`)
/// 4. cargo-play dependencies (`//# ...`)
/// 5. rfast line comments (`//!`)
///
/// If no metadata is found, returns a default instance with edition `"2024"`.
pub fn parse_meta(source: &str) -> Result<ScriptMeta> {
    if let Some(m) = parse_cargo_script_block_comment(source)? {
        return Ok(m);
    }
    if let Some(m) = parse_block_comment(source)? {
        return Ok(m);
    }
    if let Some(m) = parse_cargo_script_short_comment(source)? {
        return Ok(m);
    }
    if let Some(m) = parse_cargo_play_deps(source)? {
        return Ok(m);
    }
    if let Some(m) = parse_line_comments(source)? {
        return Ok(m);
    }
    Ok(ScriptMeta {
        edition: "2024".into(),
        ..Default::default()
    })
}

/// Parse cargo-script's code block manifest (//! ```cargo ... ```)
fn parse_cargo_script_block_comment(source: &str) -> Result<Option<ScriptMeta>> {
    let src = strip_shebang(source);
    let start_marker = "//! ```cargo";
    if let Some(start) = src.find(start_marker) {
        // Find the closing "```" after the marker
        let after_start = start + start_marker.len();
        if let Some(end) = src[after_start..].find("```") {
            let block = &src[after_start..after_start + end];
            // Split into lines, remove leading "//!" and trim
            let fragment = block
                .lines()
                .map(|line| {
                    let trimmed = line.trim_start();
                    if trimmed.starts_with("//!") {
                        trimmed[3..].trim_start()
                    } else {
                        trimmed
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            return parse_toml_fragment(&fragment);
        }
    }
    Ok(None)
}

/// Parse cargo-script's short comment manifest (// cargo-deps: ...)
fn parse_cargo_script_short_comment(source: &str) -> Result<Option<ScriptMeta>> {
    let src = strip_shebang(source);
    let lines: Vec<&str> = src.lines().collect();
    if lines.is_empty() {
        return Ok(None);
    }
    let deps_line = lines.iter().find(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("// cargo-deps:") || trimmed.starts_with("//cargo-deps:")
    });
    if let Some(line) = deps_line {
        let deps_part = line.split(':').nth(1).unwrap_or("").trim();
        let mut toml_fragment = String::from("[dependencies]\n");
        for pair in deps_part.split(',') {
            let trimmed_pair = pair.trim();
            if trimmed_pair.is_empty() {
                continue;
            }
            if let Some((name, version)) = trimmed_pair.split_once('=') {
                let name_trim = name.trim();
                let version_trim = version.trim();
                let version_clean = version_trim
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .unwrap_or(version_trim);
                toml_fragment.push_str(&format!("{} = \"{}\"\n", name_trim, version_clean));
            } else {
                toml_fragment.push_str(&format!("{} = \"*\"\n", trimmed_pair));
            }
        }
        return parse_toml_fragment(&toml_fragment);
    }
    Ok(None)
}

/// Parse dependencies in cargo-play style: lines starting with `//#`
/// Example:
///   //# serde_json = "*"
///   //# anyhow = "1.0"
///   //# regex
/// (the last one becomes `regex = "*"`)
fn parse_cargo_play_deps(source: &str) -> Result<Option<ScriptMeta>> {
    let src = strip_shebang(source);
    let mut deps = String::new();
    let mut found = false;
    for line in src.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("//#") {
            found = true;
            let spec = rest.trim();
            if spec.contains('=') {
                deps.push_str(&format!("{}\n", spec));
            } else {
                deps.push_str(&format!("{} = \"*\"\n", spec));
            }
        } else if found && !trimmed.is_empty() && !trimmed.starts_with("//") {
            // Non-empty line that is not a comment – end of dependency block
            break;
        }
    }
    if found {
        let toml_fragment = format!("[dependencies]\n{}", deps);
        parse_toml_fragment(&toml_fragment)
    } else {
        Ok(None)
    }
}

/// Parse metadata from a `/* ... */` block.
///
/// The block must start immediately after the shebang (no other tokens before the `/*`).
/// Also handles documentation blocks `/*!` and `/**` by skipping the extra character.
fn parse_block_comment(source: &str) -> Result<Option<ScriptMeta>> {
    let src = strip_shebang(source);
    let start = match src.find("/*") {
        Some(p) => p,
        None => return Ok(None),
    };
    if !src[..start].trim().is_empty() {
        return Ok(None);
    }
    let end = src[start..]
        .find("*/")
        .ok_or_else(|| anyhow::anyhow!("unclosed `/*` metadata block"))?;
    let mut offset = 2; // skip "/*"
    let after_start = start + offset;
    if after_start < src.len() {
        let third = src.chars().nth(after_start).unwrap_or(' ');
        // If the third character is '!' or '*', skip it as well (documentation block)
        if third == '!' || third == '*' {
            offset = 3;
        }
    }
    let inner = &src[start + offset..start + end];
    parse_toml_fragment(inner)
}

/// Parse metadata from `//!` line comments.
///
/// All consecutive lines starting with `//!` (after trimming) are joined into one TOML fragment.
fn parse_line_comments(source: &str) -> Result<Option<ScriptMeta>> {
    let src = strip_shebang(source);
    let mut lines: Vec<&str> = Vec::new();
    let mut started = false;
    for line in src.lines() {
        if let Some(rest) = line.trim().strip_prefix("//!") {
            lines.push(rest.trim_start_matches(' '));
            started = true;
        } else if started {
            break;
        }
    }
    if !started {
        return Ok(None);
    }
    parse_toml_fragment(&lines.join("\n"))
}

/// Replace `//` comments with `#` for TOML parsing (only outside string literals).
fn replace_comments(s: &str) -> String {
    let mut out = String::new();
    let mut in_string = false;
    let mut i = 0;
    let chars: Vec<char> = s.chars().collect();
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            in_string = !in_string;
            out.push(c);
            i += 1;
            continue;
        }
        if !in_string && c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            out.push('#');
            i += 2;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Parse a TOML fragment and extract edition, features and the raw dependencies text.
fn parse_toml_fragment(fragment: &str) -> Result<Option<ScriptMeta>> {
    let fragment = fragment.trim();
    if fragment.is_empty() {
        return Ok(Some(ScriptMeta {
            edition: "2024".into(),
            ..Default::default()
        }));
    }
    let cleaned = replace_comments(fragment);
    let value: toml::Value = toml::from_str(&cleaned).map_err(|e| {
        anyhow::anyhow!("invalid TOML in script metadata:\n{e}\n\nfragment:\n{fragment}")
    })?;
    let mut meta = ScriptMeta {
        edition: "2024".into(),
        ..Default::default()
    };
    if let Some(e) = value.get("edition").and_then(|e| e.as_str()) {
        match e {
            "2015" | "2018" | "2021" | "2024" => meta.edition = e.into(),
            other => bail!("unsupported edition `{other}` — valid: 2015, 2018, 2021, 2024"),
        }
    }
    if let Some(f) = value.get("features") {
        meta.features = toml::to_string(f)?;
    }
    // Extract the raw dependencies text (preserves original formatting).
    meta.dependencies = extract_dependencies_text(fragment);
    Ok(Some(meta))
}

/// Extract the raw text of the `[dependencies]` section (including any subtables).
///
/// This function scans the TOML fragment line by line and collects every line
/// that belongs to the `[dependencies]` tree, starting from the first occurrence
/// of `[dependencies]` or `[dependencies.*]` until the next top‑level section
/// (a line starting with `[` that is not a subtable of dependencies).
///
/// The original indentation and line breaks are preserved.
fn extract_dependencies_text(fragment: &str) -> String {
    let lines: Vec<&str> = fragment.lines().collect();
    let mut in_deps = false;
    let mut deps_lines = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("[dependencies") && trimmed.ends_with(']') {
            in_deps = true;
            deps_lines.push(line);
            continue;
        }
        if in_deps {
            // Stop when a new top‑level section (not a subtable of deps) is encountered.
            if trimmed.starts_with('[') && !trimmed.starts_with("[dependencies") {
                break;
            }
            deps_lines.push(line);
        }
    }
    if !deps_lines.is_empty() {
        deps_lines.join("\n")
    } else {
        String::new()
    }
}

/// Remove a shebang (`#!`) line from the beginning of a string, if present.
fn strip_shebang(s: &str) -> &str {
    if s.starts_with("#!") {
        s.find('\n').map(|p| &s[p + 1..]).unwrap_or("")
    } else {
        s
    }
}