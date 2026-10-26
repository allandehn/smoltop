//! UI state. The terminal loop in `main` turns [`Effect`]s into smolvm jobs
//! and feeds [`Outcome`]s back. Nothing here touches the terminal or spawns
//! a process, so the key handling can be tested headlessly.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::commands::{self, CreateRequest, CreateSource};
use crate::config::Config;
use crate::names::{self, validate_name};
use crate::packs::{self, PackRef};
use crate::parse::{LiveStats, Machine};

pub const POLL_INTERVAL: Duration = Duration::from_millis(1500);

/// How the status line is painted. ANSI named colors only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    /// Machine count and other quiet notes.
    Info,
    /// A smolvm call is in flight, or the first list has not returned.
    Busy,
    /// The last action finished.
    Ok,
    /// The last action or check failed.
    Error,
}

#[derive(Debug)]
pub enum Effect {
    Job(Job),
    Shell(Vec<String>),
    SaveConfig,
    Quit,
}

#[derive(Debug)]
pub enum Job {
    List,
    Stats { name: String, args: Vec<String> },
    Action { id: ActionId, args: Vec<String> },
}

#[derive(Debug)]
pub enum Outcome {
    List(Result<Vec<Machine>, String>),
    Stats {
        name: String,
        result: Result<LiveStats, String>,
    },
    Action {
        id: ActionId,
        result: Result<(), String>,
    },
}

#[derive(Debug, Clone)]
pub enum ActionId {
    Start { name: String },
    Stop { name: String },
    Delete { name: String },
    Create { name: String },
    CreateCopy { name: String },
    Pack { name: String },
    PackForCopy { dest: String, sidecar: PathBuf },
}

#[derive(Debug)]
pub enum Mode {
    Normal,
    Help,
    Delete(DeletePrompt),
    Create(Box<CreateForm>),
    Throwaway(Box<ThrowawayPrompt>),
    Pack(PackPrompt),
    Copy(Box<CopyForm>),
}

