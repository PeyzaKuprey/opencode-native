use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
            anyhow::bail!("LLM API error {}: {}", status, text);
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
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}
