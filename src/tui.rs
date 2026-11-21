use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Terminal,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use crate::agent::{self, AgentEvent};
use crate::config::Config;
use crate::llm::ChatMessage;

#[derive(Clone, Serialize, Deserialize)]
struct Msg {
    role: String,
    text: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Session {
    id: String,
    title: String,
    messages: Vec<Msg>,
    history: Vec<ChatMessage>,
    created_at: u64,
}

struct App {
    cfg: Config,
    sessions: Vec<Session>,
    current: usize,
    input: String,
    input_cursor: usize,
    status: String,
    busy: bool,
    scroll: u16,
    auto_scroll: bool,
    show_palette: bool,
    palette_sel: usize,
    palette_filter: String,
    show_sessions: bool,
    session_sel: usize,
    show_help: bool,
    show_model: bool,
    model_input: String,
    prompt_history: Vec<String>,
    prompt_history_idx: Option<usize>,
    last_error: Option<String>,
}

enum UiEvent {
    Agent(AgentEvent),
    Done(Vec<ChatMessage>),
}

#[derive(Clone)]
struct Cmd {
    id: &'static str,
    title: &'static str,
    key: &'static str,
}

fn commands() -> Vec<Cmd> {
    vec![
        Cmd { id: "session.new", title: "New session", key: "Ctrl+N" },
        Cmd { id: "session.list", title: "Switch/list sessions", key: "Ctrl+L" },
        Cmd { id: "model.set", title: "Set model", key: "" },
        Cmd { id: "model.info", title: "Show model/config", key: "" },
        Cmd { id: "session.clear", title: "Clear current messages", key: "" },
        Cmd { id: "help.show", title: "Show help", key: "Ctrl+G / ?" },
        Cmd { id: "app.quit", title: "Quit", key: "Ctrl+C" },
    ]
}

pub async fn run(cfg: Config) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let res = app_loop(&mut terminal, cfg).await;
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

fn data_file() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|b| b.data_dir().to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("opencode-native/sessions.json")
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn load_sessions() -> Vec<Session> {
    let p = data_file();
    fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_sessions(sessions: &[Session]) {
    let p = data_file();
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string_pretty(sessions) {
        let _ = fs::write(p, s);
    }
}

fn new_session() -> Session {
    let id = format!("s-{}", now_ms());
    Session {
        id,
        title: "New session".into(),
        messages: vec![Msg {
            role: "sys".into(),
            text: "New session. Enter to send, Ctrl+P commands, /help for commands.".into(),
        }],
        history: vec![],
        created_at: now_ms(),
    }
}

fn push_msg(app: &mut App, role: &str, text: impl Into<String>) {
    app.sessions[app.current].messages.push(Msg {
        role: role.into(),
        text: text.into(),
    });
    app.auto_scroll = true;
    app.scroll = 0;
}

fn filtered_commands(filter: &str) -> Vec<Cmd> {
    let f = filter.to_lowercase();
    commands()
        .into_iter()
        .filter(|c| f.is_empty() || c.id.contains(&f) || c.title.to_lowercase().contains(&f))
        .collect()
}

async fn app_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, cfg: Config) -> Result<()> {
    let mut sessions = load_sessions();
    if sessions.is_empty() {
        sessions.push(new_session());
    }
    let mut app = App {
        cfg,
        sessions,
        current: 0,
        input: String::new(),
        input_cursor: 0,
        status: "idle".into(),
        busy: false,
        scroll: 0,
        auto_scroll: true,
        show_palette: false,
        palette_sel: 0,
        palette_filter: String::new(),
        show_sessions: false,
        session_sel: 0,
        show_help: false,
        show_model: false,
        model_input: String::new(),
        prompt_history: Vec::new(),
        prompt_history_idx: None,
        last_error: None,
    };
    let (tx, rx) = mpsc::channel::<UiEvent>();

    loop {
        while let Ok(ev) = rx.try_recv() {
            match ev {
                UiEvent::Agent(AgentEvent::Status(s)) => app.status = s,
                UiEvent::Agent(AgentEvent::ToolCall { name, args }) => {
                    let short: String = args.chars().take(200).collect();
                    push_msg(&mut app, "tool", format!("▶ {name} {short}"));
                }
                UiEvent::Agent(AgentEvent::ToolResult { name, output }) => {
                    let short = output.lines().take(10).collect::<Vec<_>>().join("\n");
                    let more = output.lines().count().saturating_sub(10);
                    let mut text = format!("✓ {name}\n{short}");
                    if more > 0 { text.push_str(&format!("\n... ({more} more lines)")); }
                    push_msg(&mut app, "tool", text);
                }
                UiEvent::Agent(AgentEvent::AssistantDelta(t)) => {
                    if let Some(last) = app.sessions[app.current].messages.last_mut() {
                        if last.role == "assistant_stream" {
                            last.text.push_str(&t);
                        } else {
                            app.sessions[app.current].messages.push(Msg { role: "assistant_stream".into(), text: t });
                        }
                    } else {
                        app.sessions[app.current].messages.push(Msg { role: "assistant_stream".into(), text: t });
                    }
                    app.auto_scroll = true; app.scroll = 0;
                }
                UiEvent::Agent(AgentEvent::Assistant(t)) => {
                    if let Some(last) = app.sessions[app.current].messages.last_mut() {
                        if last.role == "assistant_stream" {
                            last.role = "assistant".into();
                            last.text = t.clone();
                        } else {
                            app.sessions[app.current].messages.push(Msg { role: "assistant".into(), text: t });
                        }
                    } else {
                        app.sessions[app.current].messages.push(Msg { role: "assistant".into(), text: t });
                    }
                    app.auto_scroll = true; app.scroll = 0;
                }
                UiEvent::Agent(AgentEvent::Error(e)) => {
                    app.last_error = Some(e.clone());
                    push_msg(&mut app, "error", e);
                }
                UiEvent::Done(hist) => {
                    app.busy = false;
                    app.status = "idle".into();
                    app.sessions[app.current].history = hist;
                    if app.sessions[app.current].title == "New session" {
                        if let Some(m) = app.sessions[app.current].messages.iter().find(|m| m.role == "user") {
                            app.sessions[app.current].title = m.text.chars().take(48).collect();
                        }
                    }
                    save_sessions(&app.sessions);
                }
            }
        }

        terminal.draw(|f| draw_ui(f, &app))?;

        if !event::poll(Duration::from_millis(80))? {
            continue;
        }
        let Event::Key(key) = event::read()? else { continue; };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        if app.show_model {
            match key.code {
                KeyCode::Esc => app.show_model = false,
                KeyCode::Enter => {
                    let m = app.model_input.trim().to_string();
                    if !m.is_empty() {
                        app.cfg.model.model = m.clone();
                        push_msg(&mut app, "sys", format!("Model set to: {m}"));
                    }
                    app.show_model = false;
                }
                KeyCode::Backspace => { app.model_input.pop(); }
                KeyCode::Char(c) => {
                    if !key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT) {
                        app.model_input.push(c);
                    }
                }
                _ => {}
            }
            continue;
        }

        if app.show_palette {
            let cmds = filtered_commands(&app.palette_filter);
            match key.code {
                KeyCode::Esc => { app.show_palette = false; app.palette_filter.clear(); }
                KeyCode::Up => app.palette_sel = app.palette_sel.saturating_sub(1),
                KeyCode::Down => if !cmds.is_empty() { app.palette_sel = (app.palette_sel + 1).min(cmds.len() - 1); },
                KeyCode::Backspace => { app.palette_filter.pop(); app.palette_sel = 0; }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT) => {
                    app.palette_filter.push(c); app.palette_sel = 0;
                }
                KeyCode::Enter => {
                    if let Some(cmd) = cmds.get(app.palette_sel).cloned() {
                        app.show_palette = false;
                        app.palette_filter.clear();
                        run_command(&mut app, cmd.id, &tx);
                    }
                }
                _ => {}
            }
            continue;
        }

