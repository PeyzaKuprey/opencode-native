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
struct Msg { role: String, text: String }

#[derive(Clone, Serialize, Deserialize)]
struct Session {
    id: String,
    title: String,
    messages: Vec<Msg>,
    history: Vec<ChatMessage>,
}

struct App {
    cfg: Config,
    sessions: Vec<Session>,
    current: usize,
    input: String,
    status: String,
    busy: bool,
    scroll: u16,
    show_palette: bool,
    palette_sel: usize,
    show_sessions: bool,
    session_sel: usize,
    show_help: bool,
}

enum UiEvent { Agent(AgentEvent), Done(Vec<ChatMessage>) }

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
    let base = std::env::var("XDG_DATA_HOME").ok().filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|b| b.data_dir().to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("opencode-native/sessions.json")
}

fn load_sessions() -> Vec<Session> {
    let p = data_file();
    fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}
fn save_sessions(sessions: &[Session]) {
    let p = data_file();
    if let Some(parent) = p.parent() { let _ = fs::create_dir_all(parent); }
    if let Ok(s) = serde_json::to_string_pretty(sessions) { let _ = fs::write(p, s); }
}
fn new_session() -> Session {
    let id = format!("s-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis());
    Session { id, title: "New session".into(), messages: vec![Msg{role:"sys".into(), text:"New session. Type a prompt, or Ctrl+P for commands.".into()}], history: vec![] }
}

async fn app_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, cfg: Config) -> Result<()> {
    let mut sessions = load_sessions();
    if sessions.is_empty() { sessions.push(new_session()); }
    let mut app = App {
        cfg, sessions, current: 0, input: String::new(), status: "idle".into(), busy: false,
        scroll: 0, show_palette: false, palette_sel: 0, show_sessions: false, session_sel: 0, show_help: false,
    };
    let (tx, rx) = mpsc::channel::<UiEvent>();

    let commands: Vec<(&str,&str)> = vec![
        ("session.new", "New session"),
        ("session.list", "Sessions list"),
        ("model.info", "Show model/config"),
        ("help.show", "Help"),
        ("session.clear", "Clear current session messages"),
        ("app.quit", "Quit"),
    ];

    loop {
        while let Ok(ev) = rx.try_recv() {
            match ev {
                UiEvent::Agent(AgentEvent::Status(s)) => app.status = s,
                UiEvent::Agent(AgentEvent::ToolCall{name,args}) => {
                    let short: String = args.chars().take(160).collect();
                    app.sessions[app.current].messages.push(Msg{role:"tool".into(), text:format!("▶ {name} {short}")});
                }
                UiEvent::Agent(AgentEvent::ToolResult{name,output}) => {
                    let short = output.lines().take(8).collect::<Vec<_>>().join("\n");
                    app.sessions[app.current].messages.push(Msg{role:"tool".into(), text:format!("✓ {name}\n{short}")});
                }
                UiEvent::Agent(AgentEvent::Assistant(t)) => {
                    app.sessions[app.current].messages.push(Msg{role:"assistant".into(), text:t});
                    app.scroll = 0;
                }
                UiEvent::Agent(AgentEvent::Error(e)) => {
                    app.sessions[app.current].messages.push(Msg{role:"error".into(), text:e});
                }
                UiEvent::Done(hist) => {
                    app.busy = false; app.status = "idle".into();
                    app.sessions[app.current].history = hist;
                    if app.sessions[app.current].title == "New session" {
                        if let Some(m) = app.sessions[app.current].messages.iter().find(|m| m.role=="user") {
                            app.sessions[app.current].title = m.text.chars().take(40).collect();
                        }
                    }
                    save_sessions(&app.sessions);
                }
            }
        }

        terminal.draw(|f| {
            let root = Layout::default().direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(5), Constraint::Length(3), Constraint::Length(1)])
                .split(f.area());
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let header = Line::from(vec![
                Span::styled(" opencode-native ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" "),
                Span::styled(app.sessions[app.current].title.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled(cwd.display().to_string(), Style::default().fg(Color::DarkGray)),
            ]);
            f.render_widget(Paragraph::new(header), root[0]);

            let mut lines: Vec<Line> = Vec::new();
            for m in &app.sessions[app.current].messages {
                let (label, color) = match m.role.as_str() {
                    "user" => ("You", Color::Cyan),
                    "assistant" => ("OpenCode Native", Color::Green),
                    "tool" => ("Tool", Color::Magenta),
                    "error" => ("Error", Color::Red),
                    _ => ("System", Color::Yellow),
                };
                lines.push(Line::from(Span::styled(label, Style::default().fg(color).add_modifier(Modifier::BOLD))));
                for l in m.text.lines() { lines.push(Line::from(Span::raw(format!("  {l}")))); }
                lines.push(Line::from(""));
            }
            let msgw = Paragraph::new(Text::from(lines))
                .block(Block::default().borders(Borders::ALL).title(" Messages "))
                .wrap(Wrap { trim: false }).scroll((app.scroll, 0));
            f.render_widget(msgw, root[1]);

            let prompt_title = if app.busy { " Prompt (running...) " } else { " Prompt — Enter send, Ctrl+P commands " };
            f.render_widget(Paragraph::new(app.input.as_str()).wrap(Wrap{trim:false})
                .block(Block::default().borders(Borders::ALL).title(prompt_title)), root[2]);

            let foot = Line::from(vec![
                Span::styled(&app.status, Style::default().fg(Color::Yellow)),
                Span::raw(" | "),
                Span::styled(&app.cfg.model.model, Style::default().fg(Color::DarkGray)),
                Span::raw(" | Ctrl+P commands  Ctrl+N new  Ctrl+L sessions  Ctrl+C quit"),
            ]);
            f.render_widget(Paragraph::new(foot), root[3]);

            if app.show_help {
                let area = centered_rect(70, 60, f.area());
                f.render_widget(Clear, area);
                let t = "opencode-native TUI\n\nEnter        send prompt\nCtrl+P       command palette\nCtrl+N       new session\nCtrl+L       sessions list\nCtrl+G       help\nPgUp/PgDn    scroll\nEsc/Ctrl+C   quit / close overlay\n\nThis is a lightweight native OpenCode-style client.\nNot full OpenCode parity yet (no server/MCP/plugins/diff/permissions UI).\nUse official OpenCode for full features.";
                f.render_widget(Paragraph::new(t).wrap(Wrap{trim:false}).block(Block::default().borders(Borders::ALL).title(" Help ")), area);
            }
            if app.show_palette {
                let area = centered_rect(60, 50, f.area());
                f.render_widget(Clear, area);
                let items: Vec<ListItem> = commands.iter().enumerate().map(|(i,(id,desc))| {
                    let st = if i==app.palette_sel { Style::default().fg(Color::Black).bg(Color::Green) } else { Style::default() };
                    ListItem::new(Line::from(vec![Span::styled(format!("{id:<15}"), st.add_modifier(Modifier::BOLD)), Span::styled(*desc, st)]))
                }).collect();
                f.render_widget(List::new(items).block(Block::default().borders(Borders::ALL).title(" Commands ")), area);
            }
            if app.show_sessions {
                let area = centered_rect(70, 60, f.area());
                f.render_widget(Clear, area);
                let items: Vec<ListItem> = app.sessions.iter().enumerate().map(|(i,s)| {
                    let st = if i==app.session_sel { Style::default().fg(Color::Black).bg(Color::Cyan) } else { Style::default() };
                    let cur = if i==app.current { "*" } else { " " };
                    ListItem::new(Line::from(Span::styled(format!("{cur} {}", s.title), st)))
                }).collect();
                f.render_widget(List::new(items).block(Block::default().borders(Borders::ALL).title(" Sessions (Enter switch, Del delete, Esc close) ")), area);
            }
        })?;

        if event::poll(Duration::from_millis(80))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                if app.show_palette {
                    match key.code {
                        KeyCode::Esc => app.show_palette = false,
                        KeyCode::Up => app.palette_sel = app.palette_sel.saturating_sub(1),
                        KeyCode::Down => app.palette_sel = (app.palette_sel+1).min(commands.len()-1),
                        KeyCode::Enter => {
                            let cmd = commands[app.palette_sel].0;
                            app.show_palette = false;
                            match cmd {
                                "session.new" => { app.sessions.insert(0, new_session()); app.current=0; save_sessions(&app.sessions); }
                                "session.list" => { app.show_sessions=true; app.session_sel=app.current; }
                                "model.info" => { app.sessions[app.current].messages.push(Msg{role:"sys".into(), text:format!("provider={} model={} base_url={} key_env={}", app.cfg.model.provider, app.cfg.model.model, app.cfg.model.base_url, app.cfg.model.api_key_env)}); }
                                "help.show" => app.show_help = true,
                                "session.clear" => { app.sessions[app.current].messages.clear(); }
                                "app.quit" => { save_sessions(&app.sessions); return Ok(()); }
                                _ => {}
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
                        KeyCode::Down => app.session_sel = (app.session_sel+1).min(app.sessions.len().saturating_sub(1)),
                        KeyCode::Enter => { app.current = app.session_sel; app.show_sessions=false; }
                        KeyCode::Delete => {
                            if app.sessions.len() > 1 {
                                app.sessions.remove(app.session_sel);
                                if app.current >= app.sessions.len() { app.current = app.sessions.len()-1; }
                                if app.session_sel >= app.sessions.len() { app.session_sel = app.sessions.len()-1; }
                                save_sessions(&app.sessions);
                            }
                        }
                        _ => {}
                    }
                    continue;
                }
                if app.show_help {
                    if key.code == KeyCode::Esc || key.code == KeyCode::Enter { app.show_help=false; }
                    continue;
                }
                match (key.modifiers, key.code) {
                    (KeyModifiers::CONTROL, KeyCode::Char('c')) => { save_sessions(&app.sessions); return Ok(()); }
                    (KeyModifiers::CONTROL, KeyCode::Char('p')) => { app.show_palette=true; app.palette_sel=0; }
                    (KeyModifiers::CONTROL, KeyCode::Char('n')) => { app.sessions.insert(0,new_session()); app.current=0; save_sessions(&app.sessions); }
                    (KeyModifiers::CONTROL, KeyCode::Char('l')) => { app.show_sessions=true; app.session_sel=app.current; }
                    (KeyModifiers::CONTROL, KeyCode::Char('g')) => { app.show_help=true; }
                    (_, KeyCode::Esc) => { save_sessions(&app.sessions); return Ok(()); }
                    (_, KeyCode::PageUp) => app.scroll = app.scroll.saturating_add(5),
                    (_, KeyCode::PageDown) => app.scroll = app.scroll.saturating_sub(5),
                    (_, KeyCode::Enter) => {
                        let text = app.input.trim().to_string();
                        if text.is_empty() || app.busy { continue; }
                        app.input.clear();
                        if text == "/quit" { save_sessions(&app.sessions); return Ok(()); }
                        if text == "/new" { app.sessions.insert(0,new_session()); app.current=0; save_sessions(&app.sessions); continue; }
                        if text == "/help" { app.show_help=true; continue; }
                        if text == "/sessions" { app.show_sessions=true; app.session_sel=app.current; continue; }
                        if text == "/model" { app.sessions[app.current].messages.push(Msg{role:"sys".into(), text:format!("{} @ {}", app.cfg.model.model, app.cfg.model.base_url)}); continue; }
                        app.sessions[app.current].messages.push(Msg{role:"user".into(), text:text.clone()});
                        app.busy=true; app.status="queued".into();
                        let cfg2 = app.cfg.clone();
                        let mut hist2 = app.sessions[app.current].history.clone();
                        let tx2 = tx.clone();
                        let text2 = text.clone();
                        tokio::spawn(async move {
                            let atx = tx2.clone();
                            let (etx, erx) = mpsc::channel::<AgentEvent>();
                            let atx2 = atx.clone();
                            // bridge agent events
                            std::thread::spawn(move || { while let Ok(ev)=erx.recv() { let _ = atx2.send(UiEvent::Agent(ev)); } });
                            let res = agent::chat_turn(&cfg2, &mut hist2, &text2, Some(etx)).await;
                            if let Err(e) = res { let _ = atx.send(UiEvent::Agent(AgentEvent::Error(format!("{e:#}")))); }
                            let _ = atx.send(UiEvent::Done(hist2));
                        });
                    }
                    (_, KeyCode::Backspace) => { app.input.pop(); }
                    (_, KeyCode::Char(c)) => {
                        if !key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT) { app.input.push(c); }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn centered_rect(px: u16, py: u16, r: Rect) -> Rect {
    let v = Layout::default().direction(Direction::Vertical)
        .constraints([Constraint::Percentage((100-py)/2), Constraint::Percentage(py), Constraint::Percentage((100-py)/2)]).split(r);
    Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Percentage((100-px)/2), Constraint::Percentage(px), Constraint::Percentage((100-px)/2)]).split(v[1])[1]
}
