//! Drawing. Named ANSI colors only (the 16-color palette plus reset, bold,
//! dim, and reverse). No RGB and no 256-color indexes, so an Omarchy palette
//! swap recolors the screen. The terminal loop clears and redraws when the
//! theme path changes.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use crate::app::{
    App, CopyForm, CreateFocus, CreateForm, CreateSourceKind, DeletePrompt, Mode, PackPrompt,
    StatusTone, ThrowawayPrompt,
};
use crate::VERSION;

const ACCENT: Color = Color::Cyan;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width < 36 || area.height < 16 {
        frame.render_widget(
            Paragraph::new("terminal too small").style(Style::default().fg(Color::Yellow)),
            area,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(5),
            Constraint::Length(4),
        ])
        .split(area);

    draw_header(frame, chunks[0]);
    draw_table(frame, app, chunks[1]);
    draw_detail(frame, app, chunks[2]);
    draw_footer(frame, app, chunks[3]);
    draw_modal(frame, app, area);
}

fn draw_header(frame: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled(
            format!(" smoltop {VERSION}"),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled("   ", Style::default()),
        key_span("?"),
        label_span("help"),
        key_span("q"),
        label_span("quit"),
        key_span("r"),
        label_span("refresh"),
    ]);
    frame.render_widget(Paragraph::new(line), area);
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
        vec![Row::new(vec![Cell::from(Span::styled(
            "  no machines yet",
            Style::default().add_modifier(Modifier::DIM),
        ))])]
    } else {
        app.machines
            .iter()
            .enumerate()
            .skip(start)
            .take(body_rows)
            .map(|(index, machine)| {
                let selected = index == app.selected;
                let marker = if selected { ">" } else { " " };
                let name = format!("{marker} {}", machine.name);
                let row_style = if selected {
                    Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
                } else {
                    Style::default()
                };
                Row::new(vec![
                    Cell::from(name),
                    Cell::from(state_badge(&machine.state)),
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
            Constraint::Length(22),
            Constraint::Length(14),
            Constraint::Length(6),
            Constraint::Length(12),
            Constraint::Min(16),
        ],
    )
    .header(Row::new(vec![
        header_cell("NAME"),
        header_cell("STATE"),
        header_cell("CPUS"),
        header_cell("MEMORY"),
        header_cell("IMAGE / PARENT"),
    ]))
    .block(panel(
        Line::from(vec![
            Span::styled(
                " machines ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {count} "),
                Style::default().add_modifier(Modifier::DIM),
            ),
        ]),
        ACCENT,
    ))
    .column_spacing(1);
    frame.render_widget(table, area);
}

fn draw_detail(frame: &mut Frame, app: &App, area: Rect) {
    let (title, color) = match app.selected_machine() {
        Some(machine) => (
            format!(" {} ", machine.name),
            state_color(&machine.state).unwrap_or(Color::Gray),
        ),
        None => (" machine ".into(), ACCENT),
    };
    let paragraph = Paragraph::new(detail_lines(app))
        .block(panel(
            Span::styled(
                title,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            color,
        ))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn detail_lines(app: &App) -> Vec<Line<'static>> {
    let Some(machine) = app.selected_machine() else {
        return vec![Line::from(vec![
            Span::styled(
                "No machines.  ",
                Style::default().add_modifier(Modifier::DIM),
            ),
            key_span("c"),
            label_span("create"),
            key_span("t"),
            label_span("throwaway"),
        ])];
    };
    let (stats, stats_style) = match &app.live {
        Some(live) => (
            format!("cpu {}    mem {}", live.cpu_label(), live.memory_label()),
            Style::default().fg(Color::Green),
        ),
        None if app.stats_message.contains("unavailable") => (
            app.stats_message.clone(),
            Style::default().fg(Color::Yellow),
        ),
        None => (
            app.stats_message.clone(),
            Style::default().add_modifier(Modifier::DIM),
        ),
    };
    vec![
        Line::from(vec![
            state_badge(&machine.state),
            Span::raw(format!(
                "  {} cpus    {} MiB configured",
                machine.cpus, machine.memory_mib
            )),
        ]),
        Line::from(vec![
            Span::styled("image ", Style::default().add_modifier(Modifier::DIM)),
            Span::raw(machine.image.clone().unwrap_or_else(|| "-".into())),
            Span::styled("    parent ", Style::default().add_modifier(Modifier::DIM)),
            Span::raw(machine.parent.clone().unwrap_or_else(|| "-".into())),
        ]),
        Line::styled(stats, stats_style),
    ]
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let mut keys = vec![Span::raw(" ")];
    for (key, label) in [
        ("j/k", "move"),
        ("enter", "shell"),
        ("s", "start"),
        ("x", "stop"),
        ("d", "delete"),
        ("c", "create"),
        ("t", "throwaway"),
        ("p", "pack"),
        ("y", "copy"),
    ] {
        keys.push(key_span(key));
        keys.push(label_span(label));
    }
    let paragraph = Paragraph::new(vec![
        Line::from(keys),
        Line::styled(format!(" {}", app.status), tone_style(app.status_tone())),
    ])
    .block(panel(
        Span::styled(
            " keys ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        ACCENT,
    ));
    frame.render_widget(paragraph, area);
}

fn draw_modal(frame: &mut Frame, app: &App, area: Rect) {
    match &app.mode {
        Mode::Normal => {}
        Mode::Help => draw_popup(frame, area, 72, 17, " help ", help_lines(), ACCENT),
        Mode::Delete(prompt) => {
            draw_popup(
                frame,
                area,
                62,
                6,
                " delete ",
                delete_lines(prompt),
                Color::Red,
            );
        }
        Mode::Create(form) => {
            draw_popup(frame, area, 70, 16, " create ", create_lines(form), ACCENT)
        }
        Mode::Throwaway(prompt) => {
            draw_popup(
                frame,
                area,
                64,
                14,
                " throwaway ",
                throwaway_lines(prompt),
                ACCENT,
            );
        }
        Mode::Pack(prompt) => draw_popup(frame, area, 68, 7, " pack ", pack_lines(prompt), ACCENT),
        Mode::Copy(form) => draw_popup(frame, area, 68, 8, " copy ", copy_lines(form), ACCENT),
    }
}

fn draw_popup(
    frame: &mut Frame,
    area: Rect,
    width: u16,
    height: u16,
    title: &str,
    lines: Vec<Line<'static>>,
    color: Color,
) {
    let width = width.min(area.width.saturating_sub(2)).max(8);
    let height = height.min(area.height.saturating_sub(2)).max(3);
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);
    let paragraph = Paragraph::new(lines)
        .block(panel(
            Span::styled(
                title.to_string(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            color,
        ))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, popup);
}

fn help_lines() -> Vec<Line<'static>> {
    vec![
        bind("j/k, arrows", "select a machine"),
        bind("enter", "shell: machine exec --name NAME -it -- bash"),
        bind("s", "start"),
        bind("x", "stop"),
        bind("d, then y or n", "delete (smolvm is then passed --force)"),
        bind("c", "create (image or a .smolmachine pack)"),
        bind("t", "throwaway from a pack, named tmp-N"),
        bind("p", "pack a stopped machine into the packs dir"),
        bind("y", "copy a stopped machine (pack, then create)"),
        bind("r", "refresh now"),
        bind("? or esc", "close this help"),
        bind("q", "quit"),
        bind("ctrl-c", "quit from anywhere"),
        bind("esc", "cancel a prompt"),
    ]
}

fn bind(key: &str, action: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {key:<16}"),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::raw(action.to_string()),
    ])
}

