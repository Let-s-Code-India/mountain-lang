//! Parser for `mountain.toml`, the package manifest format defined in
//! Document 15 §3.1.
//!
//! Design decision (Phase 1): hand-rolled parser supporting the specific
//! subset of TOML that Document 15's examples actually use — `[section]`
//! headers, `key = "string"`, `key = true/false`, and `key = [ "a", "b" ]`
//! arrays-of-strings — rather than depending on the `toml`/`serde` crates.
//! This is purely a Phase 1 tooling-constraint decision (see PROGRESS.md,
//! no crates.io access in this environment), not a language design choice;
//! swapping in `serde`/`toml` later is a drop-in replacement once we have
//! real registry access, and nothing in the public `Manifest` API below
//! should need to change when that happens.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum TomlValue {
    Str(String),
    Bool(bool),
    Array(Vec<TomlValue>),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Manifest {
    /// section name -> (key -> value), preserves the two-level structure
    /// of mountain.toml (Document 15 §3.1: [package], [dependencies],
    /// [targets]) without hard-coding those specific section names, so
    /// this stays forward-compatible with new sections later documents
    /// might introduce.
    pub sections: BTreeMap<String, BTreeMap<String, TomlValue>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManifestError {
    pub message: String,
    pub line: usize,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mountain.toml:{}: {}", self.line, self.message)
    }
}

impl Manifest {
    pub fn get(&self, section: &str, key: &str) -> Option<&TomlValue> {
        self.sections.get(section)?.get(key)
    }

    pub fn get_str(&self, section: &str, key: &str) -> Option<&str> {
        match self.get(section, key) {
            Some(TomlValue::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn get_bool(&self, section: &str, key: &str) -> Option<bool> {
        match self.get(section, key) {
            Some(TomlValue::Bool(b)) => Some(*b),
            _ => None,
        }
    }

    /// Parses a `mountain.toml` source string into a `Manifest`.
    /// Returns every error found (does not stop at the first one),
    /// consistent with the error-recovery philosophy applied elsewhere
    /// in this compiler (Document 17 §2).
    pub fn parse(src: &str) -> std::result::Result<Manifest, Vec<ManifestError>> {
        let mut sections: BTreeMap<String, BTreeMap<String, TomlValue>> = BTreeMap::new();
        let mut current_section = String::new();
        let mut errors = Vec::new();

        for (idx, raw_line) in src.lines().enumerate() {
            let line_no = idx + 1;
            let line = strip_comment(raw_line).trim();
            if line.is_empty() {
                continue;
            }

            if line.starts_with('[') {
                if let Some(stripped) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                    current_section = stripped.trim().to_string();
                    sections.entry(current_section.clone()).or_default();
                } else {
                    errors.push(ManifestError {
                        message: format!("malformed section header: {}", line),
                        line: line_no,
                    });
                }
                continue;
            }

            if current_section.is_empty() {
                errors.push(ManifestError {
                    message: "key-value pair found before any [section] header".to_string(),
                    line: line_no,
                });
                continue;
            }

            match line.split_once('=') {
                Some((key, value_str)) => {
                    let key = key.trim().to_string();
                    let value_str = value_str.trim();
                    match parse_value(value_str) {
                        Ok(value) => {
                            sections.entry(current_section.clone()).or_default().insert(key, value);
                        }
                        Err(msg) => errors.push(ManifestError { message: msg, line: line_no }),
                    }
                }
                None => errors.push(ManifestError {
                    message: format!("expected `key = value`, found: {}", line),
                    line: line_no,
                }),
            }
        }

        if errors.is_empty() {
            Ok(Manifest { sections })
        } else {
            Err(errors)
        }
    }
}

fn strip_comment(line: &str) -> &str {
    // TOML comments start with `#` outside of strings. Since our value
    // grammar subset only allows strings/bools/string-arrays, and `#`
    // cannot legally appear outside a quoted string in any value we
    // support, a naive first-`#`-outside-quotes scan is sufficient here.
    let mut in_string = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_string = !in_string,
            '#' if !in_string => return &line[..i],
            _ => {}
        }
    }
    line
}

fn parse_value(s: &str) -> std::result::Result<TomlValue, String> {
    let s = s.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        return Ok(TomlValue::Str(s[1..s.len() - 1].to_string()));
    }
    if s == "true" {
        return Ok(TomlValue::Bool(true));
    }
    if s == "false" {
        return Ok(TomlValue::Bool(false));
    }
    if s.starts_with('[') && s.ends_with(']') {
        let inner = &s[1..s.len() - 1];
        if inner.trim().is_empty() {
            return Ok(TomlValue::Array(Vec::new()));
        }
        let mut items = Vec::new();
        for part in split_top_level_commas(inner) {
            items.push(parse_value(part.trim())?);
        }
        return Ok(TomlValue::Array(items));
    }
    Err(format!("unsupported value syntax: `{}` (Phase 1 supports strings, bools, and string arrays only)", s))
}

fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_string = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_string = !in_string,
            ',' if !in_string => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[package]
name = "my-trading-engine"
version = "0.1.0"
authors = ["Your Name"]
edition = "2026"

[dependencies]
http-toolkit = "1.2.0"
json-parser = "0.9.4"

[targets]
native = true
wasm = true
"#;

    #[test]
    fn parses_document15_sample_manifest() {
        let m = Manifest::parse(SAMPLE).expect("should parse cleanly");
        assert_eq!(m.get_str("package", "name"), Some("my-trading-engine"));
        assert_eq!(m.get_str("package", "version"), Some("0.1.0"));
        assert_eq!(m.get_str("package", "edition"), Some("2026"));
        assert_eq!(
            m.get("package", "authors"),
            Some(&TomlValue::Array(vec![TomlValue::Str("Your Name".to_string())]))
        );
        assert_eq!(m.get_str("dependencies", "http-toolkit"), Some("1.2.0"));
        assert_eq!(m.get_str("dependencies", "json-parser"), Some("0.9.4"));
        assert_eq!(m.get_bool("targets", "native"), Some(true));
        assert_eq!(m.get_bool("targets", "wasm"), Some(true));
    }

    #[test]
    fn missing_section_header_is_reported_not_panicking() {
        let src = "name = \"oops\"\n";
        let err = Manifest::parse(src).unwrap_err();
        assert_eq!(err.len(), 1);
        assert!(err[0].message.contains("before any [section] header"));
    }

    #[test]
    fn malformed_line_reported_and_parsing_continues() {
        let src = "[package]\nname = \"ok\"\nthis is not kv\nversion = \"1.0\"\n";
        let err = Manifest::parse(src).unwrap_err();
        // exactly the one bad line reported, not an abort
        assert_eq!(err.len(), 1);
        assert!(err[0].line == 3);
    }

    #[test]
    fn comments_are_stripped() {
        let src = "[package]\nname = \"x\" # a comment\n";
        let m = Manifest::parse(src).unwrap();
        assert_eq!(m.get_str("package", "name"), Some("x"));
    }

    #[test]
    fn empty_array() {
        let src = "[package]\nauthors = []\n";
        let m = Manifest::parse(src).unwrap();
        assert_eq!(m.get("package", "authors"), Some(&TomlValue::Array(vec![])));
    }
}
