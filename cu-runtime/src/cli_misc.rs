use std::{path::PathBuf, time::Duration};
fn run(program: PathBuf, args: Vec<String>) -> Result<(i32, String), String> {
    cu_tools::execute(
        &program,
        &cu_tools::Invocation { args, stdin: None },
        Duration::from_secs(5),
    )
    .map_err(|e| e.to_string())
}
fn native(args: Vec<String>) -> Result<(i32, String), String> {
    run(
        std::env::var_os("CU_NATIVE")
            .map(PathBuf::from)
            .unwrap_or_else(|| cu_tools::sibling("cu-native")),
        args,
    )
}
pub fn dispatch(raw: &[String]) -> Option<Result<(i32, String), String>> {
    let a = if raw.first().is_some_and(|s| s == "--json") {
        &raw[1..]
    } else {
        raw
    };
    let cmd = a.first().map(String::as_str).unwrap_or("help");
    if ![
        "help",
        "-h",
        "--help",
        "version",
        "-v",
        "--version",
        "apps",
        "log",
        "clean",
        "doctor",
    ]
    .contains(&cmd)
    {
        return None;
    }
    Some((|| {
        let base = std::env::var_os("CU_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cu")
            });
        let shots = base.join("shots");
        let text = match cmd {
            "version" | "-v" | "--version" => format!("cu {}", env!("CARGO_PKG_VERSION")),
            "help" | "-h" | "--help" => include_str!("../help.txt").into(),
            "apps" => {
                let script = std::env::var_os("CU_OSASCRIPT")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("/usr/bin/osascript"));
                let (code,text)=run(script,vec!["-e".into(),"tell application \"System Events\" to get name of every process whose background only is false".into()])?;
                if code == 0 {
                    text
                } else {
                    let (code, rows) = native(vec!["windows".into()])?;
                    if code != 0 {
                        return Err(rows);
                    }
                    let mut names = vec![];
                    for row in rows.lines() {
                        if let Some(name) = row.split('\t').nth(6)
                            && !names.contains(&name)
                        {
                            names.push(name);
                        }
                    }
                    names.join("\n")
                }
            }
            "log" => {
                let count = a
                    .get(1)
                    .map(|s| s.parse::<usize>())
                    .transpose()
                    .map_err(|_| "log count must be a positive integer")?
                    .unwrap_or(25)
                    .min(10000);
                match std::fs::read_to_string(base.join("log")) {
                    Ok(text) => {
                        let rows: Vec<_> = text.lines().collect();
                        rows[rows.len().saturating_sub(count)..].join("\n")
                    }
                    Err(_) => "(nothing logged yet)".into(),
                }
            }
            "clean" => {
                if shots.is_symlink() {
                    return Err("Screenshot storage must not be a symlink".into());
                }
                let mut count = 0;
                for entry in std::fs::read_dir(&shots).map_err(|e| e.to_string())? {
                    let entry = entry.map_err(|e| e.to_string())?;
                    let path = entry.path();
                    if path.extension().is_some_and(|s| s == "png")
                        && entry.file_type().map_err(|e| e.to_string())?.is_file()
                    {
                        std::fs::remove_file(path).map_err(|e| e.to_string())?;
                        count += 1;
                    }
                }
                format!("removed {count} screenshot(s) from {}", shots.display())
            }
            "doctor" => {
                let input = std::env::var_os("CLICLICK")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| cu_tools::sibling("cu-input"));
                let (code, version) = run(input.clone(), vec!["-V".into()])?;
                let (ax, _) = native(vec!["ax-status".into()])?;
                let (_, point) = run(input, vec!["p".into()])?;
                format!(
                    "input    : {}\nnative AX: {}\ncursor   : {}\ncapture  : not tested (native checks do not capture your screen)\n",
                    if code == 0 { version } else { "MISSING".into() },
                    if ax == 0 {
                        "OK"
                    } else {
                        "unavailable — grant Accessibility"
                    },
                    point
                )
            }
            _ => unreachable!(),
        };
        Ok((0, text))
    })())
}