fn delete_lines(prompt: &DeletePrompt) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::raw("Delete "),
            Span::styled(
                prompt.name.clone(),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" and its disks? This cannot be undone."),
        ]),
        Line::from(vec![
            key_span("y"),
            Span::styled("delete    ", Style::default().fg(Color::Red)),
            key_span("n"),
            label_span("or"),
            key_span("esc"),
            label_span("cancel"),
        ]),
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
                let style = if index == form.pack_index {
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                lines.push(Line::styled(format!("  {marker} {}", pack.name), style));
            }
            if form.packs.is_empty() {
                lines.push(Line::styled(
                    format!("  no packs in {}", form.packs_dir.display()),
                    Style::default().add_modifier(Modifier::DIM),
                ));
            }
        }
    }
    lines.push(field(
        "network",
        network,
        form.focus == CreateFocus::Network,
    ));
    lines.push(field("dns", &form.dns, form.focus == CreateFocus::Dns));
    lines.push(Line::styled(
        "tab field   left/right change source or network   space toggles",
        Style::default().add_modifier(Modifier::DIM),
    ));
    lines.push(Line::from(vec![
        key_span("enter"),
        label_span("create"),
        key_span("esc"),
        label_span("cancel"),
    ]));
    if let Some(error) = &form.error {
        lines.push(Line::styled(
            error.clone(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }
    lines
}

fn throwaway_lines(prompt: &ThrowawayPrompt) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(prompt.dir.display().to_string()),
        Line::from("enter creates tmp-N with the configured network and dns"),
        Line::from(vec![
            key_span("j/k"),
            label_span("select"),
            key_span("esc"),
            label_span("cancel"),
        ]),
    ];
    for (index, pack) in prompt.packs.iter().take(8).enumerate() {
        let marker = if index == prompt.index { ">" } else { " " };
        let style = if index == prompt.index {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::styled(format!("{marker} {}", pack.name), style));
    }
    lines
}

