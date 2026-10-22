use serde_json::json;
use std::{
    io::{Read, Write},
    path::PathBuf,
    time::Duration,
};
fn invoke(args: Vec<String>) -> Result<String, String> {
    let program = std::env::var_os("CLICLICK")
        .map(PathBuf::from)
        .unwrap_or_else(|| cu_tools::sibling("cu-input"));
    let (code, text) = cu_tools::execute(
        &program,
        &cu_tools::Invocation { args, stdin: None },
        Duration::from_secs(65),
    )
    .map_err(|e| e.to_string())?;
    if code != 0 {
        Err("CU input failed".into())
    } else {
        Ok(text)
    }
}
fn key(s: &str) -> &str {
    match s {
        "backspace" => "delete",
        "escape" => "esc",
        _ => s,
    }
}
fn number(s: &str) -> bool {
    s.parse::<f64>().is_ok_and(f64::is_finite)
}
fn log(message: &str) {
    let base = std::env::var_os("CU_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cu"));
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(base.join("log"))
    {
        let _ = writeln!(file, "{message}");
    }
}
pub fn dispatch(raw: &[String]) -> Option<Result<(i32, String), String>> {
    let json_mode = raw.first().is_some_and(|s| s == "--json");
    let args = if json_mode { &raw[1..] } else { raw };
    let cmd = args.first()?.as_str();
    let a = &args[1..];
    if ![
        "type", "paste", "key", "combo", "move", "drag", "pos", "color",
    ]
    .contains(&cmd)
    {
        return None;
    }
    Some((|| {
        let message = match cmd {
            "type" => {
                let mut secret = false;
                let mut stdin = false;
                let mut paced = false;
                let mut interval = std::env::var("CU_TYPE_INTERVAL_MS").unwrap_or("180".into());
                let mut text = None;
                let mut i = 0;
                while i < a.len() {
                    match a[i].as_str() {
                        "--secret" => secret = true,
                        "--stdin" => stdin = true,
                        "--paced" => paced = true,
                        "--interval" => {
                            i += 1;
                            interval = a.get(i).ok_or("--interval requires milliseconds")?.clone();
                        }
                        _ => {
                            if text.is_some() {
                                return Err("Provide only one text argument".into());
                            }
                            text = Some(a[i].clone());
                        }
                    }
                    i += 1;
                }
                if stdin {
                    if text.is_some() {
                        return Err("do not combine --stdin with inline text".into());
                    }
                    let mut bytes = Vec::new();
                    std::io::stdin()
                        .take(1_048_577)
                        .read_to_end(&mut bytes)
                        .map_err(|e| e.to_string())?;
                    if bytes.len() > 1_048_576 {
                        return Err("Text exceeds 1 MiB".into());
                    }
                    text = Some(String::from_utf8(bytes).map_err(|_| "Text must be UTF-8")?);
                }
                let text = text.ok_or("usage: cu type TEXT or --stdin")?;
                if text.contains('\0') {
                    return Err("Text contains NUL".into());
                }
                let delay = if paced {
                    interval
                        .parse::<u64>()
                        .map_err(|_| "type interval must be a non-negative integer")?
                } else {
                    0
                };
                if stdin || secret {
                    eprintln!(
                        "cu: warning: typed text is not written to cu logs; prefer --stdin for secrets to avoid shell history"
                    );
                }
                let mut input = vec![];
                if paced {
                    let count = text.chars().count();
                    for (idx, c) in text.chars().enumerate() {
                        input.push(if c == '\n' {
                            "kp:return".into()
                        } else {
                            format!("t:{c}")
                        });
                        if idx + 1 < count && delay > 0 {
                            input.push(format!("w:{delay}"));
                        }
                    }
                } else {
                    for (idx, s) in text.split('\n').enumerate() {
                        if idx > 0 {
                            input.push("kp:return".into());
                        }
                        if !s.is_empty() {
                            input.push(format!("t:{s}"));
                        }
                    }
                }
                if !input.is_empty() {
                    invoke(input)?;
                }
                let count = text.chars().count();
                log(&format!("type {count} chars"));
                if json_mode {
                    let mut v = json!({"ok":true,"typed_chars":count});
                    if paced {
                        v["paced"] = json!(true);
                    }
                    v.to_string()
                } else if paced {
                    format!("typed {count} chars (paced {delay}ms)")
                } else {
                    format!("typed {count} chars")
                }
            }
            "paste" => {
                if a != ["--stdin"] {
                    return Err("usage: cu paste --stdin".into());
                }
                let mut text = String::new();
                std::io::stdin()
                    .take(1_048_577)
                    .read_to_string(&mut text)
                    .map_err(|e| e.to_string())?;
                if text.len() > 1_048_576 {
                    return Err("Text exceeds 1 MiB".into());
                }
                let mut file = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
                file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
                let native = std::env::var_os("CU_NATIVE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| cu_tools::sibling("cu-native"));
                let (code, out) = cu_tools::execute(
                    &native,
                    &cu_tools::Invocation {
                        args: vec!["paste".into(), file.path().to_string_lossy().into_owned()],
                        stdin: None,
                    },
                    Duration::from_secs(5),
                )
                .map_err(|e| e.to_string())?;
                if code != 0 {
                    return Err(out);
                }
                let n = text.chars().count();
                log(&format!("paste {n} chars"));
                if json_mode {
                    json!({"ok":true,"pasted_chars":n}).to_string()
                } else {
                    format!("pasted {n} chars")
                }
            }
            "key" => {
                if a.len() != 1 || a[0].is_empty() {
                    return Err("usage: cu key KEY".into());
                }
                invoke(vec![format!("kp:{}", key(&a[0]))])?;
                log(&format!("key {}", a[0]));
                format!("pressed {}", a[0])
            }
            "combo" => {
                if a.len() < 2 {
                    return Err("usage: cu combo MODIFIER... KEY".into());
                }
                let mut input: Vec<_> =
                    a[..a.len() - 1].iter().map(|s| format!("kd:{s}")).collect();
                let k = key(a.last().unwrap());
                input.push(format!(
                    "{}:{k}",
                    if k.chars().count() == 1 { "t" } else { "kp" }
                ));
                input.extend(a[..a.len() - 1].iter().map(|s| format!("ku:{s}")));
                invoke(input)?;
                log(&format!("combo {}", a.join(" ")));
                format!("combo: {}", a.join(" "))
            }
            "pos" => invoke(vec!["p".into()])?,
            "move" | "color" => {
                if a.len() != 2 || !a.iter().all(|s| number(s)) {
                    return Err("usage: cu move/color X Y".into());
                }
                let out = invoke(vec![format!(
                    "{}:{},{}",
                    if cmd == "move" { "m" } else { "cp" },
                    a[0],
                    a[1]
                )])?;
                if cmd == "color" {
                    out
                } else {
                    log(&format!("move {},{}", a[0], a[1]));
                    format!("moved to {},{}", a[0], a[1])
                }
            }
            "drag" => {
                if a.len() != 4 || !a.iter().all(|s| number(s)) {
                    return Err("usage: cu drag X1 Y1 X2 Y2".into());
                }
                let delay = std::env::var("CU_CLICK_SETTLE_MS")
                    .unwrap_or("35".into())
                    .parse::<u64>()
                    .map_err(|_| "Invalid settle milliseconds")?;
                invoke(vec![
                    format!("m:{},{}", a[0], a[1]),
                    format!("w:{delay}"),
                    format!("dd:{},{}", a[0], a[1]),
                    "w:80".into(),
                    format!("dm:{},{}", a[2], a[3]),
                    "w:80".into(),
                    format!("du:{},{}", a[2], a[3]),
                ])?;
                format!("dragged {},{} -> {},{}", a[0], a[1], a[2], a[3])
            }
            _ => unreachable!(),
        };
        Ok((0, message))
    })())
}
