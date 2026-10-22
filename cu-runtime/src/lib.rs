#[path = "../../shared/input_lease.rs"]
pub mod input_lease;
use serde_json::{Value, json};
use std::{
    io::{Read, Seek, Write},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const MAX_OUTPUT: usize = 2 * 1024 * 1024;
pub fn tools() -> Value {
    serde_json::from_str(include_str!("../tools.json")).expect("embedded tool schema")
}
pub fn sibling(name: &str) -> PathBuf {
    let executable = std::env::current_exe().unwrap_or_default();
    resolved_sibling(&executable, name)
}
fn resolved_sibling(executable: &std::path::Path, name: &str) -> PathBuf {
    executable
        .canonicalize()
        .unwrap_or_else(|_| executable.to_path_buf())
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join(name)
}
pub fn cu_binary() -> PathBuf {
    std::env::var_os("CU_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let local = sibling("cu");
            if local.is_file() {
                local
            } else {
                PathBuf::from("cu")
            }
        })
}
pub fn validate(name: &str, arguments: &Value) -> Result<(), String> {
    let all = tools();
    let tool = all
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == name)
        .ok_or("Unknown tool")?;
    let args = arguments.as_object().ok_or("Arguments must be an object")?;
    let schema = &tool["inputSchema"];
    let props = schema["properties"].as_object().unwrap();
    if let Some(required) = schema["required"].as_array() {
        for key in required {
            if !args.contains_key(key.as_str().unwrap()) {
                return Err("Missing required argument".into());
            }
        }
    }
    for (key, val) in args {
        let prop = props.get(key).ok_or("Unsupported argument")?;
        let valid = match prop["type"].as_str().unwrap_or("") {
            "string" => val.as_str().is_some_and(|s| {
                !s.contains('\0')
                    && s.chars().count() <= prop["maxLength"].as_u64().unwrap_or(1024) as usize
            }),
            "boolean" => val.is_boolean(),
            "integer" => (val.is_i64() || val.is_u64()) && in_range(val, prop),
            "number" => val.is_number() && in_range(val, prop),
            _ => false,
        };
        if !valid
            || prop["enum"]
                .as_array()
                .is_some_and(|items| !items.contains(val))
        {
            return Err(format!("Invalid argument: {key}"));
        }
    }
    if name == "cu_click"
        && arguments["window_relative"] == true
        && arguments["app"].as_str().is_none_or(str::is_empty)
    {
        return Err("window_relative requires app".into());
    }
    Ok(())
}
fn in_range(val: &Value, prop: &Value) -> bool {
    val.as_f64().is_some_and(|v| {
        v.is_finite()
            && v >= prop["minimum"].as_f64().unwrap_or(-1e9)
            && v <= prop["maximum"].as_f64().unwrap_or(1e9)
    })
}
#[derive(Debug, PartialEq)]
pub struct Invocation {
    pub args: Vec<String>,
    pub stdin: Option<String>,
}
pub fn invocation(name: &str, a: &Value) -> Result<Invocation, String> {
    validate(name, a)?;
    let text = |key: &str| a[key].as_str().unwrap_or("").to_owned();
    let mut args: Vec<String> = vec!["--json".into()];
    let mut input = None;
    match name {
        "cu_observe" | "cu_audit" | "cu_read" => {
            args.extend([name[3..].into(), text("app")]);
            if name == "cu_read" && a.get("url").is_some() {
                args.extend(["--url".into(), text("url")]);
            }
        }
        "cu_act" => args.extend([
            "act".into(),
            text("element"),
            "--snapshot".into(),
            text("snapshot"),
        ]),
        "cu_wait" => {
            let mode = text("mode");
            args.extend(["wait".into(), format!("--{mode}"), text("app")]);
            if ["window", "element", "value"].contains(&mode.as_str()) {
                args.push(text("target"));
            }
        }
        "cu_batch" => {
            args.extend(["batch".into(), "--stdin".into()]);
            input = Some(text("script"));
        }
        "cu_shot" => {
            args = vec!["shot".into()];
            if !text("app").is_empty() {
                args.extend(["-w".into(), text("app")]);
            }
            if !text("name").is_empty() {
                args.push(text("name"));
            }
        }
        "cu_click" => {
            args.push("click".into());
            if !text("app").is_empty() {
                args.extend([
                    if a["window_relative"] == true {
                        "--window"
                    } else {
                        "--app"
                    }
                    .into(),
                    text("app"),
                ]);
            }
            args.extend([a["x"].to_string(), a["y"].to_string()]);
        }
        "cu_select_file" => args.extend([
            "select-file".into(),
            text("app"),
            "--pid".into(),
            a["pid"].to_string(),
            "--window-id".into(),
            a["window_id"].to_string(),
            "--path".into(),
            text("path"),
            "--expected-source".into(),
            text("expected_source"),
        ]),
        "cu_clickel" => args.extend(["clickel".into(), text("app"), text("name")]),
        "cu_type" => {
            args.extend(["type".into(), "--stdin".into()]);
            input = Some(text("text"));
        }
        _ => return Err("Unknown tool".into()),
    }
    for (key, flag) in [
        ("background", "--background"),
        ("native_only", "--native-only"),
        ("draft_values", "--draft-values"),
        ("all", "--all"),
        ("gone", "--gone"),
        ("secret", "--secret"),
        ("paced", "--paced"),
    ] {
        if a[key] == true {
            args.push(flag.into());
        }
    }
    if a["verify"] == false {
        args.push("--no-verify".into());
    }
    for (key, flag) in [
        ("since", "--since"),
        ("role", "--role"),
        ("max_elements", "--max-elements"),
        ("max_issues", "--max-issues"),
        ("timeout_ms", "--timeout"),
        ("index", "--index"),
        ("interval_ms", "--interval"),
    ] {
        if !a[key].is_null() {
            args.extend([
                flag.into(),
                a[key]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| a[key].to_string()),
            ]);
        }
    }
    if name == "cu_observe" && !text("name").is_empty() {
        args.extend(["--name".into(), text("name")]);
    }
    Ok(Invocation { args, stdin: input })
}
pub fn execute(
    program: &std::path::Path,
    call: &Invocation,
    timeout: Duration,
) -> Result<(i32, String), std::io::Error> {
    let mut source = tempfile::tempfile()?;
    source.write_all(call.stdin.as_deref().unwrap_or("").as_bytes())?;
    source.rewind()?;
    let mut output = tempfile::tempfile()?;
    let mut command = Command::new(program);
    command
        .args(&call.args)
        .stdin(Stdio::from(source))
        .stdout(output.try_clone()?)
        .stderr(output.try_clone()?)
        .process_group(0);
    input_lease::delegate_current(&mut command).map_err(std::io::Error::other)?;
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            return Ok(failure(
                127,
                "cli_unavailable",
                "CU executable could not be started.",
            ));
        }
    };
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        let oversized = output.metadata()?.len() > MAX_OUTPUT as u64;
        if oversized || start.elapsed() >= timeout {
            // The child owns a new process group, so helpers cannot survive a timeout.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            return Ok(failure(
                124,
                if oversized {
                    "output_limit"
                } else {
                    "cli_timeout"
                },
                "Execution stopped. Inspect current state before retrying; an action may already have completed.",
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    output.rewind()?;
    let mut bytes = Vec::new();
    output
        .take((MAX_OUTPUT + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_OUTPUT {
        return Ok(failure(
            1,
            "output_limit",
            "Output exceeded the limit. Narrow the observation; actions were not retried.",
        ));
    }
    Ok((
        status.code().unwrap_or(1),
        String::from_utf8_lossy(&bytes).trim().to_owned(),
    ))
}
fn failure(code: i32, kind: &str, message: &str) -> (i32, String) {
    (
        code,
        json!({"ok":false,"error":{"code":kind,"message":message}}).to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn document_selector_is_forwarded_without_shell_interpretation() {
        let call = invocation(
            "cu_read",
            &json!({"app":"Chrome","url":"https://example.com/a?x=$(date)"}),
        )
        .unwrap();
        assert_eq!(
            call.args,
            [
                "--json",
                "read",
                "Chrome",
                "--url",
                "https://example.com/a?x=$(date)"
            ]
        );
    }
    #[test]
    fn helpers_follow_executable_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        let links = root.path().join("links");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&links).unwrap();
        std::fs::write(real.join("cu"), "fixture").unwrap();
        std::os::unix::fs::symlink(real.join("cu"), links.join("cu")).unwrap();
        assert_eq!(
            resolved_sibling(&links.join("cu"), "cu-input"),
            real.canonicalize().unwrap().join("cu-input")
        );
    }
    #[test]
    fn all_tools_have_equivalent_argv() {
        for (name, args, expected) in [
            (
                "cu_observe",
                json!({"app":"Safari","native_only":true,"background":true,"max_elements":12}),
                vec![
                    "--json",
                    "observe",
                    "Safari",
                    "--background",
                    "--native-only",
                    "--max-elements",
                    "12",
                ],
            ),
            (
                "cu_act",
                json!({"element":"e_1","snapshot":"s_1","verify":false}),
                vec!["--json", "act", "e_1", "--snapshot", "s_1", "--no-verify"],
            ),
            (
                "cu_audit",
                json!({"app":"Notes"}),
                vec!["--json", "audit", "Notes"],
            ),
            (
                "cu_wait",
                json!({"app":"Notes","mode":"window","target":"Example"}),
                vec!["--json", "wait", "--window", "Notes", "Example"],
            ),
            (
                "cu_batch",
                json!({"script":"observe Notes"}),
                vec!["--json", "batch", "--stdin"],
            ),
            (
                "cu_shot",
                json!({"app":"Notes","name":"demo"}),
                vec!["shot", "-w", "Notes", "demo"],
            ),
            (
                "cu_click",
                json!({"x":1,"y":2,"app":"Notes","window_relative":true}),
                vec!["--json", "click", "--window", "Notes", "1", "2"],
            ),
            (
                "cu_clickel",
                json!({"app":"Notes","name":"New"}),
                vec!["--json", "clickel", "Notes", "New"],
            ),
            (
                "cu_type",
                json!({"text":"secret","secret":true}),
                vec!["--json", "type", "--stdin", "--secret"],
            ),
            (
                "cu_read",
                json!({"app":"Safari"}),
                vec!["--json", "read", "Safari"],
            ),
        ] {
            assert_eq!(invocation(name, &args).unwrap().args, expected, "{name}");
        }
        assert_eq!(
            invocation("cu_type", &json!({"text":"--secret private"}))
                .unwrap()
                .stdin,
            Some("--secret private".into())
        );
    }
    #[test]
    fn rejects_bad_input() {
        for (name, args) in [
            ("cu_observe", json!({"app":"A","max_elements":true})),
            ("cu_wait", json!({"app":"A","mode":"shell"})),
            ("cu_click", json!({"x":0,"y":0,"window_relative":true})),
            ("cu_type", json!({"text":"a\u{0000}"})),
            ("cu_read", json!({"app":"A","unknown":true})),
        ] {
            assert!(invocation(name, &args).is_err());
        }
    }
    #[test]
    fn process_timeout_and_output_limits() {
        let call = Invocation {
            args: vec!["-c".into(), "sleep 5".into()],
            stdin: None,
        };
        let start = Instant::now();
        let (code, text) = execute(
            std::path::Path::new("/bin/sh"),
            &call,
            Duration::from_millis(25),
        )
        .unwrap();
        assert_eq!(code, 124);
        assert!(text.contains("cli_timeout"));
        assert!(start.elapsed() < Duration::from_secs(1));
        let call = Invocation {
            args: vec![],
            stdin: Some("x".repeat(MAX_OUTPUT + 10)),
        };
        let (_, text) = execute(
            std::path::Path::new("/bin/cat"),
            &call,
            Duration::from_secs(1),
        )
        .unwrap();
        assert!(text.contains("output_limit"));
    }
}

#[path = "../../shared/teams_identity.rs"]
pub mod teams_identity;