#[derive(Debug, Clone)]
pub struct DeletePrompt {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct PackPrompt {
    pub name: String,
    pub stub: PathBuf,
}

#[derive(Debug, Clone)]
pub struct CopyForm {
    pub source: String,
    pub name: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ThrowawayPrompt {
    pub packs: Vec<PackRef>,
    pub index: usize,
    pub dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateSourceKind {
    Image,
    Pack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateFocus {
    Name,
    Source,
    Image,
    Pack,
    Network,
    Dns,
}

#[derive(Debug, Clone)]
pub struct CreateForm {
    pub name: String,
    pub source: CreateSourceKind,
    pub image: String,
    pub packs: Vec<PackRef>,
    pub pack_index: usize,
    pub packs_dir: PathBuf,
    pub network: bool,
    pub dns: String,
    pub focus: CreateFocus,
    pub error: Option<String>,
}

pub struct App {
    pub machines: Vec<Machine>,
    pub selected: usize,
    pub mode: Mode,
    pub status: String,
    /// `true` when `status` is a failure. Success, progress, and the machine
    /// count are distinguished by [`App::status_tone`].
    pub status_error: bool,
    pub live: Option<LiveStats>,
    pub stats_message: String,
    pub config: Config,
    pub config_path: PathBuf,
    pub config_writable: bool,
    config_warning: Option<String>,
    list_inflight: bool,
    list_again: bool,
    stats_inflight: bool,
    action_inflight: bool,
    last_poll: Option<Instant>,
    status_from_list: bool,
    live_for: Option<String>,
}

impl App {
    pub fn new(config: Config, config_path: PathBuf, config_writable: bool) -> Self {
        Self {
            machines: Vec::new(),
            selected: 0,
            mode: Mode::Normal,
            status: "loading…".into(),
            status_error: false,
            live: None,
            stats_message: "live cpu and memory are sampled while running".into(),
            config,
            config_path,
            config_writable,
            config_warning: None,
            list_inflight: false,
            list_again: false,
            stats_inflight: false,
            action_inflight: false,
            last_poll: None,
            status_from_list: true,
            live_for: None,
        }
    }

    pub fn set_startup_error(&mut self, message: impl Into<String>) {
        self.set_status(message, true, false);
    }

    pub fn note_shell(&mut self, code: i32) {
        if code == 0 {
            self.set_status("shell closed", false, false);
        } else {
            self.set_status(format!("shell exited {code}"), true, false);
        }
    }

    pub fn request_refresh(&mut self) -> Vec<Effect> {
        self.force_list()
    }

    pub fn note_config_save(&mut self, result: Result<(), String>) {
        self.config_warning = result.err();
    }

    pub fn is_busy(&self) -> bool {
        self.list_inflight || self.stats_inflight || self.action_inflight
    }

    pub fn status_tone(&self) -> StatusTone {
        if self.status_error {
            return StatusTone::Error;
        }
        if self.status == "busy" || self.status.ends_with('…') {
            return StatusTone::Busy;
        }
        if self.status_from_list {
            return StatusTone::Info;
        }
        StatusTone::Ok
    }

    pub fn selected_machine(&self) -> Option<&Machine> {
        self.machines.get(self.selected)
    }

    pub fn on_tick(&mut self, now: Instant) -> Vec<Effect> {
        if self.action_inflight || self.list_inflight {
            return Vec::new();
        }
        if !matches!(self.mode, Mode::Normal | Mode::Help) {
            return Vec::new();
        }
        let due = match self.last_poll {
            Some(then) => now.saturating_duration_since(then) >= POLL_INTERVAL,
            None => true,
        };
        if !due {
            return Vec::new();
        }
        self.last_poll = Some(now);
        self.enqueue_list()
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.kind == KeyEventKind::Release {
            return Vec::new();
        }
        if is_ctrl_c(&key) {
            return vec![Effect::Quit];
        }
        match self.mode {
            Mode::Help => self.on_help_key(&key),
            Mode::Normal => self.on_normal_key(&key),
            Mode::Delete(_) => self.on_delete_key(&key),
            Mode::Create(_) => self.on_create_key(&key),
            Mode::Throwaway(_) => self.on_throwaway_key(&key),
            Mode::Pack(_) => self.on_pack_key(&key),
            Mode::Copy(_) => self.on_copy_key(&key),
        }
    }

    pub fn apply(&mut self, outcome: Outcome) -> Vec<Effect> {
        let mut effects = match outcome {
            Outcome::List(result) => {
                self.list_inflight = false;
                match result {
                    Ok(machines) => {
                        self.install_machines(machines);
                        if self.status_from_list {
                            let count = self.machines.len();
                            self.set_status(format!("{count} machine(s)"), false, true);
                        }
                        self.kick_stats()
                    }
                    Err(error) => {
                        self.set_status(error, true, true);
                        Vec::new()
                    }
                }
            }
            Outcome::Stats { name, result } => {
                self.stats_inflight = false;
                let current = self.selected_machine().map(|machine| machine.name.clone());
                if current.as_deref() != Some(name.as_str()) {
                    self.kick_stats()
                } else if !self.selected_is_running() {
                    self.live = None;
                    self.live_for = None;
                    self.stats_message = self.idle_stats_message();
                    Vec::new()
                } else {
                    match result {
                        Ok(stats) => {
                            self.stats_message.clear();
                            self.live = Some(stats);
                            self.live_for = Some(name);
                        }
                        Err(error) => {
                            self.live = None;
                            self.live_for = None;
                            self.stats_message = format!("stats unavailable: {error}");
                        }
                    }
                    Vec::new()
                }
            }
            Outcome::Action { id, result } => {
                self.action_inflight = false;
                match result {
                    Ok(()) => self.action_ok(id),
                    Err(error) => {
                        self.set_status(error, true, false);
                        self.force_list()
                    }
                }
            }
        };
        if self.list_again {
            self.list_again = false;
            effects.extend(self.force_list());
        }
        effects
    }

    fn on_help_key(&mut self, key: &KeyEvent) -> Vec<Effect> {
        if command_letter(key) == Some('q') {
            return vec![Effect::Quit];
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => self.mode = Mode::Normal,
            _ => {}
        }
        Vec::new()
    }

    fn on_normal_key(&mut self, key: &KeyEvent) -> Vec<Effect> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k' | 'K') => return self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j' | 'J') => return self.move_selection(1),
            _ => {}
        }
        if key.kind == KeyEventKind::Repeat {
            return Vec::new();
        }
        let Some(letter) = command_letter(key) else {
            if key.code == KeyCode::Enter {
                return self.open_shell();
            }
            if key.code == KeyCode::Char('?') || key.code == KeyCode::F(1) {
                self.mode = Mode::Help;
            }
            return Vec::new();
        };
        match letter {
            'q' => vec![Effect::Quit],
            '?' => {
                self.mode = Mode::Help;
                Vec::new()
            }
            'r' => {
                if self.action_inflight {
                    self.set_status("busy", false, false);
                    Vec::new()
                } else {
                    self.force_list()
                }
            }
            's' => self.start_selected(),
            'x' => self.stop_selected(),
            'd' => self.open_delete(),
            'c' => self.open_create(),
            't' => self.open_throwaway(),
            'p' => self.open_pack(),
            'y' => self.open_copy(),
            _ => Vec::new(),
        }
    }

    fn on_delete_key(&mut self, key: &KeyEvent) -> Vec<Effect> {
        let letter = command_letter(key);
        if key.code == KeyCode::Esc || letter == Some('n') {
            self.mode = Mode::Normal;
            return Vec::new();
        }
        if letter != Some('y') || key.kind == KeyEventKind::Repeat {
            return Vec::new();
        }
        let Mode::Delete(prompt) = &self.mode else {
            return Vec::new();
        };
        let name = prompt.name.clone();
        match commands::delete_args(&name) {
            Ok(args) => self.begin_action(
                ActionId::Delete { name: name.clone() },
                args,
                format!("deleting {name}…"),
            ),
            Err(error) => {
                self.mode = Mode::Normal;
                self.set_status(error, true, false);
                Vec::new()
            }
        }
    }

    fn on_create_key(&mut self, key: &KeyEvent) -> Vec<Effect> {
        let op = {
            let Mode::Create(form) = &mut self.mode else {
                return Vec::new();
            };
            edit_create(form, key)
        };
        match op {
            CreateOp::Stay => Vec::new(),
            CreateOp::Cancel => {
                self.mode = Mode::Normal;
                Vec::new()
            }
            CreateOp::Submit(request) => self.submit_create(request, true),
        }
    }

