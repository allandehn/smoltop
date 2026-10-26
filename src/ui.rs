//! Drawing. Named ANSI colors, bold, dim, and reverse only — no RGB — so the
//! terminal theme (Omarchy included) supplies the actual palette.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use crate::app::{
    App, CopyForm, CreateFocus, CreateForm, CreateSourceKind, DeletePrompt, Mode, PackPrompt,
    ThrowawayPrompt,
};
use crate::VERSION;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width < 24 || area.height < 8 {
        frame.render_widget(Paragraph::new("terminal too small"), area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),
            Constraint::Length(5),
            Constraint::Length(3),
        ])
        .split(area);

    draw_table(frame, app, chunks[0]);
    draw_detail(frame, app, chunks[1]);
    draw_footer(frame, app, chunks[2]);
    draw_modal(frame, app, area);
}

fn draw_table(frame: &mut Frame, app: &App, area: Rect) {
    let body_rows = area.height.saturating_sub(3).max(1) as usize;
    let start = if app.machines.len() <= body_rows {
        0
    } else {
        app.selected
            .saturating_add(1)
            .saturating_sub(body_rows)
            .min(app.machines.len().saturating_sub(body_rows))
    };

    let rows: Vec<Row> = if app.machines.is_empty() {
        vec![Row::new(vec![Cell::from("  no machines yet")])]
    } else {
        app.machines
            .iter()
            .enumerate()
            .skip(start)
            .take(body_rows)
            .map(|(index, machine)| {
                let selected = index == app.selected;
                let marker = if selected { ">" } else { " " };
                let row_style = if selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                Row::new(vec![
                    Cell::from(format!("{marker} {}", machine.name)),
                    Cell::from(machine.state.clone()).style(state_style(&machine.state)),
                    Cell::from(machine.cpus.to_string()),
                    Cell::from(format!("{} MiB", machine.memory_mib)),
                    Cell::from(machine.origin()),
                ])
                .style(row_style)
            })
            .collect()
    };

    let count = app.machines.len();
    let table = Table::new(
        rows,
        [
            Constraint::Length(20),
            Constraint::Length(12),
            Constraint::Length(6),
            Constraint::Length(12),
            Constraint::Min(16),
        ],
    )
    .header(
        Row::new(vec!["NAME", "STATE", "CPUS", "MEMORY", "IMAGE / PARENT"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" smoltop {VERSION}  ({count}) ")),
    )
    .column_spacing(1);
    frame.render_widget(table, area);
}

fn draw_detail(frame: &mut Frame, app: &App, area: Rect) {
    let title = app
        .selected_machine()
        .map(|machine| format!(" {} ", machine.name))
        .unwrap_or_else(|| " machine ".into());
    let lines = detail_lines(app);
    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(machine) = app.selected_machine() else {
        return vec![Line::from(
            "No machines.  c create,  t throwaway from a pack.",
        )];
    };
    let stats = match &app.live {
        Some(live) => format!("cpu {}    mem {}", live.cpu_label(), live.memory_label()),
        None => app.stats_message.clone(),
    };
    vec![
        Line::from(vec![
            Span::styled(machine.state.clone(), state_style(&machine.state)),
            Span::raw(format!(
                "    {} cpus    {} MiB configured",
                machine.cpus, machine.memory_mib
            )),
        ]),
        Line::from(format!(
            "image {}    parent {}",
            machine.image.as_deref().unwrap_or("-"),
            machine.parent.as_deref().unwrap_or("-")
        )),
        Line::from(stats),
    ]
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let status_style = if app.status_error {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::Green)
    };
    let paragraph = Paragraph::new(vec![
        Line::styled(
            "j/k move   enter shell   s start   x stop   d delete   c create   t throwaway",
            Style::default().add_modifier(Modifier::DIM),
        ),
        Line::styled(
            "p pack   y copy   r refresh   ? help   q quit",
            Style::default().add_modifier(Modifier::DIM),
        ),
        Line::styled(app.status.clone(), status_style),
    ]);
    frame.render_widget(paragraph, area);
}

fn draw_modal(frame: &mut Frame, app: &App, area: Rect) {
    match &app.mode {
        Mode::Normal => {}
        Mode::Help => draw_popup(frame, area, 68, 16, " help ", help_lines()),
        Mode::Delete(prompt) => draw_popup(frame, area, 60, 5, " delete ", delete_lines(prompt)),
        Mode::Create(form) => draw_popup(frame, area, 68, 16, " create ", create_lines(form)),
        Mode::Throwaway(prompt) => {
            draw_popup(frame, area, 64, 14, " throwaway ", throwaway_lines(prompt));
        }
        Mode::Pack(prompt) => draw_popup(frame, area, 68, 6, " pack ", pack_lines(prompt)),
        Mode::Copy(form) => draw_popup(frame, area, 64, 7, " copy ", copy_lines(form)),
    }
}

