use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Terminal,
};
use std::io;
use std::sync::mpsc;
use std::time::Duration;

use crate::agent;
use crate::config::Config;
use crate::llm::ChatMessage;

enum AgentMsg {
    Done(String),
    Error(String),
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

async fn app_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, cfg: Config) -> Result<()> {
    let mut input = String::new();
    let mut messages: Vec<(String, String)> = vec![(
        "system".into(),
        "OpenCode Native TUI. Enter=send, Esc/Ctrl+C=quit, Ctrl+L=clear.".into(),
    )];
    let mut history: Vec<ChatMessage> = Vec::new();
    let mut status = "ready".to_string();
    let mut busy = false;
    let (tx, rx) = mpsc::channel::<AgentMsg>();

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3), Constraint::Length(1)])
                .split(f.area());

            let items: Vec<ListItem> = messages
                .iter()
                .flat_map(|(role, text)| {
                    let (label, color) = match role.as_str() {
                        "user" => ("you", Color::Cyan),
                        "assistant" => ("ocn", Color::Green),
                        "error" => ("err", Color::Red),
                        _ => ("sys", Color::Yellow),
                    };
                    let mut lines = vec![ListItem::new(Line::from(vec![
                        Span::styled(format!("{label}: "), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                    ]))];
                    for l in text.lines() {
                        lines.push(ListItem::new(Line::from(Span::raw(format!("  {l}")))));
                    }
                    lines.push(ListItem::new(Line::from("")));
                    lines
                })
                .collect();
            let msg_list = List::new(items).block(Block::default().borders(Borders::ALL).title(" OpenCode Native "));
            f.render_widget(msg_list, chunks[0]);

            let input_widget = Paragraph::new(input.as_str())
                .style(Style::default().fg(Color::White))
                .wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::ALL).title(if busy { " Input (busy...) " } else { " Input " }));
            f.render_widget(input_widget, chunks[1]);

            let status_widget = Paragraph::new(Line::from(vec![
                Span::styled("status: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&status),
                Span::styled(" | model: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&cfg.model.model),
            ]));
            f.render_widget(status_widget, chunks[2]);
        })?;

        // drain agent messages
        while let Ok(msg) = rx.try_recv() {
            busy = false;
            match msg {
                AgentMsg::Done(text) => {
                    status = "ready".to_string();
                    messages.push(("assistant".into(), text));
                }
                AgentMsg::Error(e) => {
                    status = "error".to_string();
                    messages.push(("error".into(), e));
                }
            }
        }

        if event::poll(Duration::from_millis(80))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                match (key.modifiers, key.code) {
                    (_, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Ok(()),
                    (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
                        messages.clear();
                        messages.push(("system".into(), "Cleared.".into()));
                    }
                    (_, KeyCode::Enter) => {
                        let text = input.trim().to_string();
                        if text.is_empty() || busy { continue; }
                        input.clear();
                        messages.push(("user".into(), text.clone()));
                        status = "thinking...".to_string();
                        busy = true;
                        let cfg2 = cfg.clone();
                        let mut hist2 = history.clone();
                        let tx2 = tx.clone();
                        let text2 = text.clone();
                        tokio::spawn(async move {
                            let res = agent::chat_turn(&cfg2, &mut hist2, &text2).await;
                            match res {
                                Ok(t) => { let _ = tx2.send(AgentMsg::Done(t)); }
                                Err(e) => { let _ = tx2.send(AgentMsg::Error(format!("{e:#}"))); }
                            }
                        });
                        history.push(ChatMessage { role: "user".into(), content: Some(text), tool_calls: None, tool_call_id: None, name: None });
                    }
                    (_, KeyCode::Backspace) => { input.pop(); }
                    (_, KeyCode::Char(c)) => {
                        if !key.modifiers.contains(KeyModifiers::CONTROL) {
                            input.push(c);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
