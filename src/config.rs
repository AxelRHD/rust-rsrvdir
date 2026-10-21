//! Configuration: either a TOML file with several shares or a single directory from the command line.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::Deserialize;

use crate::listing::{Order, SortKey};

pub const THEMES: &[(&str, &str)] = &[
    ("auto", "Auto"),
    ("nord", "Nord"),
    ("squirrel", "Squirrel"),
    ("archlinux", "Archlinux"),
    ("monokai", "Monokai"),
    ("zenburn", "Zenburn"),
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub listen: Option<String>,
    pub title: Option<String>,
    pub theme: Option<String>,
    pub auth_file: Option<PathBuf>,
    #[serde(default, rename = "share")]
    pub shares: Vec<ShareConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareConfig {
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub description: String,
    /// Glob patterns (relative to the share root) for files that may be served; empty = all files.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Glob patterns for files and directories that are never listed or served.
    #[serde(default)]
    pub hide: Vec<String>,
    #[serde(default)]
    pub sort: SortKey,
    #[serde(default)]
    pub order: Order,
    /// Send files as attachment (click downloads instead of opening in the browser).
    #[serde(default)]
    pub download: bool,
    /// Charset added to text/* responses, e.g. "windows-1252".
    pub charset: Option<String>,
}

impl FileConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

/// A directory served under `/<name>/` (or under `/` when `name` is empty).
#[derive(Debug)]
pub struct Share {
    pub name: String,
    pub root: PathBuf,
    pub description: String,
    pub allow: Option<GlobSet>,
    pub hide: GlobSet,
    pub sort: SortKey,
    pub order: Order,
    pub download: bool,
    pub charset: Option<String>,
}

impl Share {
    pub fn new(cfg: ShareConfig) -> Result<Self> {
        if cfg.name.contains('/') || cfg.name.starts_with('.') || cfg.name.starts_with('_') {
            bail!(
                "share name {:?}: must not contain '/' or start with '.' or '_'",
                cfg.name
            );
        }
        let root = cfg
            .path
            .canonicalize()
            .with_context(|| format!("share {:?}: path {}", cfg.name, cfg.path.display()))?;
        if !root.is_dir() {
            bail!(
                "share {:?}: {} is not a directory",
                cfg.name,
                root.display()
            );
        }
        let allow = if cfg.allow.is_empty() {
            None
        } else {
            Some(globs(&cfg.allow)?)
        };
        Ok(Self {
            name: cfg.name,
            root,
            description: cfg.description,
            allow,
            hide: globs(&cfg.hide)?,
            sort: cfg.sort,
            order: cfg.order,
            download: cfg.download,
            charset: cfg.charset,
        })
    }

    /// URL prefix including the trailing slash.
    pub fn mount(&self) -> String {
        if self.name.is_empty() {
            "/".into()
        } else {
            format!("/{}/", self.name)
        }
    }

    pub fn is_hidden(&self, rel: &str) -> bool {
        self.hide.is_match(rel)
    }

    pub fn is_allowed_file(&self, rel: &str) -> bool {
        !self.is_hidden(rel) && self.allow.as_ref().is_none_or(|a| a.is_match(rel))
    }
}

/// Patterns without '/' match the file name at any depth, others match the relative path.
fn globs(patterns: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let p = if p.contains('/') {
            p.trim_start_matches('/').to_string()
        } else {
            format!("**/{p}")
        };
        // `*` must not cross directory boundaries
        b.add(
            GlobBuilder::new(&p)
                .literal_separator(true)
                .build()
                .with_context(|| format!("glob {p:?}"))?,
        );
    }
    Ok(b.build()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_respect_separators() {
        let g = globs(&["*/*.txt".into(), "strans.sh".into()]).unwrap();
        assert!(g.is_match("000000015/a.txt"));
        assert!(!g.is_match("000000015/sub/a.txt"));
        assert!(!g.is_match("a.txt"));
        assert!(g.is_match("strans.sh"));
        assert!(g.is_match("000000015/strans.sh"));
    }
}