    fn on_throwaway_key(&mut self, key: &KeyEvent) -> Vec<Effect> {
        if key.code == KeyCode::Esc {
            self.mode = Mode::Normal;
            return Vec::new();
        }
        if matches!(key.code, KeyCode::Up | KeyCode::Char('k' | 'K')) {
            if let Mode::Throwaway(prompt) = &mut self.mode {
                prompt.index = prompt.index.saturating_sub(1);
            }
            return Vec::new();
        }
        if matches!(key.code, KeyCode::Down | KeyCode::Char('j' | 'J')) {
            if let Mode::Throwaway(prompt) = &mut self.mode {
                if !prompt.packs.is_empty() {
                    prompt.index = (prompt.index + 1).min(prompt.packs.len() - 1);
                }
            }
            return Vec::new();
        }
        if key.code != KeyCode::Enter || key.kind == KeyEventKind::Repeat {
            return Vec::new();
        }
        let (path, dir_display) = match &self.mode {
            Mode::Throwaway(prompt) => {
                let dir_display = prompt.dir.display().to_string();
                let path = prompt.packs.get(prompt.index).map(|pack| pack.path.clone());
                (path, dir_display)
            }
            _ => return Vec::new(),
        };
        let Some(path) = path else {
            self.set_status(format!("no packs in {dir_display}"), true, false);
            return Vec::new();
        };
        let existing: Vec<&str> = self
            .machines
            .iter()
            .map(|machine| machine.name.as_str())
            .collect();
        let name = match names::next_throwaway_name(existing) {
            Ok(name) => name,
            Err(error) => {
                self.set_status(error, true, false);
                return Vec::new();
            }
        };
        let request = CreateRequest {
            name,
            source: CreateSource::Pack(path),
            network: self.config.network,
            dns: self.config.dns.clone(),
        };
        self.submit_create(request, false)
    }

    fn on_pack_key(&mut self, key: &KeyEvent) -> Vec<Effect> {
        let letter = command_letter(key);
        if key.code == KeyCode::Esc || letter == Some('n') {
            self.mode = Mode::Normal;
            return Vec::new();
        }
        if letter != Some('y') || key.kind == KeyEventKind::Repeat {
            return Vec::new();
        }
        let Mode::Pack(prompt) = &self.mode else {
            return Vec::new();
        };
        let name = prompt.name.clone();
        let stub = prompt.stub.clone();
        self.start_pack(name, stub, None)
    }

    fn on_copy_key(&mut self, key: &KeyEvent) -> Vec<Effect> {
        if key.code == KeyCode::Esc {
            self.mode = Mode::Normal;
            return Vec::new();
        }
        if key.code == KeyCode::Enter && key.kind != KeyEventKind::Repeat {
            let Mode::Copy(form) = &self.mode else {
                return Vec::new();
            };
            let source = form.source.clone();
            let dest = form.name.clone();
            return self.submit_copy(source, dest);
        }
        if let Mode::Copy(form) = &mut self.mode {
            if edit_field(&mut form.name, key, name_char, names::MAX_NAME_LEN) {
                form.error = None;
            }
        }
        Vec::new()
    }

    fn move_selection(&mut self, delta: isize) -> Vec<Effect> {
        if self.machines.is_empty() {
            return Vec::new();
        }
        let len = self.machines.len() as isize;
        let next = (self.selected as isize + delta).clamp(0, len - 1) as usize;
        if next == self.selected {
            return Vec::new();
        }
        self.selected = next;
        self.live = None;
        self.live_for = None;
        self.stats_message = if self.selected_is_running() {
            "sampling…".into()
        } else {
            self.idle_stats_message()
        };
        self.kick_stats()
    }

    fn start_selected(&mut self) -> Vec<Effect> {
        let Some(name) = self.selected_valid_name() else {
            return Vec::new();
        };
        let state = self
            .selected_machine()
            .map(|machine| machine.state.as_str());
        match state {
            Some("running") => {
                self.set_status(format!("{name} is already running"), true, false);
                Vec::new()
            }
            Some("paused" | "pausing") => {
                self.set_status(
                    format!("{name} is paused; smoltop does not resume it"),
                    true,
                    false,
                );
                Vec::new()
            }
            Some("frozen") => {
                self.set_status(format!("{name} is a frozen branch source"), true, false);
                Vec::new()
            }
            _ => match commands::start_args(&name) {
                Ok(args) => self.begin_action(
                    ActionId::Start { name: name.clone() },
                    args,
                    format!("starting {name}…"),
                ),
                Err(error) => {
                    self.set_status(error, true, false);
                    Vec::new()
                }
            },
        }
    }

    fn stop_selected(&mut self) -> Vec<Effect> {
        let Some(name) = self.selected_valid_name() else {
            return Vec::new();
        };
        let state = self
            .selected_machine()
            .map(|machine| machine.state.as_str())
            .unwrap_or("");
        if !matches!(state, "running" | "pausing" | "unreachable" | "frozen") {
            self.set_status(format!("{name} is {state}"), true, false);
            return Vec::new();
        }
        match commands::stop_args(&name) {
            Ok(args) => self.begin_action(
                ActionId::Stop { name: name.clone() },
                args,
                format!("stopping {name}…"),
            ),
            Err(error) => {
                self.set_status(error, true, false);
                Vec::new()
            }
        }
    }