        if app.show_sessions {
            match key.code {
                KeyCode::Esc => app.show_sessions = false,
                KeyCode::Up => app.session_sel = app.session_sel.saturating_sub(1),
                KeyCode::Down => app.session_sel = (app.session_sel + 1).min(app.sessions.len().saturating_sub(1)),
                KeyCode::Enter => { app.current = app.session_sel; app.show_sessions = false; app.auto_scroll = true; app.scroll = 0; }
                KeyCode::Delete => {
                    if app.sessions.len() > 1 {
                        app.sessions.remove(app.session_sel);
                        if app.current >= app.sessions.len() { app.current = app.sessions.len() - 1; }
                        if app.session_sel >= app.sessions.len() { app.session_sel = app.sessions.len() - 1; }
                        save_sessions(&app.sessions);
                    }
                }
                _ => {}
            }
            continue;
        }

        if app.show_help {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
                app.show_help = false;
            }
            continue;
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                save_sessions(&app.sessions);
                return Ok(());
            }
            (KeyModifiers::CONTROL, KeyCode::Char('p')) => {
                app.show_palette = true; app.palette_sel = 0; app.palette_filter.clear();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('n')) => run_command(&mut app, "session.new", &tx),
            (KeyModifiers::CONTROL, KeyCode::Char('l')) => run_command(&mut app, "session.list", &tx),
            (KeyModifiers::CONTROL, KeyCode::Char('g')) => app.show_help = true,
            (_, KeyCode::Esc) => {
                if app.busy {
                    push_msg(&mut app, "sys", "Interrupt requested (current MVP run may still finish in background).");
                } else {
                    save_sessions(&app.sessions);
                    return Ok(());
                }
            }
            (_, KeyCode::PageUp) => { app.scroll = app.scroll.saturating_add(8); app.auto_scroll = false; }
            (_, KeyCode::PageDown) => { app.scroll = app.scroll.saturating_sub(8); if app.scroll == 0 { app.auto_scroll = true; } }
            (_, KeyCode::Up) => {
                if app.input.is_empty() || app.prompt_history_idx.is_some() {
                    if !app.prompt_history.is_empty() {
                        let idx = match app.prompt_history_idx {
                            Some(i) => i.saturating_sub(1),
                            None => app.prompt_history.len() - 1,
                        };
                        app.prompt_history_idx = Some(idx);
                        app.input = app.prompt_history[idx].clone();
                        app.input_cursor = app.input.len();
                    }
                }
            }
            (_, KeyCode::Down) => {
                if let Some(idx) = app.prompt_history_idx {
                    if idx + 1 < app.prompt_history.len() {
                        let ni = idx + 1;
                        app.prompt_history_idx = Some(ni);
                        app.input = app.prompt_history[ni].clone();
                    } else {
                        app.prompt_history_idx = None;
                        app.input.clear();
                    }
                    app.input_cursor = app.input.len();
                }
            }
            (_, KeyCode::Enter) => {
                let text = app.input.trim().to_string();
                if text.is_empty() || app.busy { continue; }
                app.input.clear();
                app.input_cursor = 0;
                app.prompt_history_idx = None;
                if app.prompt_history.last().map(|s| s.as_str()) != Some(text.as_str()) {
                    app.prompt_history.push(text.clone());
                }
                if handle_slash(&mut app, &text, &tx) { continue; }
                submit_prompt(&mut app, text, &tx);
            }
            (_, KeyCode::Backspace) => {
                if app.input_cursor > 0 && app.input_cursor <= app.input.len() {
                    app.input.remove(app.input_cursor - 1);
                    app.input_cursor -= 1;
                }
            }
            (_, KeyCode::Left) => { app.input_cursor = app.input_cursor.saturating_sub(1); }
            (_, KeyCode::Right) => { app.input_cursor = (app.input_cursor + 1).min(app.input.len()); }
            (_, KeyCode::Home) => { app.input_cursor = 0; }
            (_, KeyCode::End) => { app.input_cursor = app.input.len(); }
            (_, KeyCode::Char(c)) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::ALT) {
                    continue;
                }
                let idx = app.input_cursor.min(app.input.len());
                app.input.insert(idx, c);
                app.input_cursor += c.len_utf8();
            }
            _ => {}
        }
    }
}

