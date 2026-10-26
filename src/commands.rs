//! Argument vectors for the smolvm commands smoltop uses.
//!
//! Checked against smolvm v1.32.3 (`src/main.rs`, `src/cli/machine.rs`,
//! `src/cli/pack.rs`):
//! - `--error-format` is a global flag (`text` | `json`).
//! - `machine create/start/stop/delete/stats` take the machine as `--name` /
//!   `-n`, not as a positional argument.
//! - `machine exec` takes `--name` and has no `-n` short flag. The guest
//!   command is a trailing argument after `--`. `-i` and `-t` combine as `-it`.
//! - `machine ls --json` prints a JSON array. `machine stats --json` prints one
//!   object. Stats also has a human text form.
//! - `machine delete --force` skips smolvm's own prompt. smoltop asks y/n first.
//! - `pack create --from-vm NAME -o PATH` writes an executable stub at PATH and
//!   a sidecar at `PATH.smolmachine`. `--output` must not itself end in
//!   `.smolmachine`.
//! - `machine create --from PATH` conflicts with `--image`. `--net` enables
//!   outbound networking. `--dns IP` is an IPv4 resolver and implies `--net`.
//!   On `--from`, smolvm ORs `--net` with the pack's network bit.

use std::path::{Component, Path, PathBuf};

use crate::names::{validate_dns, validate_image, validate_name};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateRequest {
    pub name: String,
    pub source: CreateSource,
    pub network: bool,
    pub dns: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateSource {
    Image(String),
    Pack(PathBuf),
}

pub fn list_args() -> Vec<String> {
    vec![
        "--error-format".into(),
        "json".into(),
        "machine".into(),
        "ls".into(),
        "--json".into(),
    ]
}

pub fn stats_args(name: &str) -> Result<Vec<String>, String> {
    named("stats", name, &["--json"])
}

pub fn start_args(name: &str) -> Result<Vec<String>, String> {
    named("start", name, &[])
}

pub fn stop_args(name: &str) -> Result<Vec<String>, String> {
    named("stop", name, &[])
}

/// Delete after smoltop's own y/n prompt. `--force` is what stops smolvm from
/// reading a second confirmation off the terminal.
pub fn delete_args(name: &str) -> Result<Vec<String>, String> {
    named("delete", name, &["--force"])
}

/// `smolvm --error-format json machine exec --name NAME -it -- bash`
///
/// `machine exec` has no `-n`; the name is only passed as `--name`.
pub fn shell_args(name: &str) -> Result<Vec<String>, String> {
    validate_name(name)?;
    let args = vec![
        "--error-format".into(),
        "json".into(),
        "machine".into(),
        "exec".into(),
        "--name".into(),
        name.to_string(),
        "-it".into(),
        "--".into(),
        "bash".into(),
    ];
    debug_assert!(!args.iter().any(|arg| arg == "-n"));
    screen_invocation(&args)?;
    Ok(args)
}

pub fn create_args(request: &CreateRequest) -> Result<Vec<String>, String> {
    validate_name(&request.name)?;
    let mut args = global("machine", "create");
    args.push("--name".into());
    args.push(request.name.clone());
    match &request.source {
        CreateSource::Image(image) => {
            validate_image(image)?;
            args.push("--image".into());
            args.push(image.clone());
        }
        CreateSource::Pack(path) => {
            let path = safe_pack_path(path)?;
            args.push("--from".into());
            args.push(path);
        }
    }
    if request.network {
        let dns = validate_dns(&request.dns)?;
        args.push("--net".into());
        args.push("--dns".into());
        args.push(dns);
    }
    screen_invocation(&args)?;
    Ok(args)
}

/// `smolvm pack create --from-vm NAME -o <packs dir>/NAME`
///
/// `stub` is the executable path. The sidecar is [`sidecar_path`].
pub fn pack_args(name: &str, stub: &Path) -> Result<Vec<String>, String> {
    validate_name(name)?;
    let stub = safe_stub_path(name, stub)?;
    let mut args = global("pack", "create");
    args.push("--from-vm".into());
    args.push(name.to_string());
    args.push("-o".into());
    args.push(stub);
    screen_invocation(&args)?;
    Ok(args)
}

pub fn sidecar_path(stub: &Path) -> PathBuf {
    let mut os = stub.as_os_str().to_os_string();
    os.push(".smolmachine");
    PathBuf::from(os)
}

/// Last-line defense: the runner calls this before `Command` is spawned.
pub fn screen_invocation(args: &[String]) -> Result<(), String> {
    if args.first().map(String::as_str) != Some("--error-format")
        || args.get(1).map(String::as_str) != Some("json")
    {
        return Err("smolvm invocations must start with --error-format json".into());
    }
    match args.get(2).map(String::as_str) {
        Some("machine" | "pack") => {}
        Some("serve") => return Err("refusing to run smolvm serve".into()),
        _ => return Err("unexpected smolvm command".into()),
    }
    for arg in args {
        if is_forbidden_arg(arg) {
            return Err(format!("refusing forbidden argument {arg}"));
        }
    }
    for window in args.windows(2) {
        if (window[0] == "sh" || window[0] == "bash") && window[1] == "-c" {
            return Err("refusing to run a shell".into());
        }
    }
    Ok(())
}

fn is_forbidden_arg(arg: &str) -> bool {
    arg == "sudo"
        || arg == "--ssh-agent"
        || arg == "--volume"
        || arg == "-v"
        || arg == "--allow-system-mounts"
        || arg == "--docker-socket"
        || arg == "--mount-socket"
        || arg.starts_with("--secret-")
}

fn global(group: &str, action: &str) -> Vec<String> {
    vec![
        "--error-format".into(),
        "json".into(),
        group.into(),
        action.into(),
    ]
}

fn named(action: &str, name: &str, extra: &[&str]) -> Result<Vec<String>, String> {
    validate_name(name)?;
    let mut args = global("machine", action);
    args.push("--name".into());
    args.push(name.to_string());
    args.extend(extra.iter().map(|part| (*part).to_string()));
    screen_invocation(&args)?;
    Ok(args)
}

fn safe_pack_path(path: &Path) -> Result<String, String> {
    let text = safe_absolute_utf8(path)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "pack path has no file name".to_string())?;
    let stem = name
        .strip_suffix(".smolmachine")
        .ok_or_else(|| "pack path must end in .smolmachine".to_string())?;
    validate_name(stem)?;
    if !path.is_file() {
        return Err(format!("pack not found: {text}"));
    }
    Ok(text)
}

