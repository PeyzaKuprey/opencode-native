use anyhow::{Context, Result};
use glob::glob;
use regex::RegexBuilder;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::Command;
use walkdir::WalkDir;

use crate::llm::ToolSpec;

pub fn specs() -> Vec<ToolSpec> {
    vec![
        spec("shell", "Run a shell command in the current working directory", json!({
            "type":"object","properties":{"command":{"type":"string"}}, "required":["command"]
        })),
        spec("read", "Read a UTF-8 text file", json!({
            "type":"object","properties":{"path":{"type":"string"}}, "required":["path"]
        })),
        spec("write", "Write UTF-8 text to a file (overwrites)", json!({
            "type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}}, "required":["path","content"]
        })),
        spec("edit", "Replace first occurrence of old with new in a file", json!({
            "type":"object","properties":{"path":{"type":"string"},"old":{"type":"string"},"new":{"type":"string"}}, "required":["path","old","new"]
        })),
        spec("glob", "Find files matching a glob pattern", json!({
            "type":"object","properties":{"pattern":{"type":"string"}}, "required":["pattern"]
        })),
        spec("grep", "Regex search in files under a path", json!({
            "type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"}}, "required":["pattern"]
        })),
    ]
}

fn spec(name: &str, description: &str, parameters: Value) -> ToolSpec {
    ToolSpec {
        kind: "function".to_string(),
        function: crate::llm::ToolFunctionSpec {
            name: name.to_string(),
            description: description.to_string(),
            parameters,
        },
    }
}

pub fn execute(name: &str, args: &Value) -> Result<String> {
    match name {
        "shell" => tool_shell(args),
        "read" => tool_read(args),
        "write" => tool_write(args),
        "edit" => tool_edit(args),
        "glob" => tool_glob(args),
        "grep" => tool_grep(args),
        _ => anyhow::bail!("unknown tool: {name}"),
    }
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .with_context(|| format!("missing string arg: {key}"))
}

fn tool_shell(args: &Value) -> Result<String> {
    let command = arg_str(args, "command")?;
    let out = Command::new("sh")
        .arg("-lc")
        .arg(command)
        .output()
        .with_context(|| format!("run shell command: {command}"))?;
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&out.stdout));
    if !out.stderr.is_empty() {
        if !text.is_empty() { text.push_str("\n"); }
        text.push_str("[stderr]\n");
        text.push_str(&String::from_utf8_lossy(&out.stderr));
    }
    text.push_str(&format!("\n[exit_code={}]", out.status.code().unwrap_or(-1)));
    Ok(truncate(&text, 12000))
}

fn tool_read(args: &Value) -> Result<String> {
    let path = arg_str(args, "path")?;
    let data = fs::read_to_string(path).with_context(|| format!("read file: {path}"))?;
    Ok(truncate(&data, 12000))
}

fn tool_write(args: &Value) -> Result<String> {
    let path = arg_str(args, "path")?;
    let content = arg_str(args, "content")?;
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("create dir {}", parent.display()))?;
        }
    }
    fs::write(path, content).with_context(|| format!("write file: {path}"))?;
    Ok(format!("wrote {} bytes to {}", content.as_bytes().len(), path))
}

fn tool_edit(args: &Value) -> Result<String> {
    let path = arg_str(args, "path")?;
    let old = arg_str(args, "old")?;
    let new = arg_str(args, "new")?;
    let data = fs::read_to_string(path).with_context(|| format!("read file: {path}"))?;
    if !data.contains(old) {
        anyhow::bail!("old string not found in {path}");
    }
    let updated = data.replacen(old, new, 1);
    fs::write(path, &updated).with_context(|| format!("write file: {path}"))?;
    Ok(format!("edited {}", path))
}

fn tool_glob(args: &Value) -> Result<String> {
    let pattern = arg_str(args, "pattern")?;
    let mut out = Vec::new();
    for entry in glob(pattern).with_context(|| format!("invalid glob: {pattern}"))? {
        let p = entry?;
        out.push(p.display().to_string());
        if out.len() >= 200 { break; }
    }
    Ok(out.join("\n"))
}

fn tool_grep(args: &Value) -> Result<String> {
    let pattern = arg_str(args, "pattern")?;
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let re = RegexBuilder::new(pattern).case_insensitive(true).build().context("invalid regex")?;
    let mut out = String::new();
    let mut count = 0;
    for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() { continue; }
        let p = entry.path();
        let Ok(data) = fs::read_to_string(p) else { continue; };
        for (i, line) in data.lines().enumerate() {
            if re.is_match(line) {
                out.push_str(&format!("{}:{}:{}\n", p.display(), i + 1, line));
                count += 1;
                if count >= 200 { return Ok(truncate(&out, 12000)); }
            }
        }
    }
    if out.is_empty() { Ok("no matches".to_string()) } else { Ok(truncate(&out, 12000)) }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("{}...\n[truncated]", &s[..max]) }
}
