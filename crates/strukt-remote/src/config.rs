use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::SshAlias;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigDiscoveryLimits {
    pub max_files: usize,
    pub max_depth: usize,
    pub max_file_bytes: usize,
    pub max_warnings: usize,
}

impl Default for ConfigDiscoveryLimits {
    fn default() -> Self {
        Self {
            max_files: 64,
            max_depth: 8,
            max_file_bytes: 256 * 1_024,
            max_warnings: 32,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConfigDiscovery {
    pub aliases: Vec<SshAlias>,
    pub warnings: Vec<String>,
    pub truncated: bool,
}

/// Discovers concrete `Host` aliases from bounded OpenSSH configuration files.
///
/// Discovery is intentionally best effort: failures become bounded warnings and
/// never prevent callers from using an explicit validated alias.
#[must_use]
pub fn discover_aliases(roots: &[PathBuf], limits: &ConfigDiscoveryLimits) -> ConfigDiscovery {
    let mut scanner = Scanner::new(*limits);
    for root in roots {
        scanner.visit(root, 0);
    }
    scanner.finish()
}

struct Scanner {
    limits: ConfigDiscoveryLimits,
    aliases: BTreeSet<String>,
    warnings: Vec<String>,
    visited: HashSet<PathBuf>,
    files: usize,
    truncated: bool,
}

impl Scanner {
    fn new(limits: ConfigDiscoveryLimits) -> Self {
        Self {
            limits,
            aliases: BTreeSet::new(),
            warnings: Vec::new(),
            visited: HashSet::new(),
            files: 0,
            truncated: false,
        }
    }

    fn visit(&mut self, path: &Path, depth: usize) {
        if depth > self.limits.max_depth || self.files >= self.limits.max_files {
            self.truncated = true;
            self.warn(format!("SSH config limit reached at {}", path.display()));
            return;
        }

        let identity = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if !self.visited.insert(identity) {
            self.warn(format!("SSH config include cycle at {}", path.display()));
            return;
        }
        self.files += 1;

        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.warn(format!(
                    "cannot read SSH config {}: {error}",
                    path.display()
                ));
                return;
            }
        };
        let bytes = if bytes.len() > self.limits.max_file_bytes {
            self.truncated = true;
            self.warn(format!("SSH config {} was truncated", path.display()));
            &bytes[..self.limits.max_file_bytes]
        } else {
            bytes.as_slice()
        };
        let text = String::from_utf8_lossy(bytes);
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        for (line_index, line) in text.lines().enumerate() {
            let Ok(words) = split_words(line) else {
                self.warn(format!(
                    "malformed SSH config line {}:{}",
                    path.display(),
                    line_index + 1
                ));
                continue;
            };
            let Some((directive, values)) = words.split_first() else {
                continue;
            };
            if directive.eq_ignore_ascii_case("host") {
                self.record_hosts(values);
            } else if directive.eq_ignore_ascii_case("include") {
                for include in values {
                    for included_path in expand_include(parent, include) {
                        self.visit(&included_path, depth + 1);
                    }
                }
            }
        }
    }

    fn record_hosts(&mut self, values: &[String]) {
        for value in values {
            if value.starts_with('!') || contains_pattern(value) {
                continue;
            }
            if let Ok(alias) = SshAlias::new(value.clone()) {
                self.aliases.insert(alias.to_string());
            }
        }
    }

    fn warn(&mut self, warning: String) {
        if self.warnings.len() < self.limits.max_warnings {
            self.warnings.push(warning);
        } else {
            self.truncated = true;
        }
    }

    fn finish(self) -> ConfigDiscovery {
        ConfigDiscovery {
            aliases: self
                .aliases
                .into_iter()
                .filter_map(|value| SshAlias::new(value).ok())
                .collect(),
            warnings: self.warnings,
            truncated: self.truncated,
        }
    }
}

fn contains_pattern(value: &str) -> bool {
    value
        .bytes()
        .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']'))
}

fn expand_include(parent: &Path, value: &str) -> Vec<PathBuf> {
    let path = Path::new(value);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        parent.join(path)
    };
    let Some(file_pattern) = path.file_name().and_then(|name| name.to_str()) else {
        return vec![path];
    };
    if !file_pattern.contains(['*', '?']) {
        return vec![path];
    }
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let mut matches = fs::read_dir(directory)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            wildcard_match(file_pattern.as_bytes(), name.as_bytes()).then(|| entry.path())
        })
        .collect::<Vec<_>>();
    matches.sort();
    matches
}

fn wildcard_match(pattern: &[u8], value: &[u8]) -> bool {
    let (mut pattern_index, mut value_index) = (0, 0);
    let (mut star, mut star_value) = (None, 0);
    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star = Some(pattern_index);
            pattern_index += 1;
            star_value = value_index;
        } else if let Some(star_index) = star {
            pattern_index = star_index + 1;
            star_value += 1;
            value_index = star_value;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

fn split_words(line: &str) -> Result<Vec<String>, ()> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in line.chars() {
        if escaped {
            word.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            } else {
                word.push(character);
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '#' => break,
            character if character.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            _ => word.push(character),
        }
    }
    if quote.is_some() || escaped {
        return Err(());
    }
    if !word.is_empty() {
        words.push(word);
    }
    Ok(words)
}
