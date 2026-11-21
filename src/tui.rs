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
use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use crate::agent::{self, AgentEvent};
use crate::config::Config;
use crate::llm::ChatMessage;

#[derive(Clone)]
struct Msg {
    role: String,
    text: String,
}

pub async fn run(cfg: Config) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let res = app(&mut terminal, cfg).await;
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

async fn app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, cfg: Config) -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut input = String::new();
    let mut messages: Vec<Msg> = vec![Msg {
        role: "sys".into(),
        text: "OpenCode Native TUI. Ctrl+P commands, Enter send, Ctrl+C quit, Ctrl+L clear, Ctrl+N new session.".into(),
    }];
    let mut history: Vec<ChatMessage> = Vec::new();
    let mut status = "idle".to_string();
    let mut busy = false;
    let mut scroll: u16 = 0;
    let mut show_help = false;
    let mut show_commands = false;
    let mut cmd_sel: usize = 0;
    let commands = vec![
        ("new", "New session / clear history"),
        ("clear", "Clear screen messages"),
        ("help", "Toggle help"),
        ("model", "Show model/config"),
        ("quit", "Exit"),
    ];
    let (tx, rx) = mpsc::channel::<AgentEvent>();

    loop {
        // drain events
        while let Ok(ev) = rx.try_recv() {
            match ev {
                AgentEvent::Status(s) => status = s,
                AgentEvent::ToolCall { name, args } => {
                    let short = args.chars().take(120).collect::<String>();
                    messages.push(Msg { role: "tool".into(), text: format!("{name} {short}") });
                }
                AgentEvent::ToolResult { name, output } => {
                    let short = output.lines().take(6).collect::<Vec<_>>().join("\n");
                    messages.push(Msg { role: "tool".into(), text: format!("{name} -> {short}") });
                }
                AgentEvent::Assistant(t) => {
                    busy = false;
                    status = "idle".into();
                    messages.push(Msg { role: "assistant".into(), text: t });
                    scroll = 0;
                }
                AgentEvent::Error(e) => {
                    busy = false;
                    status = "error".into();
                    messages.push(Msg { role: "error".into(), text: e });
                }
            }
        }

        terminal.draw(|f| {
            let root = Layout::default().direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(5), Constraint::Length(3), Constraint::Length(1)])
                .split(f.area());

            // header
            let title = Line::from(vec![
                Span::styled(" OpenCode Native ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled(cwd.display().to_string(), Style::default().fg(Color::Gray)),
            ]);
            f.render_widget(Paragraph::new(title), root[0]);

            // messages
            let mut lines: Vec<Line> = Vec::new();
            for m in &messages {
                let (label, color) = match m.role.as_str() {
                    "user" => ("You", Color::Cyan),
                    "assistant" => ("Assistant", Color::Green),
                    "tool" => ("Tool", Color::Magenta),
                    "error" => ("Error", Color::Red),
                    _ => ("System", Color::Yellow),
                };
                lines.push(Line::from(Span::styled(format!("{label}"), Style::default().fg(color).add_modifier(Modifier::BOLD))));
                for l in m.text.lines() {
                    lines.push(Line::from(Span::raw(l.to_string())));
                }
                lines.push(Line::from(""));
            }
            let msg = Paragraph::new(Text::from(lines))
                .block(Block::default().borders(Borders::ALL).title(" Session "))
                .wrap(Wrap { trim: false })
                .scroll((scroll, 0));
            f.render_widget(msg, root[1]);

            // input
            let input_title = if busy { " Prompt (busy) " } else { " Prompt " };
            let input_w = Paragraph::new(input.as_str())
                .block(Block::default().borders(Borders::ALL).title(input_title))
                .wrap(Wrap { trim: false });
            f.render_widget(input_w, root[2]);

            // footer
            let footer = Line::from(vec![
                Span::styled(" Ctrl+P ", Style::default().fg(Color::DarkGray)),
                Span::raw("commands "),
                Span::styled(" Enter ", Style::default().fg(Color::DarkGray)),
                Span::raw("send "),
                Span::styled(" Ctrl+N ", Style::default().fg(Color::DarkGray)),
                Span::raw("new "),
                Span::styled(" Ctrl+C ", Style::default().fg(Color::DarkGray)),
                Span::raw("quit "),
                Span::styled(format!(" | {} | {} ", status, cfg.model.model), Style::default().fg(Color::DarkGray)),
            ]);
            f.render_widget(Paragraph::new(footer), root[3]);

            if show_help {
                let area = centered_rect(70, 60, f.area());
                f.render_widget(Clear, area);
                let help = "OpenCode Native TUI (MVP)\n\nEnter: send prompt\nCtrl+P: command palette\nCtrl+N: new session\nCtrl+L: clear messages\nCtrl+C / Esc: quit (Esc closes palettes first)\nPgUp/PgDn: scroll session\n\nThis is a lightweight native TUI, not full OpenCode parity.\nUse `opencode-native run/chat/config` for non-TUI modes.";
                f.render_widget(Paragraph::new(help).wrap(Wrap { trim: false }).block(Block::default().borders(Borders::ALL).title(" Help ")), area);
            }
            if show_commands {
                let area = centered_rect(60, 50, f.area());
                f.render_widget(Clear, area);
                let items: Vec<ListItem> = commands.iter().enumerate().map(|(i,(n,d))| {
                    let style = if i==cmd_sel { Style::default().fg(Color::Black).bg(Color::Cyan) } else { Style::default() };
                    ListItem::new(Line::from(vec![Span::styled(format!("{n:<8}"), style.add_modifier(Modifier::BOLD)), Span::styled(format!(" {d}"), style)]))
                }).collect();
                f.render_widget(List::new(items).block(Block::default().borders(Borders::ALL).title(" Commands ")), area);
            }
        })?;

        if event::poll(Duration::from_millis(80))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                if show_commands {
                    match key.code {
                        KeyCode::Esc => show_commands = false,
                        KeyCode::Up => cmd_sel = cmd_sel.saturating_sub(1),
                        KeyCode::Down => cmd_sel = (cmd_sel + 1).min(commands.len()-1),
                        KeyCode::Enter => {
                            let cmd = commands[cmd_sel].0;
                            show_commands = false;
                            match cmd {
                                "new" => { history.clear(); messages.clear(); messages.push(Msg{role:"sys".into(),text:"New session.".into()}); status="idle".into(); }
                                "clear" => { messages.clear(); }
                                "help" => { show_help = !show_help; }
                                "model" => { messages.push(Msg{role:"sys".into(), text: format!("model={} base_url={} api_key_env={}", cfg.model.model, cfg.model.base_url, cfg.model.api_key_env)}); }
                                "quit" => return Ok(()),
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                    continue;
                }
                if show_help && key.code == KeyCode::Esc { show_help = false; continue; }

                match (key.modifiers, key.code) {
                    (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Ok(()),
                    (KeyModifiers::CONTROL, KeyCode::Char('p')) => { show_commands = true; cmd_sel = 0; }
                    (KeyModifiers::CONTROL, KeyCode::Char('l')) => { messages.clear(); }
                    (KeyModifiers::CONTROL, KeyCode::Char('n')) => { history.clear(); messages.clear(); messages.push(Msg{role:"sys".into(),text:"New session.".into()}); }
                    (_, KeyCode::Esc) => return Ok(()),
                    (_, KeyCode::PageUp) => { scroll = scroll.saturating_add(5); }
                    (_, KeyCode::PageDown) => { scroll = scroll.saturating_sub(5); }
                    (_, KeyCode::Enter) => {
                        let text = input.trim().to_string();
                        if text.is_empty() || busy { continue; }
                        input.clear();
                        messages.push(Msg { role: "user".into(), text: text.clone() });
                        busy = true; status = "queued".into();
                        let cfg2 = cfg.clone();
                        let mut hist2 = history.clone();
                        let tx2 = tx.clone();
                        let text2 = text.clone();
                        tokio::spawn(async move {
                            match agent::chat_turn(&cfg2, &mut hist2, &text2, Some(tx2.clone())).await {
                                Ok(_) => {}
                                Err(e) => { let _ = tx2.send(AgentEvent::Error(format!("{e:#}"))); }
                            }
                        });
                        history.push(ChatMessage { role:"user".into(), content:Some(text), tool_calls:None, tool_call_id:None, name:None });
                    }
                    (_, KeyCode::Backspace) => { input.pop(); }
                    (_, KeyCode::Char(c)) => {
                        if !key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT) { input.push(c); }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default().direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ]).split(r);
    Layout::default().direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ]).split(popup_layout[1])[1]
}