fn handle_slash(app: &mut App, text: &str, tx: &mpsc::Sender<UiEvent>) -> bool {
    let mut parts = text.trim().splitn(2, ' ');
    let cmd = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim();
    match cmd {
        "/quit" | "/exit" => { save_sessions(&app.sessions); std::process::exit(0); }
        "/new" => { run_command(app, "session.new", tx); true }
        "/sessions" => { run_command(app, "session.list", tx); true }
        "/help" => { app.show_help = true; true }
        "/clear" => { run_command(app, "session.clear", tx); true }
        "/model" => {
            if arg.is_empty() { run_command(app, "model.info", tx); }
            else { app.cfg.model.model = arg.to_string(); push_msg(app, "sys", format!("Model set to: {arg}")); }
            true
        }
        _ => false,
    }
}

fn run_command(app: &mut App, id: &str, _tx: &mpsc::Sender<UiEvent>) {
    match id {
        "session.new" => {
            app.sessions.insert(0, new_session());
            app.current = 0;
            app.status = "idle".into();
            save_sessions(&app.sessions);
        }
        "session.list" => { app.show_sessions = true; app.session_sel = app.current; }
        "model.info" => {
            push_msg(app, "sys", format!(
                "provider={}\nmodel={}\nbase_url={}\napi_key_env={}\nconfig_key={}",
                app.cfg.model.provider, app.cfg.model.model, app.cfg.model.base_url, app.cfg.model.api_key_env,
                if app.cfg.model.api_key.is_some() { "set" } else { "unset" }
            ));
        }
        "model.set" => {
            app.model_input = app.cfg.model.model.clone();
            app.show_model = true;
        }
        "session.clear" => { app.sessions[app.current].messages.clear(); app.sessions[app.current].history.clear(); save_sessions(&app.sessions); }
        "help.show" => { app.show_help = true; }
        "app.quit" => { save_sessions(&app.sessions); std::process::exit(0); }
        _ => push_msg(app, "sys", format!("Unknown command: {id}")),
    }
}