    fn open_shell(&mut self) -> Vec<Effect> {
        let Some(name) = self.selected_valid_name() else {
            return Vec::new();
        };
        if !self.selected_is_running() {
            let state = self
                .selected_machine()
                .map(|machine| machine.state.as_str())
                .unwrap_or("missing");
            self.set_status(
                format!("start {name} before opening a shell ({state})"),
                true,
                false,
            );
            return Vec::new();
        }
        if self.action_inflight {
            self.set_status("busy", false, false);
            return Vec::new();
        }
        match commands::shell_args(&name) {
            Ok(args) => vec![Effect::Shell(args)],
            Err(error) => {
                self.set_status(error, true, false);
                Vec::new()
            }
        }
    }

    fn open_delete(&mut self) -> Vec<Effect> {
        let Some(name) = self.selected_valid_name() else {
            return Vec::new();
        };
        self.mode = Mode::Delete(DeletePrompt { name });
        Vec::new()
    }

    fn open_create(&mut self) -> Vec<Effect> {
        let Ok(dir) = self.packs_dir_or_status() else {
            return Vec::new();
        };
        let packs = match packs::list_packs(&dir) {
            Ok(packs) => packs,
            Err(error) => {
                self.set_status(error, true, false);
                return Vec::new();
            }
        };
        self.mode = Mode::Create(Box::new(CreateForm {
            name: String::new(),
            source: CreateSourceKind::Image,
            image: String::new(),
            packs,
            pack_index: 0,
            packs_dir: dir,
            network: self.config.network,
            dns: self.config.dns.clone(),
            focus: CreateFocus::Name,
            error: None,
        }));
        Vec::new()
    }

    fn open_throwaway(&mut self) -> Vec<Effect> {
        let Ok(dir) = self.packs_dir_or_status() else {
            return Vec::new();
        };
        let packs = match packs::list_packs(&dir) {
            Ok(packs) => packs,
            Err(error) => {
                self.set_status(error, true, false);
                return Vec::new();
            }
        };
        if packs.is_empty() {
            self.set_status(format!("no packs in {}", dir.display()), true, false);
            return Vec::new();
        }
        self.mode = Mode::Throwaway(Box::new(ThrowawayPrompt {
            packs,
            index: 0,
            dir,
        }));
        Vec::new()
    }

    fn open_pack(&mut self) -> Vec<Effect> {
        let Some(name) = self.require_stopped("pack") else {
            return Vec::new();
        };
        let Ok(dir) = self.packs_dir_or_status() else {
            return Vec::new();
        };
        let stub = dir.join(&name);
        self.mode = Mode::Pack(PackPrompt { name, stub });
        Vec::new()
    }

    fn open_copy(&mut self) -> Vec<Effect> {
        let Some(source) = self.require_stopped("copy") else {
            return Vec::new();
        };
        let existing: Vec<&str> = self
            .machines
            .iter()
            .map(|machine| machine.name.as_str())
            .collect();
        let name = match names::next_copy_name(&source, existing) {
            Ok(name) => name,
            Err(error) => {
                self.set_status(error, true, false);
                return Vec::new();
            }
        };
        self.mode = Mode::Copy(Box::new(CopyForm {
            source,
            name,
            error: None,
        }));
        Vec::new()
    }

    fn submit_create(&mut self, request: CreateRequest, save: bool) -> Vec<Effect> {
        if self.action_inflight {
            self.set_status("busy", false, false);
            return Vec::new();
        }
        if self
            .machines
            .iter()
            .any(|machine| machine.name == request.name)
        {
            self.form_error("a machine with that name already exists".into());
            return Vec::new();
        }
        let args = match commands::create_args(&request) {
            Ok(args) => args,
            Err(error) => {
                self.form_error(error);
                return Vec::new();
            }
        };
        let save_effect = if save {
            self.config.network = request.network;
            self.config.dns.clone_from(&request.dns);
            self.config_writable
        } else {
            false
        };
        let name = request.name.clone();
        let mut effects = self.begin_action(
            ActionId::Create { name: name.clone() },
            args,
            format!("creating {name}…"),
        );
        if save_effect {
            effects.insert(0, Effect::SaveConfig);
        }
        effects
    }

    fn submit_copy(&mut self, source: String, dest: String) -> Vec<Effect> {
        if let Err(error) = validate_name(&dest) {
            self.form_error(error);
            return Vec::new();
        }
        if dest == source || self.machines.iter().any(|machine| machine.name == dest) {
            self.form_error("pick a new machine name".into());
            return Vec::new();
        }
        let Ok(dir) = self.packs_dir_or_status() else {
            return Vec::new();
        };
        let stub = dir.join(&source);
        self.start_pack(source, stub, Some(dest))
    }

    fn start_pack(
        &mut self,
        name: String,
        stub: PathBuf,
        copy_dest: Option<String>,
    ) -> Vec<Effect> {
        if self.action_inflight {
            self.set_status("busy", false, false);
            return Vec::new();
        }
        if let Some(parent) = stub.parent() {
            if let Err(error) = packs::ensure_dir(parent) {
                self.mode = Mode::Normal;
                self.set_status(error, true, false);
                return Vec::new();
            }
        }
        let args = match commands::pack_args(&name, &stub) {
            Ok(args) => args,
            Err(error) => {
                self.form_error(error);
                return Vec::new();
            }
        };
        let sidecar = commands::sidecar_path(&stub);
        match copy_dest {
            Some(dest) => self.begin_action(
                ActionId::PackForCopy { dest, sidecar },
                args,
                format!("packing {name}…"),
            ),
            None => self.begin_action(
                ActionId::Pack { name: name.clone() },
                args,
                format!("packing {name}…"),
            ),
        }
    }