fn draw_popup(
    frame: &mut Frame,
    area: Rect,
    width: u16,
    height: u16,
    title: &str,
    lines: Vec<Line<'static>>,
) {
    let width = width.min(area.width.saturating_sub(2)).max(8);
    let height = height.min(area.height.saturating_sub(2)).max(3);
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);
    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, popup);
}

fn help_lines() -> Vec<Line<'static>> {
    vec![
        Line::from("j/k or arrows     select a machine"),
        Line::from("enter             shell: machine exec --name NAME -it -- bash"),
        Line::from("s                 start"),
        Line::from("x                 stop"),
        Line::from("d, then y or n    delete (smolvm is then passed --force)"),
        Line::from("c                 create (image or a .smolmachine pack)"),
        Line::from("t                 throwaway from a pack, named tmp-N"),
        Line::from("p                 pack a stopped machine into the packs dir"),
        Line::from("y                 copy a stopped machine (pack, then create)"),
        Line::from("r                 refresh now"),
        Line::from("? or esc          close this help"),
        Line::from("q                 quit"),
        Line::from("ctrl-c            quit from anywhere"),
        Line::from("esc               cancel a prompt"),
    ]
}

fn delete_lines(prompt: &DeletePrompt) -> Vec<Line<'static>> {
    vec![
        Line::from(format!(
            "Delete '{}' and its disks? This cannot be undone.",
            prompt.name
        )),
        Line::from("y delete    n or esc cancel"),
    ]
}

fn create_lines(form: &CreateForm) -> Vec<Line<'static>> {
    let source = match form.source {
        CreateSourceKind::Image => "image",
        CreateSourceKind::Pack => "pack",
    };
    let network = if form.network {
        "on  (--net --dns)"
    } else {
        "off"
    };
    let pack = form
        .packs
        .get(form.pack_index)
        .map(|pack| pack.name.as_str())
        .unwrap_or("(none)");
    let mut lines = vec![
        field("name", &form.name, form.focus == CreateFocus::Name),
        field("source", source, form.focus == CreateFocus::Source),
    ];
    match form.source {
        CreateSourceKind::Image => {
            lines.push(field(
                "image",
                &form.image,
                form.focus == CreateFocus::Image,
            ));
        }
        CreateSourceKind::Pack => {
            lines.push(field("pack", pack, form.focus == CreateFocus::Pack));
            let shown = form.packs.len().min(6);
            for (index, pack) in form.packs.iter().take(shown).enumerate() {
                let marker = if index == form.pack_index { ">" } else { " " };
                lines.push(Line::from(format!("  {marker} {}", pack.name)));
            }
            if form.packs.is_empty() {
                lines.push(Line::from(format!(
                    "  no packs in {}",
                    form.packs_dir.display()
                )));
            }
        }
    }
    lines.push(field(
        "network",
        network,
        form.focus == CreateFocus::Network,
    ));
    lines.push(field("dns", &form.dns, form.focus == CreateFocus::Dns));
    lines.push(Line::from(
        "tab field   left/right change source or network   space toggles",
    ));
    lines.push(Line::from("enter create    esc cancel"));
    if let Some(error) = &form.error {
        lines.push(Line::styled(error.clone(), Style::default().fg(Color::Red)));
    }
    lines
}

fn throwaway_lines(prompt: &ThrowawayPrompt) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!("{}", prompt.dir.display())),
        Line::from("enter creates tmp-N with the configured network and dns"),
        Line::from("j/k select    esc cancel"),
    ];
    for (index, pack) in prompt.packs.iter().take(8).enumerate() {
        let marker = if index == prompt.index { ">" } else { " " };
        lines.push(Line::from(format!("{marker} {}", pack.name)));
    }
    lines
}

fn pack_lines(prompt: &PackPrompt) -> Vec<Line<'static>> {
    let sidecar = crate::commands::sidecar_path(&prompt.stub);
    vec![
        Line::from(format!("Pack stopped machine '{}'?", prompt.name)),
        Line::from(format!("stub {}", prompt.stub.display())),
        Line::from(format!("sidecar {}", sidecar.display())),
        Line::from("y pack    n or esc cancel"),
    ]
}

fn copy_lines(form: &CopyForm) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!(
            "Copy '{}' by packing it, then create --from that pack.",
            form.source
        )),
        field("name", &form.name, true),
        Line::from("enter copy    esc cancel"),
    ];
    if let Some(error) = &form.error {
        lines.push(Line::styled(error.clone(), Style::default().fg(Color::Red)));
    }
    lines
}

fn field(label: &str, value: &str, focused: bool) -> Line<'static> {
    let cursor = if focused { "_" } else { "" };
    let text = format!("{label:<8} {value}{cursor}");
    if focused {
        Line::styled(text, Style::default().add_modifier(Modifier::REVERSED))
    } else {
        Line::from(text)
    }
}

fn state_style(state: &str) -> Style {
    let color = match state {
        "running" => Color::Green,
        "created" => Color::Cyan,
        "paused" | "pausing" => Color::Yellow,
        "failed" | "unreachable" => Color::Red,
        "frozen" => Color::Magenta,
        _ => Color::Reset,
    };
    Style::default().fg(color)
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width, height)
}
