use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};
fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn handle(raw: &str) -> Option<Value> {
    let request: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => return Some(error(Value::Null, -32700, "Parse error")),
    };
    let Some(method) = request["method"].as_str() else {
        return Some(error(Value::Null, -32600, "Invalid request"));
    };
    let id = request["id"].clone();
    if id.is_null() {
        return None;
    }
    let result = match method {
        "initialize" => {
            json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"cu","version":env!("CARGO_PKG_VERSION")}})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools":cu_tools::tools()}),
        "tools/call" => {
            let args = request["params"]
                .get("arguments")
                .filter(|a| !a.is_null())
                .cloned()
                .unwrap_or(json!({}));
            let call =
                match cu_tools::invocation(request["params"]["name"].as_str().unwrap_or(""), &args)
                {
                    Ok(v) => v,
                    Err(e) => return Some(error(id, -32602, &e)),
                };
            let (code, text) = match cu_tools::execute(
                &cu_tools::cu_binary(),
                &call,
                std::time::Duration::from_secs(65),
            ) {
                Ok(v) => v,
                Err(_) => return Some(error(id, -32603, "Tool execution failed")),
            };
            let mut result = json!({"content":[{"type":"text","text":text}],"isError":code!=0});
            if let Ok(value) = serde_json::from_str::<Value>(&text)
                && value.is_object()
            {
                result["structuredContent"] = value;
            }
            result
        }
        _ => return Some(error(id, -32601, "Method not found")),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}
fn main() {
    let mut input = io::stdin().lock();
    let mut out = io::stdout().lock();
    loop {
        let mut bytes = Vec::new();
        match input.by_ref().take(262145).read_until(b'\n', &mut bytes) {
            Ok(0) | Err(_) => break,
            _ => {}
        }
        if bytes.len() > 262144 {
            let _ = writeln!(out, "{}", error(Value::Null, -32600, "Request too large"));
            let _ = out.flush();
            break;
        }
        let raw = String::from_utf8_lossy(&bytes);
        if raw.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle(&raw)
            && writeln!(out, "{response}")
                .and_then(|_| out.flush())
                .is_err()
        {
            break;
        }
    }
}
