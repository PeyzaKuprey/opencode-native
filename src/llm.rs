use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::mpsc::Sender;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunctionSpec,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolFunctionSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Option<Vec<StreamChoice>>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Option<StreamDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct StreamToolCall {
    index: usize,
    id: Option<String>,
    function: Option<StreamToolFn>,
}

#[derive(Debug, Deserialize)]
struct StreamToolFn {
    name: Option<String>,
    arguments: Option<String>,
}

pub struct LlmClient {
    http: Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl LlmClient {
    pub fn new(base_url: String, api_key: String, model: String) -> Result<Self> {
        let http = Client::builder()
            .user_agent("opencode-native/0.1")
            .build()
            .context("build http client")?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
        })
    }

    pub async fn chat(&self, messages: &[ChatMessage], tools: &[ToolSpec]) -> Result<ChatMessage> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "temperature": 0.2,
        });
        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools)?;
            body["tool_choice"] = Value::String("auto".to_string());
        }

        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("send chat request")?;
        let status = resp.status();
        let text = resp.text().await.context("read chat response")?;
        if !status.is_success() {
            anyhow::bail!("LLM API error {}: {}", status, truncate(&text, 800));
        }
        let parsed: ChatResponse = serde_json::from_str(&text)
            .with_context(|| format!("parse chat response: {}", truncate(&text, 400)))?;
        let msg = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .context("chat response missing choices[0].message")?;
        Ok(msg)
    }

    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolSpec],
        on_delta: &Sender<String>,
    ) -> Result<ChatMessage> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "temperature": 0.2,
            "stream": true,
        });
        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools)?;
            body["tool_choice"] = Value::String("auto".to_string());
        }

        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("send chat request")?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("LLM API error {}: {}", status, truncate(&text, 800));
        }

        let mut content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut buf = String::new();
        let mut stream = resp.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("read stream chunk")?;
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buf.find('\n') {
                let mut line = buf[..pos].to_string();
                buf = buf[pos + 1..].to_string();
                if line.ends_with('\r') { line.pop(); }
                let line = line.trim();
                if line.is_empty() { continue; }
                if !line.starts_with("data:") { continue; }
                let data = line.trim_start_matches("data:").trim();
                if data == "[DONE]" {
                    let msg = ChatMessage {
                        role: "assistant".into(),
                        content: if content.is_empty() { None } else { Some(content.clone()) },
                        tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls.clone()) },
                        tool_call_id: None,
                        name: None,
                    };
                    return Ok(msg);
                }
                let Ok(parsed) = serde_json::from_str::<StreamChunk>(data) else { continue; };
                let Some(choices) = parsed.choices else { continue; };
                for c in choices {
                    if let Some(delta) = c.delta {
                        if let Some(t) = delta.content {
                            if !t.is_empty() {
                                content.push_str(&t);
                                let _ = on_delta.send(t);
                            }
                        }
                        if let Some(tcs) = delta.tool_calls {
                            for tc in tcs {
                                let idx = tc.index;
                                if tool_calls.len() <= idx {
                                    tool_calls.resize(idx + 1, ToolCall {
                                        id: String::new(),
                                        kind: "function".into(),
                                        function: ToolFunctionCall { name: String::new(), arguments: String::new() },
                                    });
                                }
                                if let Some(id) = tc.id { tool_calls[idx].id = id; }
                                if let Some(f) = tc.function {
                                    if let Some(n) = f.name { tool_calls[idx].function.name.push_str(&n); }
                                    if let Some(a) = f.arguments { tool_calls[idx].function.arguments.push_str(&a); }
                                }
                            }
                        }
                    }
                    if c.finish_reason.is_some() {}
                }
            }
        }

        Ok(ChatMessage {
            role: "assistant".into(),
            content: if content.is_empty() { None } else { Some(content) },
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            tool_call_id: None,
            name: None,
        })
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}