    fn action_ok(&mut self, id: ActionId) -> Vec<Effect> {
        match id {
            ActionId::PackForCopy { dest, sidecar } => {
                if !sidecar.is_file() {
                    self.set_status(
                        format!("pack did not produce {}", sidecar.display()),
                        true,
                        false,
                    );
                    return self.force_list();
                }
                let request = CreateRequest {
                    name: dest.clone(),
                    source: CreateSource::Pack(sidecar),
                    network: self.config.network,
                    dns: self.config.dns.clone(),
                };
                match commands::create_args(&request) {
                    Ok(args) => self.begin_action(
                        ActionId::CreateCopy { name: dest.clone() },
                        args,
                        format!("creating {dest} from pack…"),
                    ),
                    Err(error) => {
                        self.set_status(error, true, false);
                        self.force_list()
                    }
                }
            }
            ActionId::Start { name } => self.finish(format!("started {name}")),
            ActionId::Stop { name } => self.finish(format!("stopped {name}")),
            ActionId::Delete { name } => self.finish(format!("deleted {name}")),
            ActionId::Create { name } | ActionId::CreateCopy { name } => {
                self.finish(format!("created {name}"))
            }
            ActionId::Pack { name } => self.finish(format!("packed {name}")),
        }
    }

    fn finish(&mut self, message: String) -> Vec<Effect> {
        let message = match self.config_warning.take() {
            Some(warning) => format!("{message} (config: {warning})"),
            None => message,
        };
        self.set_status(message, false, false);
        self.force_list()
    }

    fn begin_action(&mut self, id: ActionId, args: Vec<String>, status: String) -> Vec<Effect> {
        if self.action_inflight {
            self.set_status("busy", false, false);
            return Vec::new();
        }
        self.action_inflight = true;
        self.mode = Mode::Normal;
        self.set_status(status, false, false);
        vec![Effect::Job(Job::Action { id, args })]
    }

    fn force_list(&mut self) -> Vec<Effect> {
        self.last_poll = Some(Instant::now());
        self.enqueue_list()
    }

    fn enqueue_list(&mut self) -> Vec<Effect> {
        if self.list_inflight {
            self.list_again = true;
            return Vec::new();
        }
        self.list_inflight = true;
        vec![Effect::Job(Job::List)]
    }

    fn kick_stats(&mut self) -> Vec<Effect> {
        if self.stats_inflight || self.action_inflight {
            return Vec::new();
        }
        let Some(machine) = self.selected_machine() else {
            self.live = None;
            self.stats_message = "no machines".into();
            return Vec::new();
        };
        if machine.state != "running" {
            self.live = None;
            self.live_for = None;
            self.stats_message = self.idle_stats_message();
            return Vec::new();
        }
        let name = machine.name.clone();
        let args = match commands::stats_args(&name) {
            Ok(args) => args,
            Err(error) => {
                self.stats_message = format!("stats unavailable: {error}");
                return Vec::new();
            }
        };
        self.stats_inflight = true;
        vec![Effect::Job(Job::Stats { name, args })]
    }

    fn install_machines(&mut self, machines: Vec<Machine>) {
        let previous = self.selected_machine().map(|machine| machine.name.clone());
        self.machines = machines;
        if let Some(name) = previous {
            if let Some(index) = self
                .machines
                .iter()
                .position(|machine| machine.name == name)
            {
                self.selected = index;
            } else if self.machines.is_empty() {
                self.selected = 0;
            } else {
                self.selected = self.selected.min(self.machines.len() - 1);
            }
        }
        if self.machines.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.machines.len() {
            self.selected = self.machines.len() - 1;
        }
        if self.live_for.as_deref() != self.selected_machine().map(|machine| machine.name.as_str())
        {
            self.live = None;
            self.live_for = None;
        }
        if !self.selected_is_running() {
            self.live = None;
            self.live_for = None;
            self.stats_message = self.idle_stats_message();
        }
    }

    fn selected_is_running(&self) -> bool {
        self.selected_machine()
            .map(|machine| machine.state == "running")
            .unwrap_or(false)
    }

    fn idle_stats_message(&self) -> String {
        match self.selected_machine() {
            Some(machine) => format!(
                "live cpu and memory are sampled while running ({})",
                machine.state
            ),
            None => "no machines".into(),
        }
    }

    fn selected_valid_name(&mut self) -> Option<String> {
        let Some(machine) = self.selected_machine() else {
            self.set_status("no machine selected", true, false);
            return None;
        };
        let name = machine.name.clone();
        if let Err(error) = validate_name(&name) {
            self.set_status(format!("refusing name '{name}': {error}"), true, false);
            return None;
        }
        Some(name)
    }

    fn require_stopped(&mut self, action: &str) -> Option<String> {
        let name = self.selected_valid_name()?;
        let state = self
            .selected_machine()
            .map(|machine| machine.state.clone())
            .unwrap_or_else(|| "missing".into());
        if state != "stopped" {
            self.set_status(
                format!("{action} needs a stopped machine ({name} is {state})"),
                true,
                false,
            );
            return None;
        }
        Some(name)
    }