fn submit_prompt(app: &mut App, text: String, tx: &mpsc::Sender<UiEvent>) {
    push_msg(app, "user", text.clone());
    app.busy = true;
    app.status = "queued".into();
    let cfg2 = app.cfg.clone();
    let mut hist2 = app.sessions[app.current].history.clone();
    let tx2 = tx.clone();
    tokio::spawn(async move {
        let atx = tx2.clone();
        let (etx, erx) = mpsc::channel::<AgentEvent>();
        let atx2 = atx.clone();
        std::thread::spawn(move || {
            while let Ok(ev) = erx.recv() {
                let _ = atx2.send(UiEvent::Agent(ev));
            }
        });
        let res = agent::chat_turn(&cfg2, &mut hist2, &text, Some(etx)).await;
        if let Err(e) = res {
            let _ = atx.send(UiEvent::Agent(AgentEvent::Error(format!("{e:#}"))));
        }
        let _ = atx.send(UiEvent::Done(hist2));
    });
}

fn draw_ui(f: &mut ratatui::Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(f.area());

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let session = &app.sessions[app.current];
    let header = Line::from(vec![
        Span::styled(" opencode-native ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled(session.title.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(cwd.display().to_string(), Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(format!("{} sessions", app.sessions.len()), Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(header), root[0]);

    let mut lines: Vec<Line> = Vec::new();
    for m in &session.messages {
        let (label, color) = match m.role.as_str() {
            "user" => ("You", Color::Cyan),
            "assistant" | "assistant_stream" => ("Assistant", Color::Green),
            "tool" => ("Tool", Color::Magenta),
            "error" => ("Error", Color::Red),
            _ => ("System", Color::Yellow),
        };
        lines.push(Line::from(Span::styled(label, Style::default().fg(color).add_modifier(Modifier::BOLD))));
        for l in m.text.lines() {
            lines.push(Line::from(Span::raw(format!("  {l}"))));
        }
        lines.push(Line::from(""));
    }
    let msgw = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title(" Session "))
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0));
    f.render_widget(msgw, root[1]);

    let prompt_title = if app.busy { " Prompt (running...) " } else { " Prompt " };
    let input = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title(prompt_title))
        .wrap(Wrap { trim: false });
    f.render_widget(input, root[2]);
    if !app.busy {
        let x = root[2].x + 1 + (app.input_cursor as u16).min(root[2].width.saturating_sub(2));
        let y = root[2].y + 1;
        f.set_cursor_position((x, y));
    }

    let footer = Line::from(vec![
        Span::styled(format!(" {} ", app.status), Style::default().fg(Color::Yellow)),
        Span::raw("| "),
        Span::styled(app.cfg.model.model.clone(), Style::default().fg(Color::DarkGray)),
        Span::raw(" | "),
        Span::styled(app.cfg.model.provider.clone(), Style::default().fg(Color::DarkGray)),
        Span::raw(" | Ctrl+P palette  Ctrl+N new  Ctrl+L sessions  Ctrl+G help  Ctrl+C quit"),
    ]);
    f.render_widget(Paragraph::new(footer), root[3]);

    if app.show_help {
        let area = centered_rect(72, 72, f.area());
        f.render_widget(Clear, area);
        let t = "\
opencode-native TUI (OpenCode-style lightweight port)

Keys:
  Enter         Send prompt
  Ctrl+P        Command palette
  Ctrl+N        New session
  Ctrl+L        Sessions list
  Ctrl+G        Help
  Ctrl+C        Quit
  Esc           Close overlay / quit when idle
  PgUp/PgDn     Scroll messages
  Up/Down       Prompt history (when input empty/history browsing)

Slash commands:
  /new /sessions /model [name] /help /clear /quit

Notes:
  - Native Rust TUI, no Bun/proot runtime.
  - Uses OpenAI-compatible API config from config.toml.
  - Not full OpenCode server parity yet; actively being ported.";
        f.render_widget(Paragraph::new(t).wrap(Wrap { trim: false }).block(Block::default().borders(Borders::ALL).title(" Help ")), area);
    }

    if app.show_palette {
        let cmds = filtered_commands(&app.palette_filter);
        let area = centered_rect(70, 60, f.area());
        f.render_widget(Clear, area);
        let chunks = Layout::default().direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(area);
        f.render_widget(Paragraph::new(app.palette_filter.as_str())
            .block(Block::default().borders(Borders::ALL).title(" Command palette (type to filter) ")), chunks[0]);
        let items: Vec<ListItem> = cmds.iter().enumerate().map(|(i, c)| {
            let st = if i == app.palette_sel { Style::default().fg(Color::Black).bg(Color::Green) } else { Style::default() };
            let right = if c.key.is_empty() { c.id.to_string() } else { format!("{}  {}", c.id, c.key) };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<24}", c.title), st.add_modifier(Modifier::BOLD)),
                Span::styled(right, st.fg(if i == app.palette_sel { Color::Black } else { Color::DarkGray })),
            ]))
        }).collect();
        f.render_widget(List::new(items).block(Block::default().borders(Borders::ALL).title(" Commands ")), chunks[1]);
    }

    if app.show_sessions {
        let area = centered_rect(75, 65, f.area());
        f.render_widget(Clear, area);
        let items: Vec<ListItem> = app.sessions.iter().enumerate().map(|(i, s)| {
            let st = if i == app.session_sel { Style::default().fg(Color::Black).bg(Color::Cyan) } else { Style::default() };
            let cur = if i == app.current { "*" } else { " " };
            let msg_count = s.messages.len();
            ListItem::new(Line::from(Span::styled(format!("{cur} {}  [{} msgs]", s.title, msg_count), st)))
        }).collect();
        f.render_widget(List::new(items).block(Block::default().borders(Borders::ALL).title(" Sessions (Enter switch, Del delete, Esc close) ")), area);
    }

    if app.show_model {
        let area = centered_rect(60, 20, f.area());
        f.render_widget(Clear, area);
        f.render_widget(Paragraph::new(app.model_input.as_str())
            .block(Block::default().borders(Borders::ALL).title(" Set model (Enter apply, Esc cancel) ")), area);
    }
}

fn centered_rect(px: u16, py: u16, r: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - py) / 2),
            Constraint::Percentage(py),
            Constraint::Percentage((100 - py) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - px) / 2),
            Constraint::Percentage(px),
            Constraint::Percentage((100 - px) / 2),
        ])
        .split(v[1])[1]
}
