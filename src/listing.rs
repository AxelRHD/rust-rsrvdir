//! Directory listings: reading, filtering by the share rules, sorting.

use std::path::Path;
use std::time::SystemTime;

use serde::Deserialize;

use crate::config::Share;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    #[default]
    Name,
    Size,
    Time,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Order {
    #[default]
    Asc,
    Desc,
}

impl SortKey {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "name" => Some(Self::Name),
            "size" => Some(Self::Size),
            "time" => Some(Self::Time),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Size => "size",
            Self::Time => "time",
        }
    }
}

impl Order {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "asc" => Some(Self::Asc),
            "desc" => Some(Self::Desc),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }

    pub fn flip(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }
}

#[derive(Debug)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: SystemTime,
}

/// Reads `dir` (relative path `rel`, "" for the share root) and keeps only what the share allows:
/// directories that are not hidden, files that are allowed. Symlinks pointing outside the share are skipped.
pub fn read(share: &Share, dir: &Path, rel: &str) -> std::io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let Ok(e) = e else { continue };
        let Some(name) = e.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let path = e.path();
        if e.file_type().is_ok_and(|t| t.is_symlink())
            && !path
                .canonicalize()
                .is_ok_and(|p| p.starts_with(&share.root))
        {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        let rel_entry = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let visible = if meta.is_dir() {
            !share.is_hidden(&rel_entry)
        } else {
            share.is_allowed_file(&rel_entry)
        };
        if visible {
            entries.push(Entry {
                name,
                is_dir: meta.is_dir(),
                size: meta.len(),
                modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            });
        }
    }
    Ok(entries)
}

/// Directories first, then by key; ties broken by name.
pub fn sort(entries: &mut [Entry], key: SortKey, order: Order) {
    entries.sort_by(|a, b| {
        let by_key = match key {
            SortKey::Name => std::cmp::Ordering::Equal,
            SortKey::Size => a.size.cmp(&b.size),
            SortKey::Time => a.modified.cmp(&b.modified),
        }
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        let by_key = if order == Order::Desc {
            by_key.reverse()
        } else {
            by_key
        };
        b.is_dir.cmp(&a.is_dir).then(by_key)
    });
}

pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn e(name: &str, is_dir: bool, secs: u64) -> Entry {
        Entry {
            name: name.into(),
            is_dir,
            size: secs,
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
        }
    }

    #[test]
    fn dirs_first_then_newest() {
        let mut v = vec![
            e("a.txt", false, 1),
            e("b.txt", false, 3),
            e("d", true, 0),
            e("c.txt", false, 2),
        ];
        sort(&mut v, SortKey::Time, Order::Desc);
        let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["d", "b.txt", "c.txt", "a.txt"]);
    }

    #[test]
    fn sizes() {
        assert_eq!(format_size(731), "731 B");
        assert_eq!(format_size(6_920_601), "6.6 MB");
    }
}