    fn packs_dir_or_status(&mut self) -> Result<PathBuf, ()> {
        match self.config.packs_path() {
            Ok(dir) => Ok(dir),
            Err(error) => {
                self.set_status(error, true, false);
                Err(())
            }
        }
    }

    fn form_error(&mut self, error: String) {
        match &mut self.mode {
            Mode::Create(form) => form.error = Some(error),
            Mode::Copy(form) => form.error = Some(error),
            _ => self.set_status(error, true, false),
        }
    }

    fn set_status(&mut self, text: impl Into<String>, error: bool, from_list: bool) {
        self.status = text.into();
        self.status_error = error;
        self.status_from_list = from_list;
    }
}

enum CreateOp {
    Stay,
    Cancel,
    Submit(CreateRequest),
}

fn edit_create(form: &mut CreateForm, key: &KeyEvent) -> CreateOp {
    if key.code == KeyCode::Esc {
        return CreateOp::Cancel;
    }
    if key.code == KeyCode::Enter && key.kind != KeyEventKind::Repeat {
        return match request_from_form(form) {
            Ok(request) => CreateOp::Submit(request),
            Err(error) => {
                form.error = Some(error);
                CreateOp::Stay
            }
        };
    }
    if key.code == KeyCode::Tab {
        form.focus = next_focus(form.focus, form.source);
        form.error = None;
        return CreateOp::Stay;
    }
    if key.code == KeyCode::BackTab {
        form.focus = prev_focus(form.focus, form.source);
        form.error = None;
        return CreateOp::Stay;
    }
    match form.focus {
        CreateFocus::Name => {
            if edit_field(&mut form.name, key, name_char, names::MAX_NAME_LEN) {
                form.error = None;
            }
        }
        CreateFocus::Image => {
            if edit_field(&mut form.image, key, image_char, 512) {
                form.error = None;
            }
        }
        CreateFocus::Dns => {
            if edit_field(&mut form.dns, key, dns_char, 15) {
                form.error = None;
            }
        }
        CreateFocus::Source => match key.code {
            KeyCode::Left | KeyCode::Char('h' | 'H') => set_source(form, CreateSourceKind::Image),
            KeyCode::Right | KeyCode::Char('l' | 'L') => set_source(form, CreateSourceKind::Pack),
            KeyCode::Char(' ') => {
                let next = match form.source {
                    CreateSourceKind::Image => CreateSourceKind::Pack,
                    CreateSourceKind::Pack => CreateSourceKind::Image,
                };
                set_source(form, next);
            }
            _ => {}
        },
        CreateFocus::Network => match key.code {
            KeyCode::Left | KeyCode::Char('h' | 'H') => form.network = false,
            KeyCode::Right | KeyCode::Char('l' | 'L') => form.network = true,
            KeyCode::Char(' ') => form.network = !form.network,
            _ => {}
        },
        CreateFocus::Pack => match key.code {
            KeyCode::Up | KeyCode::Char('k' | 'K') => {
                form.pack_index = form.pack_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j' | 'J') if !form.packs.is_empty() => {
                form.pack_index = (form.pack_index + 1).min(form.packs.len() - 1);
            }
            _ => {}
        },
    }
    CreateOp::Stay
}

fn set_source(form: &mut CreateForm, source: CreateSourceKind) {
    form.source = source;
    form.focus = match (form.focus, source) {
        (CreateFocus::Image, CreateSourceKind::Pack) => CreateFocus::Pack,
        (CreateFocus::Pack, CreateSourceKind::Image) => CreateFocus::Image,
        (focus, _) => focus,
    };
    form.error = None;
}

fn request_from_form(form: &CreateForm) -> Result<CreateRequest, String> {
    let source = match form.source {
        CreateSourceKind::Image => CreateSource::Image(form.image.clone()),
        CreateSourceKind::Pack => {
            let pack = form
                .packs
                .get(form.pack_index)
                .ok_or_else(|| format!("no packs in {}", form.packs_dir.display()))?;
            CreateSource::Pack(pack.path.clone())
        }
    };
    Ok(CreateRequest {
        name: form.name.clone(),
        source,
        network: form.network,
        dns: form.dns.clone(),
    })
}

fn next_focus(focus: CreateFocus, source: CreateSourceKind) -> CreateFocus {
    match (focus, source) {
        (CreateFocus::Name, _) => CreateFocus::Source,
        (CreateFocus::Source, CreateSourceKind::Image) => CreateFocus::Image,
        (CreateFocus::Source, CreateSourceKind::Pack) => CreateFocus::Pack,
        (CreateFocus::Image | CreateFocus::Pack, _) => CreateFocus::Network,
        (CreateFocus::Network, _) => CreateFocus::Dns,
        (CreateFocus::Dns, _) => CreateFocus::Name,
    }
}

fn prev_focus(focus: CreateFocus, source: CreateSourceKind) -> CreateFocus {
    match (focus, source) {
        (CreateFocus::Name, CreateSourceKind::Image) => CreateFocus::Dns,
        (CreateFocus::Name, CreateSourceKind::Pack) => CreateFocus::Dns,
        (CreateFocus::Source, _) => CreateFocus::Name,
        (CreateFocus::Image | CreateFocus::Pack, _) => CreateFocus::Source,
        (CreateFocus::Network, CreateSourceKind::Image) => CreateFocus::Image,
        (CreateFocus::Network, CreateSourceKind::Pack) => CreateFocus::Pack,
        (CreateFocus::Dns, _) => CreateFocus::Network,
    }
}

fn edit_field(buffer: &mut String, key: &KeyEvent, allow: fn(char) -> bool, max: usize) -> bool {
    match key.code {
        KeyCode::Backspace => {
            buffer.pop();
            true
        }
        KeyCode::Char(ch) if allow(ch) && buffer.len() < max => {
            if buffer.is_empty() && ch == '-' {
                return false;
            }
            buffer.push(ch);
            true
        }
        _ => false,
    }
}

fn name_char(ch: char) -> bool {
    matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '-')
}

