//! `~/.config/smoltop/config.toml` (or `$XDG_CONFIG_HOME/smoltop/config.toml`).
//!
//! Missing file means the defaults: network on, DNS `100.64.0.7`, packs in
//! `~/smolvm`. A create that edits DNS or the network switch writes the file.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::names::validate_dns;

pub const DEFAULT_DNS: &str = "100.64.0.7";
pub const DEFAULT_PACKS_DIR: &str = "~/smolvm";

fn default_dns() -> String {
    DEFAULT_DNS.to_string()
}

fn default_network() -> bool {
    true
}

fn default_packs_dir() -> String {
    DEFAULT_PACKS_DIR.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_dns")]
    pub dns: String,
    #[serde(default = "default_network")]
    pub network: bool,
    #[serde(default = "default_packs_dir")]
    pub packs_dir: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            dns: default_dns(),
            network: default_network(),
            packs_dir: default_packs_dir(),
        }
    }
}

impl Config {
    pub fn packs_path(&self) -> Result<PathBuf, String> {
        let home = std::env::var("HOME").ok();
        let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
        resolve_packs_dir(&self.packs_dir, home.as_deref(), &cwd)
    }
}

pub fn config_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() && !xdg.chars().any(char::is_control) {
            return PathBuf::from(xdg).join("smoltop").join("config.toml");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
        .join(".config")
        .join("smoltop")
        .join("config.toml")
}

pub fn load_config(path: &Path) -> Result<Config, String> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let config: Config = toml::from_str(&text)
        .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
    validate_dns(&config.dns)?;
    resolve_packs_dir(
        &config.packs_dir,
        std::env::var("HOME").ok().as_deref(),
        &std::env::current_dir().map_err(|error| error.to_string())?,
    )?;
    Ok(config)
}

pub fn save_config(path: &Path, config: &Config) -> Result<(), String> {
    validate_dns(&config.dns)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let body = toml::to_string_pretty(config).map_err(|error| error.to_string())?;
    let text = format!(
        "# smoltop configuration\n\
         # dns is passed as --dns when network is true.\n\
         # packs_dir is scanned for *.smolmachine files.\n\
         {body}"
    );
    std::fs::write(path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

pub fn resolve_packs_dir(
    packs_dir: &str,
    home: Option<&str>,
    cwd: &Path,
) -> Result<PathBuf, String> {
    if packs_dir.is_empty() || packs_dir.chars().any(char::is_control) {
        return Err("packs_dir is empty or contains control characters".into());
    }
    let expanded = expand_tilde(packs_dir, home)?;
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    };
    if absolute
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("packs_dir must not contain '..'".into());
    }
    if absolute.to_str().is_none() {
        return Err("packs_dir is not valid utf-8".into());
    }
    Ok(absolute)
}

pub fn expand_tilde(path: &str, home: Option<&str>) -> Result<PathBuf, String> {
    if path == "~" || path.starts_with("~/") {
        let Some(home) = home.filter(|value| !value.is_empty()) else {
            return Err("HOME is not set; cannot expand ~ in packs_dir".into());
        };
        if home.chars().any(char::is_control) {
            return Err("HOME contains control characters".into());
        }
        if path == "~" {
            return Ok(PathBuf::from(home));
        }
        return Ok(PathBuf::from(home).join(&path[2..]));
    }
    Ok(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "smoltop-cfg-{label}-{}-{}",
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
    fn missing_file_is_the_default() {
        let path = temp_dir("missing").join("config.toml");
        let config = load_config(&path).unwrap();
        assert_eq!(config, Config::default());
        assert_eq!(config.dns, "100.64.0.7");
        assert!(config.network);
        assert_eq!(config.packs_dir, "~/smolvm");
    }

    #[test]
    fn partial_file_fills_defaults_and_roundtrips() {
        let dir = temp_dir("round");
        let path = dir.join("config.toml");
        std::fs::write(&path, "dns = \"8.8.8.8\"\n").unwrap();
        let config = load_config(&path).unwrap();
        assert_eq!(config.dns, "8.8.8.8");
        assert!(config.network);
        assert_eq!(config.packs_dir, "~/smolvm");

        let mut edited = config;
        edited.network = false;
        edited.packs_dir = dir.to_str().unwrap().to_string();
        save_config(&path, &edited).unwrap();
        let loaded = load_config(&path).unwrap();
        assert_eq!(loaded, edited);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_dns_unknown_field_and_parent_dir_are_errors() {
        let dir = temp_dir("bad");
        let path = dir.join("config.toml");
        std::fs::write(&path, "dns = \"nope\"\n").unwrap();
        assert!(load_config(&path).is_err());
        std::fs::write(&path, "dns = \"1.1.1.1\"\nextra = 1\n").unwrap();
        assert!(load_config(&path).is_err());
        assert!(resolve_packs_dir("~/smolvm/../.ssh", Some("/home/allan"), &dir).is_err());
        assert!(resolve_packs_dir("", Some("/home/allan"), &dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tilde_and_relative_packs_dirs() {
        let cwd = PathBuf::from("/work");
        assert_eq!(
            resolve_packs_dir("~/smolvm", Some("/home/allan"), &cwd).unwrap(),
            PathBuf::from("/home/allan/smolvm")
        );
        assert_eq!(
            resolve_packs_dir("packs", Some("/home/allan"), &cwd).unwrap(),
            PathBuf::from("/work/packs")
        );
        assert!(expand_tilde("~/smolvm", None).is_err());
    }
}