fn safe_stub_path(name: &str, stub: &Path) -> Result<String, String> {
    let text = safe_absolute_utf8(stub)?;
    let file_name = stub
        .file_name()
        .and_then(|file| file.to_str())
        .ok_or_else(|| "pack output has no file name".to_string())?;
    if file_name != name {
        return Err("pack output file name must be the machine name".into());
    }
    if file_name.ends_with(".smolmachine") {
        return Err("pack -o names the stub, not the .smolmachine sidecar".into());
    }
    Ok(text)
}

fn safe_absolute_utf8(path: &Path) -> Result<String, String> {
    if !path.is_absolute() {
        return Err("path must be absolute".into());
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("path must not contain '..'".into());
    }
    let text = path
        .to_str()
        .ok_or_else(|| "path is not valid utf-8".to_string())?;
    if text.starts_with('-') || text.chars().any(char::is_control) {
        return Err("path is not allowed".into());
    }
    Ok(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn pack_file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(format!("{name}.smolmachine"));
        fs::write(&path, b"not-a-real-pack").unwrap();
        path
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "smoltop-cmd-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn list_and_lifecycle_args_match_v1_32_3() {
        assert_eq!(
            list_args(),
            vec!["--error-format", "json", "machine", "ls", "--json",]
        );
        assert_eq!(
            start_args("web").unwrap(),
            vec![
                "--error-format",
                "json",
                "machine",
                "start",
                "--name",
                "web",
            ]
        );
        assert_eq!(
            stop_args("web").unwrap(),
            vec!["--error-format", "json", "machine", "stop", "--name", "web",]
        );
        assert_eq!(
            delete_args("web").unwrap(),
            vec![
                "--error-format",
                "json",
                "machine",
                "delete",
                "--name",
                "web",
                "--force",
            ]
        );
        assert_eq!(
            stats_args("web").unwrap(),
            vec![
                "--error-format",
                "json",
                "machine",
                "stats",
                "--name",
                "web",
                "--json",
            ]
        );
        assert_eq!(
            shell_args("web").unwrap(),
            vec![
                "--error-format",
                "json",
                "machine",
                "exec",
                "--name",
                "web",
                "-it",
                "--",
                "bash",
            ]
        );
        assert!(!shell_args("web").unwrap().iter().any(|arg| arg == "-n"));
    }

    #[test]
    fn create_image_pack_and_pack_args() {
        let dir = temp_dir("create");
        let pack = pack_file(&dir, "base");
        let with_net = create_args(&CreateRequest {
            name: "web".into(),
            source: CreateSource::Image("ghcr.io/org/app:1".into()),
            network: true,
            dns: "100.64.0.7".into(),
        })
        .unwrap();
        assert_eq!(
            with_net,
            vec![
                "--error-format",
                "json",
                "machine",
                "create",
                "--name",
                "web",
                "--image",
                "ghcr.io/org/app:1",
                "--net",
                "--dns",
                "100.64.0.7",
            ]
        );

        let offline = create_args(&CreateRequest {
            name: "web".into(),
            source: CreateSource::Image("alpine".into()),
            network: false,
            dns: "100.64.0.7".into(),
        })
        .unwrap();
        assert!(!offline.iter().any(|arg| arg == "--net" || arg == "--dns"));

        let from_pack = create_args(&CreateRequest {
            name: "tmp-1".into(),
            source: CreateSource::Pack(pack.clone()),
            network: true,
            dns: "100.64.0.7".into(),
        })
        .unwrap();
        assert_eq!(
            from_pack,
            vec![
                "--error-format".to_string(),
                "json".into(),
                "machine".into(),
                "create".into(),
                "--name".into(),
                "tmp-1".into(),
                "--from".into(),
                pack.to_str().unwrap().to_string(),
                "--net".into(),
                "--dns".into(),
                "100.64.0.7".into(),
            ]
        );

        let stub = dir.join("web");
        assert_eq!(
            pack_args("web", &stub).unwrap(),
            vec![
                "--error-format".to_string(),
                "json".into(),
                "pack".into(),
                "create".into(),
                "--from-vm".into(),
                "web".into(),
                "-o".into(),
                stub.to_str().unwrap().to_string(),
            ]
        );
        assert_eq!(sidecar_path(&stub), dir.join("web.smolmachine"));
        let _ = fs::remove_dir_all(&dir);
    }

    /// Copy is `machine create --from` with the same network flags as create
    /// and throwaway: `--net --dns <config dns>` when network is enabled.
    #[test]
    fn copy_from_pack_passes_configured_net_and_dns() {
        let dir = temp_dir("copy-net");
        let pack = pack_file(&dir, "web");
        let args = create_args(&CreateRequest {
            name: "web-copy".into(),
            source: CreateSource::Pack(pack.clone()),
            network: true,
            dns: "100.64.0.7".into(),
        })
        .unwrap();
        assert_eq!(
            args,
            vec![
                "--error-format".to_string(),
                "json".into(),
                "machine".into(),
                "create".into(),
                "--name".into(),
                "web-copy".into(),
                "--from".into(),
                pack.to_str().unwrap().to_string(),
                "--net".into(),
                "--dns".into(),
                "100.64.0.7".into(),
            ]
        );

        let offline = create_args(&CreateRequest {
            name: "web-copy".into(),
            source: CreateSource::Pack(pack),
            network: false,
            dns: "100.64.0.7".into(),
        })
        .unwrap();
        assert!(!offline.iter().any(|arg| arg == "--net" || arg == "--dns"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn builders_reject_bad_names_and_paths() {
        assert!(start_args("-web").is_err());
        assert!(start_args("has space").is_err());
        assert!(shell_args("..").is_err());
        assert!(create_args(&CreateRequest {
            name: "web".into(),
            source: CreateSource::Image("-alpine".into()),
            network: false,
            dns: "100.64.0.7".into(),
        })
        .is_err());
        assert!(create_args(&CreateRequest {
            name: "web".into(),
            source: CreateSource::Image("alpine".into()),
            network: true,
            dns: "nope".into(),
        })
        .is_err());

        let relative = PathBuf::from("base.smolmachine");
        assert!(create_args(&CreateRequest {
            name: "web".into(),
            source: CreateSource::Pack(relative),
            network: false,
            dns: "100.64.0.7".into(),
        })
        .is_err());

        let escaped = PathBuf::from("/tmp/../etc/passwd.smolmachine");
        assert!(pack_args("passwd.smolmachine", &escaped).is_err());
        assert!(create_args(&CreateRequest {
            name: "web".into(),
            source: CreateSource::Pack(escaped),
            network: false,
            dns: "100.64.0.7".into(),
        })
        .is_err());
    }

    #[test]
    fn screen_blocks_forbidden_flags_shell_and_serve() {
        let mut bad = list_args();
        bad.push("--volume".into());
        assert!(screen_invocation(&bad).is_err());

        for flag in [
            "--ssh-agent",
            "-v",
            "--allow-system-mounts",
            "--docker-socket",
            "--mount-socket",
            "--secret-env",
            "--secret-file",
            "--secret-keychain",
            "sudo",
        ] {
            let mut args = start_args("web").unwrap();
            args.push(flag.into());
            assert!(screen_invocation(&args).is_err(), "{flag}");
        }

        assert!(
            screen_invocation(&["--error-format".into(), "json".into(), "serve".into()]).is_err()
        );
        assert!(screen_invocation(&[
            "--error-format".into(),
            "json".into(),
            "machine".into(),
            "exec".into(),
            "--name".into(),
            "web".into(),
            "--".into(),
            "bash".into(),
            "-c".into(),
            "id".into(),
        ])
        .is_err());

        for args in [
            list_args(),
            start_args("web").unwrap(),
            stop_args("web").unwrap(),
            delete_args("web").unwrap(),
            stats_args("web").unwrap(),
            shell_args("serve").unwrap(),
        ] {
            assert!(screen_invocation(&args).is_ok());
        }
    }
}
