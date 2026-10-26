//! Detect Omarchy theme switches without extra dependencies.
//!
//! Checked against [basecamp/omarchy](https://github.com/basecamp/omarchy):
//! `bin/omarchy-theme-set` sets `CURRENT_THEME_PATH` to
//! `$HOME/.local/state/omarchy/current/theme` and replaces that directory
//! (`rm` then `mv`) on each switch. `migrations/1781043107.sh` moved the
//! older `$HOME/.config/omarchy/current` tree, whose `theme` entry was a
//! symlink, to `~/.local/state/omarchy/current`. Terminals reload their ANSI
//! palette from the new theme; smoltop only has to notice the path changed
//! and redraw.
//!
//! A missing path is not an error. The next poll notices if it appears.

use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use std::os::unix::fs::MetadataExt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeStamp {
    present: bool,
    symlink_target: Option<String>,
    modified: Option<SystemTime>,
    ino: Option<u64>,
}

impl ThemeStamp {
    fn missing() -> Self {
        Self {
            present: false,
            symlink_target: None,
            modified: None,
            ino: None,
        }
    }
}

/// Snapshot of one theme path. Symlink retargets show up in
/// `symlink_target`. A replaced directory shows up as a new inode or mtime.
pub fn read_theme_stamp(path: &Path) -> ThemeStamp {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(_) => return ThemeStamp::missing(),
    };
    let symlink_target = if meta.file_type().is_symlink() {
        std::fs::read_link(path)
            .ok()
            .map(|target| target.to_string_lossy().into_owned())
    } else {
        None
    };
    ThemeStamp {
        present: true,
        symlink_target,
        modified: meta.modified().ok(),
        ino: Some(meta.ino()),
    }
}

/// Paths polled each tick. An explicit `theme_path` replaces the defaults.
/// When it is unset, both the current Omarchy directory and the legacy
/// symlink are watched. Neither has to exist.
pub fn watch_paths(
    theme_path: Option<&str>,
    home: Option<&str>,
    cwd: &Path,
) -> Result<Vec<PathBuf>, String> {
    if let Some(theme_path) = theme_path {
        return Ok(vec![resolve_theme_path(theme_path, home, cwd)?]);
    }
    Ok(default_theme_paths(home))
}

pub fn default_theme_paths(home: Option<&str>) -> Vec<PathBuf> {
    let Some(home) = home.filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
    else {
        return Vec::new();
    };
    let home = PathBuf::from(home);
    vec![
        home.join(".local/state/omarchy/current/theme"),
        home.join(".config/omarchy/current/theme"),
    ]
}

pub fn resolve_theme_path(
    theme_path: &str,
    home: Option<&str>,
    cwd: &Path,
) -> Result<PathBuf, String> {
    if theme_path.is_empty() || theme_path.chars().any(char::is_control) {
        return Err("theme_path is empty or contains control characters".into());
    }
    let expanded = crate::config::expand_tilde(theme_path, home)?;
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    };
    if absolute
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("theme_path must not contain '..'".into());
    }
    if absolute.to_str().is_none() {
        return Err("theme_path is not valid utf-8".into());
    }
    Ok(absolute)
}

#[derive(Debug)]
pub struct ThemeWatch {
    paths: Vec<PathBuf>,
    stamp: Vec<ThemeStamp>,
}

impl ThemeWatch {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let stamp = paths.iter().map(|path| read_theme_stamp(path)).collect();
        Self { paths, stamp }
    }

    /// `true` when a watched path appeared, disappeared, was retargeted, or
    /// had its inode/mtime replaced since the last call.
    pub fn poll(&mut self) -> bool {
        let next: Vec<ThemeStamp> = self
            .paths
            .iter()
            .map(|path| read_theme_stamp(path))
            .collect();
        if next == self.stamp {
            return false;
        }
        self.stamp = next;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "smoltop-theme-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn symlink_retarget_and_directory_replace_are_changes() {
        let dir = temp_dir("watch");
        let a = dir.join("theme-a");
        let b = dir.join("theme-b");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let link = dir.join("theme");
        symlink(&a, &link).unwrap();

        let missing = dir.join("not-here");
        let mut quiet = ThemeWatch::new(vec![missing]);
        assert!(!quiet.poll());
        assert!(!quiet.poll());

        let mut watch = ThemeWatch::new(vec![link.clone()]);
        assert!(!watch.poll(), "first polls keep the baseline");
        std::fs::remove_file(&link).unwrap();
        symlink(&b, &link).unwrap();
        assert!(watch.poll(), "repointing the symlink is a change");
        assert!(!watch.poll(), "the new target stays quiet");

        let live = dir.join("current");
        let next = dir.join("next");
        std::fs::create_dir(&live).unwrap();
        std::fs::create_dir(&next).unwrap();
        std::fs::write(next.join("colors.toml"), b"x").unwrap();
        let mut replaced = ThemeWatch::new(vec![live.clone()]);
        assert!(!replaced.poll());
        // Same shape as omarchy-theme-set: remove the live dir, then move the
        // staged dir into its place. That keeps the staged inode.
        std::fs::remove_dir(&live).unwrap();
        std::fs::rename(&next, &live).unwrap();
        assert!(replaced.poll(), "replacing the theme directory is a change");
        assert!(!replaced.poll());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn explicit_path_is_watched_and_defaults_cover_both_layouts() {
        let cwd = PathBuf::from("/work");
        let home = Some("/home/allan");
        assert_eq!(
            watch_paths(Some("~/omarchy/current/theme"), home, &cwd).unwrap(),
            vec![PathBuf::from("/home/allan/omarchy/current/theme")]
        );
        assert_eq!(
            watch_paths(None, home, &cwd).unwrap(),
            vec![
                PathBuf::from("/home/allan/.local/state/omarchy/current/theme"),
                PathBuf::from("/home/allan/.config/omarchy/current/theme"),
            ]
        );
        assert!(watch_paths(None, None, &cwd).unwrap().is_empty());
        assert!(watch_paths(Some("../x"), home, &cwd).is_err());
        assert!(resolve_theme_path("/no/such/theme", home, &cwd).is_ok());
    }
}
