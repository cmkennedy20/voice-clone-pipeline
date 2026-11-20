//! Atomic `.env` reader, Windows-backslash-safe unquoting parser, secret generator, and upserter.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use common::digest::sha256_hex;
use uuid::Uuid;

use super::DesktopError;

fn unescape_double_quoted(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('\\') | None => out.push('\\'),
                Some('"') => out.push('"'),
                Some('n') => out.push('\n'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn strip_quotes(raw_val: &str) -> String {
    let trimmed = raw_val.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
        return trimmed[1..trimmed.len() - 1].replace("'\\''", "'");
    }
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return unescape_double_quoted(&trimmed[1..trimmed.len() - 1]);
    }
    trimmed.to_owned()
}

fn format_env_value(value: &str) -> String {
    let needs_quotes = value
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '#' | '\'' | '"' | ';' | '$'));
    if needs_quotes {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_owned()
    }
}

/// Parses `.env` text into a trimmed key-value map, preserving Windows backslashes inside quotes.
#[must_use]
pub fn parse_dotenv_text(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let without_export = trimmed.strip_prefix("export ").map_or(trimmed, str::trim);
        let Some((key, val)) = without_export.split_once('=') else {
            continue;
        };
        let clean_key = key.trim();
        if !clean_key.is_empty() {
            map.insert(clean_key.to_owned(), strip_quotes(val));
        }
    }
    map
}

/// Reads and parses an optional `.env` file, returning an empty map if absent.
pub fn read_dotenv_file(path: &Path) -> Result<BTreeMap<String, String>, DesktopError> {
    if !path.is_file() {
        return Ok(BTreeMap::new());
    }
    let content = std::fs::read_to_string(path)?;
    Ok(parse_dotenv_text(&content))
}

/// Atomically updates or appends `updates` inside `path`, preserving comments and untouched keys.
pub fn upsert_dotenv_file(
    path: &Path,
    updates: &BTreeMap<String, String>,
) -> Result<(), DesktopError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let existing = if path.is_file() {
        std::fs::read_to_string(path)?
    } else {
        String::new()
    };

    let mut written_keys = BTreeSet::new();
    let mut out_lines = Vec::new();

    for raw_line in existing.lines() {
        let trimmed = raw_line.trim();
        let candidate = trimmed.strip_prefix("export ").map_or(trimmed, str::trim);
        if !trimmed.starts_with('#')
            && let Some((k, _)) = candidate.split_once('=')
        {
            let clean_key = k.trim();
            if let Some(new_val) = updates.get(clean_key) {
                out_lines.push(format!("{clean_key}={}", format_env_value(new_val)));
                written_keys.insert(clean_key.to_owned());
                continue;
            }
        }
        out_lines.push(raw_line.to_owned());
    }

    for (key, val) in updates {
        if !written_keys.contains(key) {
            out_lines.push(format!("{key}={}", format_env_value(val)));
        }
    }

    let mut serialized = out_lines.join("\n");
    serialized.push('\n');

    let tmp_path = path.with_extension(format!("env.tmp.{}", Uuid::new_v4()));
    std::fs::write(&tmp_path, serialized.as_bytes())?;
    if path.is_file() && cfg!(windows) {
        let _ = std::fs::remove_file(path);
    }
    if std::fs::rename(&tmp_path, path).is_err() {
        std::fs::write(path, serialized.as_bytes())?;
        let _ = std::fs::remove_file(&tmp_path);
    }
    Ok(())
}

/// Generates a fresh 64-character lowercase hexadecimal secret token using `CSPRNG` `UUID v4` entropy.
#[must_use]
pub fn generate_secret_hex_token() -> String {
    let mut seed = Vec::with_capacity(32);
    seed.extend_from_slice(Uuid::new_v4().as_bytes());
    seed.extend_from_slice(Uuid::new_v4().as_bytes());
    sha256_hex(&seed)
}
