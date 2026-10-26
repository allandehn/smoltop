//! `*.smolmachine` files in the configured packs directory.
//!
//! Only direct children are listed. The file stem has to pass the same name
//! check as a machine name, so a stray `../` or flag-like filename is ignored.

use std::path::Path;

use crate::names::validate_name;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRef {
    pub name: String,
    pub path: std::path::PathBuf,
}

pub fn list_packs(dir: &Path) -> Result<Vec<PackRef>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!("cannot read packs dir {}: {error}", dir.display()));
        }
    };
    let mut packs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let Some(file_name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Some(stem) = file_name.strip_suffix(".smolmachine") else {
            continue;
        };
        if validate_name(stem).is_err() {
            continue;
        }
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        packs.push(PackRef {
            name: stem.to_string(),
            path,
        });
    }
    packs.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(packs)
}

pub fn ensure_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("cannot create packs dir {}: {error}", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn lists_only_safe_smolmachine_files() {
        let dir = std::env::temp_dir().join(format!(
            "smoltop-packs-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("base.smolmachine"), b"a").unwrap();
        fs::write(dir.join("..smolmachine"), b"a").unwrap();
        fs::write(dir.join("-bad.smolmachine"), b"a").unwrap();
        fs::write(dir.join("notes.txt"), b"a").unwrap();
        fs::write(dir.join("has space.smolmachine"), b"a").unwrap();
        fs::create_dir(dir.join("dir.smolmachine")).unwrap();

        let packs = list_packs(&dir).unwrap();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].name, "base");
        assert!(list_packs(&dir.join("missing")).unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
