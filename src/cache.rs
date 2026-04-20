//! Build cache management for `rfast`.
//!
//! This module handles the persistent cache of compiled script binaries.
//! Each script is hashed (SHA‑256 of its content + file path) and stored
//! under `~/.cache/rfast/<hash>/`. The cache includes a stamp file to
//! verify validity.

use crate::{detail, gap, hint, section, success};
use anyhow::Result;
use colored::Colorize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

// ─── Paths ────────────────────────────────────────────────────────────────────

/// Returns the root cache directory for `rfast`.
///
/// Uses `dirs::cache_dir()` (e.g. `~/.cache/` on Linux) and appends `rfast`.
/// Falls back to `/tmp` if the system cache directory cannot be determined.
pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("rfast")
}

/// Returns the project subdirectory for a given script hash.
pub fn project_dir(hash: &str) -> PathBuf {
    cache_dir().join(hash)
}

/// Returns the root directory (same as `cache_dir()`).
pub fn root_dir() -> PathBuf {
    dirs::cache_dir()
        .expect("Could not determine cache directory")
        .join("rfast")
}

/// Returns the path to the stamp file used to validate a cache entry.
fn stamp_path(hash: &str) -> PathBuf {
    project_dir(hash).join(".build_stamp")
}

// ─── Validity ─────────────────────────────────────────────────────────────────

/// Computes a SHA‑256 hash of a script file (content + full path).
///
/// # Example
/// ```
/// # use rfast::cache::hash_file;
/// # let path = std::path::Path::new("script.rs");
/// let hash = hash_file(path).unwrap();
/// ```
pub fn hash_file(path: &Path) -> Result<String> {
    let content = fs::read(path)?;
    let mut h = Sha256::new();
    h.update(&content);
    h.update(path.to_string_lossy().as_bytes());
    Ok(hex::encode(h.finalize()))
}

/// Checks whether a cached build for the given hash is still valid.
///
/// Validity requires:
/// - The binary (`binary_path(hash)`) exists.
/// - The stamp file exists.
/// - The stamp file contains exactly the same hash.
pub fn is_cache_valid(hash: &str) -> bool {
    let ok = binary_path(hash).exists()
        && stamp_path(hash).exists()
        && fs::read_to_string(stamp_path(hash))
            .map(|s| s.trim() == hash)
            .unwrap_or(false);
    ok
}

/// Writes a stamp file for the given hash, marking the cache entry as valid.
pub fn write_stamp(hash: &str) -> Result<()> {
    fs::write(stamp_path(hash), hash)?;
    Ok(())
}

/// Invalidates a cache entry by removing its stamp file (the binary is left untouched).
pub fn invalidate(hash: &str) {
    let _ = fs::remove_file(stamp_path(hash));
}

// ─── CLI subcommands ──────────────────────────────────────────────────────────

/// Displays information about the current cache.
///
/// Shows location, number of entries, total size, and the five most recently
/// used entries with a ✔/✘ indicator.
pub fn info() -> Result<()> {
    let dir = cache_dir();

    gap!();
    section!("cache");

    if !dir.exists() {
        detail!("empty — nothing compiled yet");
        gap!();
        return Ok(());
    }

    let entries: Vec<_> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    let size = dir_size(&dir);

    detail!("location  {}", dir.display());
    detail!("entries   {}", entries.len());
    detail!("size      {}", fmt_bytes(size));

    if !entries.is_empty() {
        gap!();
        let mut sorted = entries;
        sorted.sort_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        for e in sorted.iter().rev().take(5) {
            let hash = e.file_name().to_string_lossy().to_string();
            let ok = binary_path(&hash).exists();
            let mark = if ok { crate::ui::ok("✔") } else { crate::ui::err("✘") };
            let short_hash = if hash.len() >= 16 { &hash[..16] } else { &hash };
            detail!("{}  {}", mark, short_hash);
        }
    }

    gap!();
    hint!("clear with   rfast clear");

    Ok(())
}

/// Clears the entire build cache (deletes the whole `rfast` cache directory).
pub fn clear() -> Result<()> {
    let dir = cache_dir();

    if !dir.exists() {
        gap!();
        detail!("cache is already empty");
        gap!();
        return Ok(());
    }

    let size = dir_size(&dir);
    fs::remove_dir_all(&dir)?;

    gap!();
    success!("cache cleared  (freed {})", crate::ui::hi(&fmt_bytes(size)));
    gap!();

    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Recursively calculates the size of a file or directory in bytes.
fn dir_size(p: &Path) -> u64 {
    if p.is_file() {
        return p.metadata().map(|m| m.len()).unwrap_or(0);
    }
    fs::read_dir(p)
        .map(|it| it.filter_map(|e| e.ok()).map(|e| dir_size(&e.path())).sum())
        .unwrap_or(0)
}

/// Formats a byte count into a human‑readable string (B, KB, MB, GB).
fn fmt_bytes(b: u64) -> String {
    match b {
        0..=1023 => format!("{b} B"),
        1024..=1_048_575 => format!("{:.1} KB", b as f64 / 1024.0),
        1_048_576..=1_073_741_823 => format!("{:.1} MB", b as f64 / 1_048_576.0),
        _ => format!("{:.2} GB", b as f64 / 1_073_741_824.0),
    }
}

/// Returns a human‑readable short cache path for display.
///
/// On Unix: `~/.cache/rfast/<short hash>`  
/// On Windows: `%LOCALAPPDATA%\rfast\<short hash>`
pub fn short_cache_path(hash: &str) -> String {
    #[cfg(windows)]
    {
        format!("%LOCALAPPDATA%\\rfast\\{}", &hash[..12])
    }
    #[cfg(not(windows))]
    {
        format!("~/.cache/rfast/{}", &hash[..12])
    }
}

/// Returns the path to the compiled binary for a given script hash.
///
/// On Windows the binary is named `script.exe`; on Unix it is `script`.
pub fn binary_path(hash: &str) -> PathBuf {
    #[cfg(windows)]
    {
        project_dir(hash).join("target/debug/script.exe")
    }
    #[cfg(not(windows))]
    {
        project_dir(hash).join("target/debug/script")
    }
}