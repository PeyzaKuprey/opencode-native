use anyhow::Result;
use serde_json::Value;
use std::sync::mpsc::{self, Sender};

use crate::config::{resolve_api_key, Config};
use crate::llm::{ChatMessage, LlmClient};
use crate::tools;

#[derive(Debug, Clone)]
pub enum AgentEvent {
    Status(String),
    ToolCall { name: String, args: String },
    ToolResult { name: String, output: String },
    AssistantDelta(String),
    Assistant(String),
    Error(String),
    PermissionRequest { id: String, name: String, args: String },
}

#[derive(Debug, Clone)]
pub enum AgentControl {
    Permission { id: String, allow: bool },
}

pub async fn run_once(cfg: &Config, prompt: &str) -> Result<String> {
    let mut history = Vec::new();
    chat_turn(cfg, &mut history, prompt, None, None).await
}

pub async fn chat_turn(
    cfg: &Config,
    history: &mut Vec<ChatMessage>,
    user_input: &str,
    events: Option<Sender<AgentEvent>>,
    control: Option<mpsc::Receiver<AgentControl>>,
) -> Result<String> {
    let send = |ev: AgentEvent| {
        if let Some(tx) = &events {
            let _ = tx.send(ev);
        }
    };

    send(AgentEvent::Status("connecting".into()));
    let api_key = resolve_api_key(cfg)?;
    let client = LlmClient::new(cfg.model.base_url.clone(), api_key, cfg.model.model.clone())?;
    let tool_specs = tools::specs();

    let mut messages: Vec<ChatMessage> = Vec::new();
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: Some(cfg.agent.system_prompt.clone()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });
    messages.extend(history.clone());
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: Some(user_input.to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });

    let mut steps = 0usize;
    loop {
        steps += 1;
        if steps > cfg.agent.max_steps {
            anyhow::bail!("max_steps exceeded ({})", cfg.agent.max_steps);
        }

        send(AgentEvent::Status(format!("thinking ({steps})")));
        let msg = if let Some(tx) = &events {
            let (dtx, drx) = mpsc::channel::<String>();
            let tx2 = tx.clone();
            std::thread::spawn(move || {
                while let Ok(delta) = drx.recv() {
                    let _ = tx2.send(AgentEvent::AssistantDelta(delta));
                }
            });
            match client.chat_stream(&messages, &tool_specs, &dtx).await {
                Ok(m) => m,
                Err(_e) => client.chat(&messages, &tool_specs).await?,
            }
        } else {
            client.chat(&messages, &tool_specs).await?
        };
        let tool_calls = msg.tool_calls.clone().unwrap_or_default();
        if !tool_calls.is_empty() {
            messages.push(msg.clone());
            for tc in tool_calls {
                let args_str = tc.function.arguments.clone();
                let risk = tools::risk_for(&tc.function.name);
                let mut allowed = matches!(risk, tools::Risk::Safe);
                if !allowed {
                    if let (Some(tx), Some(rx)) = (&events, &control) {
                        let _ = tx.send(AgentEvent::PermissionRequest { id: tc.id.clone(), name: tc.function.name.clone(), args: args_str.clone() });
                        send(AgentEvent::Status(format!("awaiting permission: {}", tc.function.name)));
                        while let Ok(ctrl) = rx.recv() {
                            match ctrl {
                                AgentControl::Permission { id, allow } if id == tc.id => { allowed = allow; break; }
                                _ => {}
                            }
                        }
                    } else {
                        // non-interactive mode: allow by default for now
                        allowed = true;
                    }
                }
                send(AgentEvent::ToolCall { name: tc.function.name.clone(), args: args_str.clone() });
                let result = if !allowed {
                    "tool denied by user".to_string()
                } else {
                    let args: Value = serde_json::from_str(&args_str)
                        .unwrap_or_else(|_| Value::Object(Default::default()));
                    match tools::execute(&tc.function.name, &args) {
                        Ok(out) => out,
                        Err(e) => format!("tool error: {e:#}"),
                    }
                };
                send(AgentEvent::ToolResult { name: tc.function.name.clone(), output: result.clone() });
                messages.push(ChatMessage {
                    role: "tool".to_string(),
                    content: Some(result),
                    tool_calls: None,
                    tool_call_id: Some(tc.id),
                    name: Some(tc.function.name),
                });
            }
            continue;
        }

        let final_text = msg.content.clone().unwrap_or_default();
        history.push(ChatMessage {
            role: "user".to_string(),
            content: Some(user_input.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
        history.push(ChatMessage {
            role: "assistant".to_string(),
            content: Some(final_text.clone()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });
        send(AgentEvent::Assistant(final_text.clone()));
        return Ok(final_text);
    }
}
