//! The only place smoltop starts a process.
//!
//! `SystemRunner` runs the `smolvm` on `PATH` via `std::process::Command`.
//! Arguments are separate `Command` values. There is no shell, no `sh -c`,
//! and no way to point this runner at another program.

use std::process::{Command, Stdio};

use crate::commands::screen_invocation;
use crate::parse::CommandOutput;

pub trait SmolvmRunner {
    fn run(&self, args: &[String]) -> Result<CommandOutput, String>;
    fn run_inherited(&self, args: &[String]) -> Result<i32, String>;
}

#[derive(Debug, Default)]
pub struct SystemRunner;

impl SystemRunner {
    pub fn new() -> Self {
        Self
    }
}

impl SmolvmRunner for SystemRunner {
    fn run(&self, args: &[String]) -> Result<CommandOutput, String> {
        let mut command = prepare("smolvm", args, false)?;
        let output = command.output().map_err(spawn_error)?;
        Ok(CommandOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn run_inherited(&self, args: &[String]) -> Result<i32, String> {
        let mut command = prepare("smolvm", args, true)?;
        let status = command.status().map_err(spawn_error)?;
        Ok(status.code().unwrap_or(-1))
    }
}

pub fn prepare(program: &str, args: &[String], inherited: bool) -> Result<Command, String> {
    if program != "smolvm" {
        return Err("smoltop only runs smolvm".into());
    }
    screen_invocation(args)?;
    let mut command = Command::new(program);
    command.args(args);
    if inherited {
        command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
    } else {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    }
    Ok(command)
}

fn spawn_error(error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        "smolvm is not on PATH".into()
    } else {
        format!("failed to run smolvm: {error}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands;

    #[test]
    fn prepare_is_smolvm_with_separate_args_and_not_a_shell() {
        let args = commands::shell_args("web").unwrap();
        let command = prepare("smolvm", &args, false).unwrap();
        assert_eq!(command.get_program(), "smolvm");
        let got: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(got, args);
        assert!(prepare("sh", &args, false).is_err());
        assert!(prepare("bash", &args, false).is_err());
        assert!(prepare("sudo", &args, false).is_err());

        let mut forbidden = commands::list_args();
        forbidden.push("--ssh-agent".into());
        assert!(prepare("smolvm", &forbidden, true).is_err());
    }
}
