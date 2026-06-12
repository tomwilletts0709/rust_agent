# Nova Rust Agent

Nova is a minimal Rust CLI agent that can answer questions about the current project using Anthropic's Messages API and a local `read_file` tool.

## What it does

- Sends a user question to Anthropic.
- Exposes a local `read_file` tool to the model.
- Lets Rust execute approved local tool calls.
- Keeps file access sandboxed to the current terminal directory.
- Returns the model's final answer in the CLI.

## Setup

Create a `.env` file outside version control:

```env
ANTHROPIC_API_KEY=your_key_here
```

The `.gitignore` excludes `.env` files so secrets are not committed.

## Run

From the crate directory:

```bash
cargo run -- "What edition does Cargo.toml use?"
```

The sandbox root is the directory you run the binary from. For this project, run commands from the same directory as `Cargo.toml`.

## Current tools

### `read_file`

Reads a UTF-8 file from the current project directory. The path is canonicalized and rejected if it escapes the sandbox root.

## Roadmap

Possible next tools:

- `list_files` to inspect project structure.
- `search_text` to search source files.
- approval-gated `cargo_check`.
- approval-gated file editing.