fn pack_lines(prompt: &PackPrompt) -> Vec<Line<'static>> {
    let sidecar = crate::commands::sidecar_path(&prompt.stub);
    vec![
        Line::from(format!("Pack stopped machine '{}'?", prompt.name)),
        Line::styled(
            format!("stub {}", prompt.stub.display()),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Line::styled(
            format!("sidecar {}", sidecar.display()),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Line::from(vec![
            key_span("y"),
            label_span("pack"),
            key_span("n"),
            label_span("or"),
            key_span("esc"),
            label_span("cancel"),
        ]),
    ]
}

fn copy_lines(form: &CopyForm) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!(
            "Copy '{}' by packing it, then create --from that pack.",
            form.source
        )),
        Line::styled(
            "network and dns come from the config",
            Style::default().add_modifier(Modifier::DIM),
        ),
        field("name", &form.name, true),
        Line::from(vec![
            key_span("enter"),
            label_span("copy"),
            key_span("esc"),
            label_span("cancel"),
        ]),
    ];
    if let Some(error) = &form.error {
        lines.push(Line::styled(
            error.clone(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }
    lines
}

fn field(label: &str, value: &str, focused: bool) -> Line<'static> {
    let cursor = if focused { "_" } else { "" };
    let text = format!(" {label:<8} {value}{cursor}");
    if focused {
        Line::styled(
            text,
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )
    } else {
        Line::from(text)
    }
}

fn state_badge(state: &str) -> Span<'static> {
    Span::styled(format!("{state:<12}"), state_style(state))
}

fn state_style(state: &str) -> Style {
    match state_color(state) {
        Some(color) => Style::default().fg(color).add_modifier(Modifier::BOLD),
        None => Style::default().add_modifier(Modifier::DIM),
    }
}

fn state_color(state: &str) -> Option<Color> {
    match state {
        "running" => Some(Color::Green),
        "created" => Some(Color::Blue),
        "paused" | "pausing" => Some(Color::Yellow),
        "failed" | "unreachable" => Some(Color::Red),
        "frozen" => Some(Color::Magenta),
        _ => None,
    }
}

fn tone_style(tone: StatusTone) -> Style {
    match tone {
        StatusTone::Error => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        StatusTone::Ok => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        StatusTone::Busy => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        StatusTone::Info => Style::default().add_modifier(Modifier::DIM),
    }
}

fn panel(title: impl Into<Line<'static>>, color: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .title(title.into())
}

fn header_cell(text: &str) -> Cell<'static> {
    Cell::from(Span::styled(
        text.to_string(),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ))
}

