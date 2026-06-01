use crate::store::Store;
use crate::{summary, time};
use anyhow::Result;
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

pub fn serve_stdio(store: Store) -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let request: Value = serde_json::from_str(&line)?;
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            continue;
        };
        let id = request.get("id").cloned();

        if id.is_none() {
            continue;
        }

        let response = match method {
            "initialize" => response(
                id,
                json!({
                    "protocolVersion": request
                        .get("params")
                        .and_then(|params| params.get("protocolVersion"))
                        .cloned()
                        .unwrap_or_else(|| json!("2025-06-18")),
                    "serverInfo": {
                        "name": "runaware",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "capabilities": {
                        "tools": {}
                    }
                }),
            ),
            "tools/list" => response(id, json!({ "tools": tools() })),
            "tools/call" => {
                let result = call_tool(&store, request.get("params").unwrap_or(&Value::Null));
                match result {
                    Ok(value) => response(id, value),
                    Err(err) => error_response(id, -32603, &err.to_string()),
                }
            }
            _ => error_response(id, -32601, "method not found"),
        };

        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }

    Ok(())
}

fn tools() -> Vec<Value> {
    vec![
        tool(
            "runaware_list_sources",
            "List known local runtime sources.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "runaware_latest_errors",
            "Return recent extracted errors and warnings.",
            json!({
                "type": "object",
                "properties": {
                    "since": { "type": "string", "description": "Relative time such as 10m, 2h, 1d, or RFC3339.", "default": "10m" },
                    "source": { "type": "string" },
                    "limit": { "type": "integer", "default": 20 }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "runaware_summarize_runtime",
            "Summarize runtime state, likely root cause, and cross-source hints.",
            json!({
                "type": "object",
                "properties": {
                    "since": { "type": "string", "default": "10m" },
                    "source": { "type": "string" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "runaware_search_logs",
            "Search recent redacted runtime logs.",
            json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string" },
                    "since": { "type": "string", "default": "30m" },
                    "source": { "type": "string" },
                    "limit": { "type": "integer", "default": 50 }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "runaware_logs_around",
            "Return redacted logs around a specific extracted error id.",
            json!({
                "type": "object",
                "required": ["error_id"],
                "properties": {
                    "error_id": { "type": "string" },
                    "seconds": { "type": "integer", "default": 10 },
                    "limit": { "type": "integer", "default": 100 }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "runaware_create_checkpoint",
            "Create a named debugging checkpoint.",
            json!({
                "type": "object",
                "required": ["name"],
                "properties": { "name": { "type": "string" } },
                "additionalProperties": false
            }),
        ),
        tool(
            "runaware_diff_since_checkpoint",
            "Summarize errors and warnings since a checkpoint id or name.",
            json!({
                "type": "object",
                "required": ["checkpoint"],
                "properties": {
                    "checkpoint": { "type": "string" },
                    "source": { "type": "string" }
                },
                "additionalProperties": false
            }),
        ),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

fn call_tool(store: &Store, params: &Value) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing tool name"))?;
    let args = params.get("arguments").unwrap_or(&Value::Null);

    let payload = match name {
        "runaware_list_sources" => json!(store.list_sources()?),
        "runaware_latest_errors" => {
            let since = time::parse_since(str_arg(args, "since", "10m"))?;
            let source = optional_str(args, "source");
            let limit = int_arg(args, "limit", 20);
            json!(store.error_blocks_since(since, source, limit, true)?)
        }
        "runaware_summarize_runtime" => {
            let since = time::parse_since(str_arg(args, "since", "10m"))?;
            let source = optional_str(args, "source");
            json!(summary::summarize(store, since, source, true)?)
        }
        "runaware_search_logs" => {
            let query = str_arg(args, "query", "");
            let since = time::parse_since(str_arg(args, "since", "30m"))?;
            let source = optional_str(args, "source");
            let limit = int_arg(args, "limit", 50);
            json!(store.search_logs(query, since, source, limit, true)?)
        }
        "runaware_logs_around" => {
            let error_id = str_arg(args, "error_id", "");
            let seconds = int_arg(args, "seconds", 10) as i64;
            let limit = int_arg(args, "limit", 100);
            json!(store.logs_around_error(error_id, seconds, limit)?)
        }
        "runaware_create_checkpoint" => {
            let name = str_arg(args, "name", "");
            json!(store.create_checkpoint(name)?)
        }
        "runaware_diff_since_checkpoint" => {
            let checkpoint = store.find_checkpoint(str_arg(args, "checkpoint", ""))?;
            let source = optional_str(args, "source");
            json!(summary::summarize(
                store,
                chrono::DateTime::parse_from_rfc3339(&checkpoint.ts)?.with_timezone(&chrono::Utc),
                source,
                false
            )?)
        }
        _ => anyhow::bail!("unknown tool '{name}'"),
    };

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&payload)?
        }]
    }))
}

fn str_arg<'a>(value: &'a Value, key: &str, default: &'a str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or(default)
}

fn optional_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn int_arg(value: &Value, key: &str, default: usize) -> usize {
    value
        .get(key)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(default)
}

fn response(id: Option<Value>, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn error_response(id: Option<Value>, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}