fn image_char(ch: char) -> bool {
    matches!(
        ch,
        'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '-' | '/' | ':' | '@'
    )
}

fn dns_char(ch: char) -> bool {
    ch.is_ascii_digit() || ch == '.'
}

fn is_ctrl_c(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('c' | 'C')) && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn command_letter(key: &KeyEvent) -> Option<char> {
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return None;
    }
    match key.code {
        KeyCode::Char(ch) => Some(ch.to_ascii_lowercase()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use std::fs;

    fn key(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)
    }

    fn code(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn machine(name: &str, state: &str) -> Machine {
        Machine {
            name: name.into(),
            state: state.into(),
            cpus: 2,
            memory_mib: 1024,
            image: Some("alpine".into()),
            parent: None,
        }
    }

    fn fixture(label: &str) -> (App, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "smoltop-app-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let config = Config {
            packs_dir: dir.to_str().unwrap().to_string(),
            ..Config::default()
        };
        let app = App::new(config, dir.join("config.toml"), true);
        (app, dir)
    }

    fn action_args(effects: &[Effect]) -> &[String] {
        effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Job(Job::Action { args, .. }) => Some(args.as_slice()),
                _ => None,
            })
            .unwrap_or(&[])
    }

    #[test]
    fn tick_lists_once_until_the_interval_passes() {
        let (mut app, dir) = fixture("tick");
        let now = Instant::now();
        assert!(matches!(
            app.on_tick(now).as_slice(),
            [Effect::Job(Job::List)]
        ));
        assert!(app.on_tick(now).is_empty());
        app.apply(Outcome::List(Ok(vec![machine("web", "stopped")])));
        assert!(app.on_tick(now).is_empty());
        let later = now + POLL_INTERVAL + Duration::from_millis(1);
        assert!(matches!(
            app.on_tick(later).as_slice(),
            [Effect::Job(Job::List)]
        ));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn running_machine_kicks_stats_and_errors_degrade() {
        let (mut app, dir) = fixture("stats");
        let effects = app.apply(Outcome::List(Ok(vec![machine("web", "running")])));
        match effects.as_slice() {
            [Effect::Job(Job::Stats { name, args })] => {
                assert_eq!(name, "web");
                assert_eq!(args, &commands::stats_args("web").unwrap());
            }
            other => panic!("expected stats, got {other:?}"),
        }
        app.apply(Outcome::Stats {
            name: "web".into(),
            result: Err("unrecognized stats output".into()),
        });
        assert!(app.live.is_none());
        assert!(app.stats_message.contains("stats unavailable"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn delete_waits_for_y_and_passes_force() {
        let (mut app, dir) = fixture("delete");
        app.machines = vec![machine("web", "running")];
        assert!(app.on_key(key('d')).is_empty());
        assert!(matches!(app.mode, Mode::Delete(_)));
        assert!(app.on_key(key('n')).is_empty());
        assert!(matches!(app.mode, Mode::Normal));
        assert!(!app.action_inflight);

        app.on_key(key('d'));
        let effects = app.on_key(key('y'));
        assert_eq!(action_args(&effects), commands::delete_args("web").unwrap());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn start_stop_and_shell_follow_state_and_reject_bad_names() {
        let (mut app, dir) = fixture("life");
        app.machines = vec![machine("web", "stopped")];
        assert_eq!(
            action_args(&app.on_key(key('s'))),
            commands::start_args("web").unwrap()
        );
        app.action_inflight = false;

        app.machines[0].state = "running".into();
        assert!(app.on_key(key('s')).is_empty());
        assert!(app.status.contains("already running"));

        let stop = app.on_key(key('x'));
        assert_eq!(action_args(&stop), commands::stop_args("web").unwrap());
        app.action_inflight = false;

        let shell = app.on_key(code(KeyCode::Enter));
        match shell.as_slice() {
            [Effect::Shell(args)] => {
                assert_eq!(args, &commands::shell_args("web").unwrap());
                assert!(!args.iter().any(|arg| arg == "-n"));
            }
            other => panic!("expected shell, got {other:?}"),
        }

        app.machines[0].state = "stopped".into();
        assert!(app.on_key(code(KeyCode::Enter)).is_empty());
        assert!(app.status.contains("start web"));

        app.machines[0].name = "-nope".into();
        assert!(app.on_key(key('s')).is_empty());
        assert!(app.status.contains("refusing name"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn create_form_builds_image_args_and_saves_config() {
        let (mut app, dir) = fixture("create");
        app.on_key(key('c'));
        for ch in "web".chars() {
            app.on_key(key(ch));
        }
        app.on_key(code(KeyCode::Tab));
        app.on_key(code(KeyCode::Tab));
        for ch in "ghcr.io/org/app:1".chars() {
            app.on_key(key(ch));
        }
        let effects = app.on_key(code(KeyCode::Enter));
        assert!(effects
            .iter()
            .any(|effect| matches!(effect, Effect::SaveConfig)));
        assert_eq!(
            action_args(&effects),
            commands::create_args(&CreateRequest {
                name: "web".into(),
                source: CreateSource::Image("ghcr.io/org/app:1".into()),
                network: true,
                dns: "100.64.0.7".into(),
            })
            .unwrap()
        );
        assert_eq!(app.config.dns, "100.64.0.7");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn create_rejects_a_bad_image_without_a_job() {
        let (mut app, dir) = fixture("badimage");
        app.on_key(key('c'));
        app.on_key(key('a'));
        app.on_key(code(KeyCode::Tab));
        app.on_key(code(KeyCode::Tab));
        app.on_key(key(' '));
        let effects = app.on_key(code(KeyCode::Enter));
        assert!(effects.is_empty());
        assert!(matches!(app.mode, Mode::Create(_)));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn throwaway_uses_tmp_n_and_the_selected_pack() {
        let (mut app, dir) = fixture("throw");
        fs::write(dir.join("base.smolmachine"), b"pack").unwrap();
        fs::write(dir.join("-bad.smolmachine"), b"pack").unwrap();
        app.machines = vec![machine("tmp-1", "stopped")];
        app.on_key(key('t'));
        let effects = app.on_key(code(KeyCode::Enter));
        let args = action_args(&effects);
        assert_eq!(
            args,
            commands::create_args(&CreateRequest {
                name: "tmp-2".into(),
                source: CreateSource::Pack(dir.join("base.smolmachine")),
                network: true,
                dns: "100.64.0.7".into(),
            })
            .unwrap()
        );
        assert!(!effects
            .iter()
            .any(|effect| matches!(effect, Effect::SaveConfig)));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn copy_packs_then_creates_with_configured_net_and_dns() {
        let (mut app, dir) = fixture("copy");
        app.config.network = true;
        app.config.dns = "100.64.0.7".into();
        app.machines = vec![machine("web", "stopped")];
        assert!(app.on_key(key('y')).is_empty());
        assert!(matches!(app.mode, Mode::Copy(_)));
        let effects = app.on_key(code(KeyCode::Enter));
        assert_eq!(
            action_args(&effects),
            commands::pack_args("web", &dir.join("web")).unwrap()
        );
        assert!(dir.join("web.smolmachine").parent().unwrap().is_dir());

        fs::write(dir.join("web.smolmachine"), b"pack").unwrap();
        let id = ActionId::PackForCopy {
            dest: "web-copy".into(),
            sidecar: dir.join("web.smolmachine"),
        };
        app.action_inflight = true;
        let effects = app.apply(Outcome::Action { id, result: Ok(()) });
        let args = action_args(&effects);
        let pack = dir.join("web.smolmachine");
        assert_eq!(
            args,
            &[
                "--error-format",
                "json",
                "machine",
                "create",
                "--name",
                "web-copy",
                "--from",
                pack.to_str().unwrap(),
                "--net",
                "--dns",
                "100.64.0.7",
            ]
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn status_tone_separates_progress_success_and_errors() {
        let (mut app, dir) = fixture("tone");
        assert_eq!(app.status_tone(), StatusTone::Busy);
        app.set_status("2 machine(s)", false, true);
        assert_eq!(app.status_tone(), StatusTone::Info);
        app.set_status("started web", false, false);
        assert_eq!(app.status_tone(), StatusTone::Ok);
        app.set_status("starting web…", false, false);
        assert_eq!(app.status_tone(), StatusTone::Busy);
        app.set_status("nope", true, false);
        assert_eq!(app.status_tone(), StatusTone::Error);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn pack_and_copy_require_stopped() {
        let (mut app, dir) = fixture("stopped");
        app.machines = vec![machine("web", "running")];
        assert!(app.on_key(key('p')).is_empty());
        assert!(app.status.contains("stopped"));
        assert!(app.on_key(key('y')).is_empty());
        assert!(matches!(app.mode, Mode::Normal));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn help_and_quit_and_release_keys() {
        let (mut app, dir) = fixture("keys");
        app.on_key(key('?'));
        assert!(matches!(app.mode, Mode::Help));
        app.on_key(code(KeyCode::Esc));
        assert!(matches!(app.mode, Mode::Normal));
        app.on_key(key('?'));
        assert!(matches!(app.on_key(key('q')).as_slice(), [Effect::Quit]));
        let mut release = key('q');
        release.kind = KeyEventKind::Release;
        assert!(app.on_key(release).is_empty());
        let ctrl = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(matches!(app.on_key(ctrl).as_slice(), [Effect::Quit]));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn polling_pauses_while_a_prompt_is_open() {
        let (mut app, dir) = fixture("pause");
        app.machines = vec![machine("web", "stopped")];
        app.on_key(key('d'));
        assert!(app.on_tick(Instant::now()).is_empty());
        let _ = fs::remove_dir_all(dir);
    }
}