fn key_span(key: &str) -> Span<'static> {
    Span::styled(
        key.to_string(),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )
}

fn label_span(label: &str) -> Span<'static> {
    Span::styled(
        format!(" {label}  "),
        Style::default().add_modifier(Modifier::DIM),
    )
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::parse::Machine;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;
    use std::path::PathBuf;

    fn sample() -> App {
        let mut app = App::new(
            Config::default(),
            PathBuf::from("/tmp/smoltop-ui.toml"),
            true,
        );
        app.machines = vec![
            Machine {
                name: "demo".into(),
                state: "running".into(),
                cpus: 2,
                memory_mib: 1024,
                image: Some("alpine".into()),
                parent: Some("base".into()),
            },
            Machine {
                name: "old".into(),
                state: "stopped".into(),
                cpus: 1,
                memory_mib: 512,
                image: Some("debian".into()),
                parent: None,
            },
            Machine {
                name: "fresh".into(),
                state: "created".into(),
                cpus: 1,
                memory_mib: 256,
                image: None,
                parent: None,
            },
        ];
        app.selected = 0;
        app.status = "DEMO: start refused".into();
        app.status_error = true;
        app
    }

    fn render(app: &App) -> Buffer {
        let backend = TestBackend::new(100, 28);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn paint(buf: &Buffer) -> String {
        let mut text = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                text.push_str(buf[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn screen_uses_ansi_colors_badges_and_a_reversed_selection() {
        let app = sample();
        let buf = render(&app);
        let text = paint(&buf);
        assert!(text.contains("demo"), "{text}");
        assert!(text.contains("running"), "{text}");
        assert!(text.contains("stopped"), "{text}");
        assert!(text.contains("created"), "{text}");
        assert!(text.contains("DEMO: start refused"), "{text}");
        assert!(text.contains('╭'), "rounded panel missing:\n{text}");

        let mut saw_reversed = false;
        let mut saw_green = false;
        let mut saw_blue = false;
        let mut saw_red = false;
        let mut saw_dim = false;
        let mut saw_cyan_bold = false;
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let cell = &buf[(x, y)];
                assert!(
                    is_palette(cell.fg) && is_palette(cell.bg),
                    "non-palette color fg {:?} bg {:?}",
                    cell.fg,
                    cell.bg
                );
                if cell.modifier.contains(Modifier::REVERSED) && cell.symbol() == "d" {
                    saw_reversed = true;
                }
                if cell.fg == Color::Green {
                    saw_green = true;
                }
                if cell.fg == Color::Blue {
                    saw_blue = true;
                }
                if cell.fg == Color::Red {
                    saw_red = true;
                }
                if cell.modifier.contains(Modifier::DIM) {
                    saw_dim = true;
                }
                if cell.fg == Color::Cyan && cell.modifier.contains(Modifier::BOLD) {
                    saw_cyan_bold = true;
                }
            }
        }
        assert!(saw_reversed, "selected row is not reversed");
        assert!(saw_green, "running badge is not green");
        assert!(saw_blue, "created badge is not blue");
        assert!(saw_red, "error status is not red");
        assert!(saw_dim, "stopped state is not dim");
        assert!(saw_cyan_bold, "headers are not cyan bold");

        let mut help = sample();
        help.mode = Mode::Help;
        help.status_error = false;
        help.status = "shell closed".into();
        let help_text = paint(&render(&help));
        assert!(
            help_text.contains("machine exec --name NAME -it -- bash"),
            "{help_text}"
        );
    }

    fn is_palette(color: Color) -> bool {
        matches!(
            color,
            Color::Reset
                | Color::Black
                | Color::Red
                | Color::Green
                | Color::Yellow
                | Color::Blue
                | Color::Magenta
                | Color::Cyan
                | Color::Gray
                | Color::DarkGray
                | Color::LightRed
                | Color::LightGreen
                | Color::LightYellow
                | Color::LightBlue
                | Color::LightMagenta
                | Color::LightCyan
                | Color::White
        )
    }
}
