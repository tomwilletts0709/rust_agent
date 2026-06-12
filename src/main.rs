use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MODEL: &str = "claude-sonnet-4-6";
const MAX_TOKENS: u32 = 4096;
const MAX_TURNS: usize = 6;
const MAX_FILE_CHARS: usize = 4_000;

const SYSTEM_PROMPT: &str = "\
You are Nova, a careful research assistant who answers questions \
about a Rust project. When the user asks for something that lives in \
a file, call the `read_file` tool. Only call tools when needed. After \
a tool result, answer the user directly in plain prose.";

#[derive(Debug, Serialize, Clone)]
struct Tool {
    name: &'static str,
    description: &'static str,
    input_schema: serde_json::Value,
}

#[derive(Debug, Serialize, Clone)]
struct Message {
    role: &'static str,
    content: Vec<Block>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Block {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        is_error: bool,
    },
}

#[derive(Debug, Serialize)]
struct Request<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    tools: &'a [Tool],
    messages: &'a [Message],
}

#[derive(Debug, Deserialize)]
struct Response {
    content: Vec<Block>,
    stop_reason: String,
}

#[derive(Debug, Deserialize)]
struct ReadFileArgs {
    path: String,
}

fn sandbox_root() -> Result<PathBuf> {
    Ok(std::env::current_dir()?.canonicalize()?)
}

fn read_file(args: ReadFileArgs) -> Result<String> {
    let root = sandbox_root()?;
    let resolved = root.join(Path::new(&args.path));
    let canonical = resolved
        .canonicalize()
        .map_err(|e| anyhow!("cannot resolve {}: {e}", args.path))?;
    if !canonical.starts_with(&root) {
        bail!("path {} escapes the sandbox", args.path);
    }
    let bytes = std::fs::read(&canonical)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(text.chars().take(MAX_FILE_CHARS).collect())
}

fn run_tool(name: &str, input: &serde_json::Value) -> Result<String> {
    match name {
        "read_file" => {
            let args: ReadFileArgs = serde_json::from_value(input.clone())?;
            read_file(args)
        }
        other => Err(anyhow!("unknown tool: {other}")),
    }
}

fn read_file_definition() -> Tool {
    Tool {
        name: "read_file",
        description: "Read a UTF-8 text file from the current project",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to tbe project root."
                }
            },
            "required": ["path"]
        }),
    }
}

async fn send(
    http: &reqwest::Client,
    api_key: &str,
    tools: &[Tool],
    messages: &[Message],
) -> Result<Response> {
    let body = Request {
        model: MODEL,
        max_tokens: MAX_TOKENS,
        system: SYSTEM_PROMPT,
        tools,
        messages,
    };
    let response = http
        .post(ANTHROPIC_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        bail!("anthropic API error {status}: {text}");
    }
    Ok(response.json().await?)
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let api_key = std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY not set")?;
    let user_question = std::env::args().skip(1).collect::<Vec<String>>().join(" ");
    if user_question.trim().is_empty() {
        bail!("please provide a question");
    }
    let http = reqwest::Client::new();
    let tools = vec![read_file_definition()];

    let mut messages: Vec<Message> = vec![Message {
        role: "user",
        content: vec![Block::Text {
            text: user_question,
        }],
    }];

    for _ in 0..MAX_TURNS {
        let response = send(&http, &api_key, &tools, &messages).await?;

        messages.push(Message {
            role: "assistant",
            content: response.content.clone(),
        });

        let tool_uses: Vec<(&str, &str, &serde_json::Value)> = response
            .content
            .iter()
            .filter_map(|block| match block {
                Block::ToolUse { id, name, input } => Some((id.as_str(), name.as_str(), input)),
                _ => None,
            })
            .collect();

        if tool_uses.is_empty() || response.stop_reason != "tool_use" {
            for block in &response.content {
                if let Block::Text { text } = block {
                    println!("{text}");
                }
            }
            return Ok(());
        }

        let results: Vec<Block> = tool_uses
            .iter()
            .map(|(id, name, input)| match run_tool(name, input) {
                Ok(content) => Block::ToolResult {
                    tool_use_id: (*id).to_string(),
                    content,
                    is_error: false,
                },
                Err(e) => Block::ToolResult {
                    tool_use_id: (*id).to_string(),
                    content: format!("ERROR: {e}"),
                    is_error: true,
                },
            })
            .collect();

        messages.push(Message {
            role: "user",
            content: results,
        });
    }

    bail!("agent did not converge in {MAX_TURNS} turns");
}
