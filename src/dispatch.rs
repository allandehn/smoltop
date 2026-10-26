//! Turns a [`Job`] into an [`Outcome`] by asking a [`SmolvmRunner`].
//!
//! The real runner shells out to `smolvm`. Tests pass a fake that never
//! spawns a process.

use crate::app::{Job, Outcome};
use crate::commands;
use crate::parse::{interpret_action, interpret_list, interpret_stats};
use crate::runner::SmolvmRunner;

pub fn dispatch(runner: &impl SmolvmRunner, job: Job) -> Outcome {
    match job {
        Job::List => Outcome::List(interpret_list(runner.run(&commands::list_args()))),
        Job::Stats { name, args } => Outcome::Stats {
            name,
            result: interpret_stats(runner.run(&args)),
        },
        Job::Action { id, args } => Outcome::Action {
            id,
            result: interpret_action(runner.run(&args)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ActionId;
    use crate::parse::CommandOutput;
    use crate::runner::SmolvmRunner;
    use std::sync::Mutex;

    struct Fake {
        calls: Mutex<Vec<Vec<String>>>,
        output: CommandOutput,
    }

    impl SmolvmRunner for Fake {
        fn run(&self, args: &[String]) -> Result<CommandOutput, String> {
            self.calls.lock().expect("lock").push(args.to_vec());
            Ok(self.output.clone())
        }

        fn run_inherited(&self, _args: &[String]) -> Result<i32, String> {
            Err("inherited runs are not used by dispatch".into())
        }
    }

    #[test]
    fn list_job_parses_stdout_from_the_fake() {
        let fake = Fake {
            calls: Mutex::new(Vec::new()),
            output: CommandOutput {
                status: 0,
                stdout: r#"[{"name":"web","state":"stopped","cpus":1,"memory_mib":512}]"#.into(),
                stderr: String::new(),
            },
        };
        let Outcome::List(Ok(machines)) = dispatch(&fake, Job::List) else {
            panic!("expected a list");
        };
        assert_eq!(machines[0].name, "web");
        assert_eq!(fake.calls.lock().unwrap()[0], commands::list_args());
    }

    #[test]
    fn action_failure_uses_the_json_error() {
        let fake = Fake {
            calls: Mutex::new(Vec::new()),
            output: CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "{\"error\":{\"code\":\"NOT_FOUND\",\"message\":\"missing\"}}".into(),
            },
        };
        let args = commands::start_args("web").unwrap();
        let Outcome::Action {
            id: ActionId::Start { name },
            result: Err(message),
        } = dispatch(
            &fake,
            Job::Action {
                id: ActionId::Start { name: "web".into() },
                args,
            },
        )
        else {
            panic!("expected a failed start");
        };
        assert_eq!(name, "web");
        assert_eq!(message, "NOT_FOUND: missing");
    }

    #[test]
    fn stats_degrade_on_unrecognized_output() {
        let fake = Fake {
            calls: Mutex::new(Vec::new()),
            output: CommandOutput {
                status: 0,
                stdout: "no stats here".into(),
                stderr: String::new(),
            },
        };
        let Outcome::Stats {
            result: Err(message),
            ..
        } = dispatch(
            &fake,
            Job::Stats {
                name: "web".into(),
                args: commands::stats_args("web").unwrap(),
            },
        )
        else {
            panic!("expected stats");
        };
        assert!(message.contains("unrecognized"));
    }
}
