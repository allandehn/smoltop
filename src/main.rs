//! Terminal loop. smolvm calls run on a worker thread so a stats sample
//! (about half a second) or a slow start does not freeze the UI. The
//! interactive shell is the exception: it needs the terminal, so the TUI
//! steps aside and comes back when `machine exec` exits.

#![forbid(unsafe_code)]

use std::io::{self, Stdout, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use smoltop::app::{App, Effect, Job, Outcome};
use smoltop::config::{self, load_config};
use smoltop::dispatch;
use smoltop::runner::{SmolvmRunner, SystemRunner};
use smoltop::ui;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("smoltop: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let path = config::config_path();
    let (loaded, writable, startup_error) = match load_config(&path) {
        Ok(config) => (config, true, None),
        Err(error) => (config::Config::default(), false, Some(error)),
    };
    let mut app = App::new(loaded, path, writable);
    if let Some(error) = startup_error {
        app.set_startup_error(error);
    }

    let mut stdout = io::stdout();
    enable_raw_mode().map_err(|error| error.to_string())?;
    if let Err(error) = execute!(stdout, EnterAlternateScreen, Hide) {
        let _ = disable_raw_mode();
        return Err(error.to_string());
    }
    let _restore = Restore;

    let mut terminal =
        Terminal::new(CrosstermBackend::new(stdout)).map_err(|error| error.to_string())?;
    terminal.clear().map_err(|error| error.to_string())?;

    let (job_tx, job_rx) = std::sync::mpsc::channel::<Job>();
    let (outcome_tx, outcome_rx) = std::sync::mpsc::channel::<Outcome>();
    std::thread::spawn(move || {
        let runner = SystemRunner::new();
        while let Ok(job) = job_rx.recv() {
            let outcome = dispatch::dispatch(&runner, job);
            if outcome_tx.send(outcome).is_err() {
                break;
            }
        }
    });

    loop {
        terminal
            .draw(|frame| ui::draw(frame, &app))
            .map_err(|error| error.to_string())?;

        while let Ok(outcome) = outcome_rx.try_recv() {
            let effects = app.apply(outcome);
            if apply_effects(&mut app, effects, &job_tx, &outcome_rx, &mut terminal)? {
                return Ok(());
            }
        }

        if event::poll(Duration::from_millis(100)).map_err(|error| error.to_string())? {
            if let Event::Key(key) = event::read().map_err(|error| error.to_string())? {
                let effects = app.on_key(key);
                if apply_effects(&mut app, effects, &job_tx, &outcome_rx, &mut terminal)? {
                    return Ok(());
                }
            }
        }

        let effects = app.on_tick(Instant::now());
        if apply_effects(&mut app, effects, &job_tx, &outcome_rx, &mut terminal)? {
            return Ok(());
        }
    }
}

fn apply_effects(
    app: &mut App,
    effects: Vec<Effect>,
    jobs: &std::sync::mpsc::Sender<Job>,
    outcomes: &std::sync::mpsc::Receiver<Outcome>,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> Result<bool, String> {
    for effect in effects {
        match effect {
            Effect::Quit => return Ok(true),
            Effect::SaveConfig => {
                let result = config::save_config(&app.config_path, &app.config);
                app.note_config_save(result);
            }
            Effect::Job(job) => {
                jobs.send(job)
                    .map_err(|_| "smolvm worker stopped".to_string())?;
            }
            Effect::Shell(args) => {
                wait_until_idle(app, jobs, outcomes)?;
                let code = suspend_for_shell(&args)?;
                terminal.clear().map_err(|error| error.to_string())?;
                app.note_shell(code);
                let refresh = app.request_refresh();
                if apply_effects(app, refresh, jobs, outcomes, terminal)? {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn wait_until_idle(
    app: &mut App,
    jobs: &std::sync::mpsc::Sender<Job>,
    outcomes: &std::sync::mpsc::Receiver<Outcome>,
) -> Result<(), String> {
    while app.is_busy() {
        let outcome = outcomes
            .recv()
            .map_err(|_| "smolvm worker stopped".to_string())?;
        for effect in app.apply(outcome) {
            match effect {
                Effect::Job(job) => {
                    jobs.send(job)
                        .map_err(|_| "smolvm worker stopped".to_string())?;
                }
                Effect::SaveConfig => {
                    let result = config::save_config(&app.config_path, &app.config);
                    app.note_config_save(result);
                }
                Effect::Quit | Effect::Shell(_) => {}
            }
        }
    }
    Ok(())
}

fn suspend_for_shell(args: &[String]) -> Result<i32, String> {
    disable_raw_mode().map_err(|error| error.to_string())?;
    let mut output = io::stdout();
    execute!(output, Show, LeaveAlternateScreen).map_err(|error| error.to_string())?;
    let _ = output.flush();
    let result = SystemRunner::new().run_inherited(args);
    enable_raw_mode().map_err(|error| error.to_string())?;
    execute!(output, EnterAlternateScreen, Hide).map_err(|error| error.to_string())?;
    result
}

struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        let mut output = io::stdout();
        let _ = disable_raw_mode();
        let _ = execute!(output, Show, LeaveAlternateScreen);
    }
}
