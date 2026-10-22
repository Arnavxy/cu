//! Native pointer/semantic commands with guarded OCR only where requested fallback permits it.
use serde_json::json;
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
type Result<T> = std::result::Result<T, String>;
fn program(key: &str, name: &str) -> PathBuf {
    std::env::var_os(key).map(PathBuf::from).unwrap_or_else(|| {
        if name.starts_with('/') {
            PathBuf::from(name)
        } else {
            cu_tools::sibling(name)
        }
    })
}
fn invoke(path: &Path, args: Vec<String>, stdin: Option<String>) -> Result<String> {
    let (code, out) = cu_tools::execute(
        path,
        &cu_tools::Invocation { args, stdin },
        Duration::from_secs(15),
    )
    .map_err(|e| e.to_string())?;
    if code == 0 {
        Ok(out.trim_end().into())
    } else {
        Err(out.trim_end().into())
    }
}
fn native(args: Vec<String>) -> Result<String> {
    invoke(&program("CU_NATIVE", "cu-native"), args, None)
}
fn osa(script: String) -> Result<String> {
    invoke(
        &program("CU_OSASCRIPT", "/usr/bin/osascript"),
        vec![],
        Some(script),
    )
}
fn log(message: &str) {
    let dir = std::env::var_os("CU_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cu"));
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("log"))
    {
        let _ = writeln!(f, "{message}");
    }
}
fn escaped(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}
fn point(s: &str) -> Result<f64> {
    let unsigned = s.strip_prefix('-').unwrap_or(s);
    let mut parts = unsigned.split('.');
    let first = parts.next().unwrap_or("");
    let second = parts.next();
    if first.is_empty()
        || !first.bytes().all(|b| b.is_ascii_digit())
        || second.is_some_and(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()))
        || parts.next().is_some()
    {
        return Err("coordinates must be numeric logical points".into());
    }
    let v = s
        .parse::<f64>()
        .map_err(|_| "coordinates must be numeric logical points")?;
    if !v.is_finite() || v.abs() > 100000. {
        return Err("coordinates must be numeric logical points within 100000".into());
    }
    Ok(v)
}
fn click(a: &[String], cmd: &str) -> Result<String> {
    let mut app = "";
    let mut mode = "";
    let mut pos = Vec::new();
    let mut i = 0;
    while i < a.len() {
        match a[i].as_str() {
            "--app" | "--window" => {
                if !mode.is_empty() {
                    return Err(format!("invalid or repeated pointer target '{}'", a[i]));
                }
                mode = &a[i];
                i += 1;
                app = a.get(i).ok_or("pointer target requires an app")?;
                if app.is_empty() {
                    return Err("pointer target requires an app".into());
                }
            }
            s if s.starts_with("--") => return Err(format!("unknown click option '{s}'")),
            s => pos.push(s),
        }
        i += 1;
    }
    if pos.len() != 2 {
        return Err("usage: cu click [--window|--app] APP X Y | cu click X Y".into());
    }
    let (mut x, mut y) = (point(pos[0])?, point(pos[1])?);
    let wait = std::env::var("CU_CLICK_SETTLE_MS")
        .unwrap_or("35".into())
        .parse::<u32>()
        .map_err(|_| "CU_CLICK_SETTLE_MS must be a non-negative integer")?;
    if wait > 10000 {
        return Err("CU_CLICK_SETTLE_MS exceeds 10000".into());
    }
    if !app.is_empty() {
        if mode == "--app" {
            let front=std::env::var("CU_FRONTMOST_APP").unwrap_or_else(|_|osa("tell application \"System Events\" to get name of first process whose frontmost is true".into()).unwrap_or_default());
            if front.to_lowercase() != app.to_lowercase() {
                return Err(format!(
                    "'{app}' is not frontmost (frontmost: '{front}'); use cu open '{app}' or --window to activate"
                ));
            }
        } else {
            crate::cli_media::activate(app).map_err(|_| format!("could not activate '{app}'"))?;
        }
        let b = crate::cli_media::bounds(app)
            .map_err(|_| format!("no visible window matched '{app}'"))?;
        if mode == "--window" {
            if x < 0. || y < 0. || x > b.w as f64 || y > b.h as f64 {
                return Err("window-relative coordinates are outside the target window".into());
            }
            x += b.x as f64;
            y += b.y as f64;
        } else if x < (b.x as f64)
            || y < (b.y as f64)
            || x > (b.x + b.w) as f64
            || y > (b.y + b.h) as f64
        {
            return Err(format!(
                "coordinates {x},{y} are outside the current '{app}' window; use --window for relative points"
            ));
        }
    }
    let action = match cmd {
        "click" => "c",
        "dclick" => "dc",
        _ => "rc",
    };
    invoke(
        &program("CLICLICK", "cu-input"),
        vec![
            format!("m:{x},{y}"),
            format!("w:{wait}"),
            format!("{action}:{x},{y}"),
        ],
        None,
    )?;
    log(&format!("{cmd} {x},{y}"));
    Ok(format!(
        "{} {x},{y}",
        match cmd {
            "click" => "clicked",
            "dclick" => "double-clicked",
            _ => "right-clicked",
        }
    ))
}
#[derive(Clone, Debug)]
struct Element {
    role: String,
    name: String,
    x: i64,
    y: i64,
}
fn parse_native(out: &str) -> Vec<Element> {
    out.lines()
        .filter_map(|line| {
            let f: Vec<_> = line.split('\t').collect();
            if f.len() < 7 {
                return None;
            }
            let (x, y, w, h) = (
                f[3].parse::<i64>().ok()?,
                f[4].parse::<i64>().ok()?,
                f[5].parse::<i64>().ok()?,
                f[6].parse::<i64>().ok()?,
            );
            Some(Element {
                role: f[1].into(),
                name: f[2].into(),
                x: x.checked_add(w / 2)?,
                y: y.checked_add(h / 2)?,
            })
        })
        .collect()
}
fn parse_legacy(out: &str) -> Vec<Element> {
    out.lines()
        .filter_map(|line| {
            let f: Vec<_> = line.split('|').map(str::trim).collect();
            if f.len() != 3 {
                return None;
            }
            let (x, y) = f[2].split_once(',')?;
            Some(Element {
                role: f[0].into(),
                name: f[1].into(),
                x: x.parse().ok()?,
                y: y.parse().ok()?,
            })
        })
        .collect()
}
fn interactive(role: &str) -> bool {
    [
        "AXButton",
        "AXRadioButton",
        "AXCheckBox",
        "AXTextField",
        "AXTextArea",
        "AXMenuButton",
        "AXMenuItem",
        "AXPopUpButton",
        "AXLink",
        "AXCell",
        "AXRow",
        "AXComboBox",
        "AXSlider",
        "AXTabGroup",
        "AXDisclosureTriangle",
        "AXOCRText",
    ]
    .iter()
    .any(|prefix| role.starts_with(prefix))
}
fn filtered(elements: Vec<Element>, all: bool, role: &str, filter: &str) -> Vec<Element> {
    let filter = filter.to_lowercase();
    elements
        .into_iter()
        .filter(|e| {
            (all || interactive(&e.role))
                && (role.is_empty() || role == e.role)
                && (filter.is_empty()
                    || format!("{} | {} | {},{}", e.role, e.name, e.x, e.y)
                        .to_lowercase()
                        .contains(&filter))
        })
        .collect()
}
fn ocr_tree(app: &str) -> Result<Vec<Element>> {
    crate::cli_media::activate(app)?;
    let b = crate::cli_media::bounds(app)?;
    let out = crate::cli_media::ocr(b)?;
    let elements: Vec<_> = out
        .lines()
        .filter_map(|line| {
            let f: Vec<_> = line.split('\t').collect();
            if f.len() < 3 {
                return None;
            }
            Some(Element {
                role: "AXOCRText".into(),
                name: f[0].into(),
                x: f[1].parse().ok()?,
                y: f[2].parse().ok()?,
            })
        })
        .collect();
    if elements.is_empty() {
        Err(format!("ERR: OCR found no text in '{app}'"))
    } else {
        Ok(elements)
    }
}
fn bad(out: &str) -> bool {
    out.starts_with("ERR:")
        || out.contains("execution error")
        || out.contains("Connection invalid")
        || out.contains("syntax error")
}
fn tree(a: &[String], json_mode: bool, find: bool) -> Result<(i32, String)> {
    if a.is_empty() || (find && a.len() < 2) {
        return Err(if find {
            "usage: cu find \"App Name\" \"text\""
        } else {
            "usage: cu tree \"App Name\" [filter] [--all]"
        }
        .into());
    }
    let app = &a[0];
    let mut filter = "";
    let mut role = "";
    let mut all = false;
    let mut native_only = false;
    let mut i = 1;
    while i < a.len() {
        match a[i].as_str() {
            "--all" => all = true,
            "--native-only" => native_only = true,
            "--role" => {
                i += 1;
                role = a.get(i).ok_or("--role requires a role")?;
            }
            s => filter = s,
        }
        i += 1;
    }
    let native_result = native(vec!["ax-tree".into(), app.into()]);
    let mut elements = native_result
        .as_ref()
        .map(|s| parse_native(s))
        .unwrap_or_default();
    let mut failure = native_result.err();
    if elements.is_empty() && !native_only {
        let script = TREE_SCRIPT.replace("$app_as", &escaped(app));
        match osa(script) {
            Ok(s) if !bad(&s) => {
                elements = parse_legacy(&s);
                failure = None;
            }
            Ok(s) => failure = Some(s),
            Err(e) => failure = Some(e),
        }
    }
    let mut source = "ax";
    let mut matches = filtered(elements, all, role, filter);
    if native_only && failure.is_some() {
        return Err(
            "native_ax_unavailable: no readable Accessibility content; no screenshot was taken"
                .into(),
        );
    }
    if !native_only
        && (failure.is_some() || matches.is_empty() && (role.is_empty() || role == "AXOCRText"))
    {
        source = "ocr";
        matches = filtered(ocr_tree(app)?, all, role, filter);
    }
    if matches.is_empty() {
        if json_mode {
            return Ok((0, json!({"ok":true,"app":app,"elements":[]}).to_string()));
        }
        return Ok((
            1,
            "(no matching elements — try --all, or a different filter)".into(),
        ));
    }
    log(&format!("tree {app} source={source}"));
    if json_mode {
        Ok((0,json!({"ok":true,"app":app,"source":source,"elements":matches.iter().map(|e|json!({"role":e.role,"name":e.name,"x":e.x,"y":e.y})).collect::<Vec<_>>()} ).to_string()))
    } else {
        Ok((
            0,
            matches
                .iter()
                .map(|e| format!("{} | {} | {},{}", e.role, e.name, e.x, e.y))
                .collect::<Vec<_>>()
                .join("\n"),
        ))
    }
}
fn native_press(app: &str, target: &str, role: &str, index: usize) -> Result<String> {
    let window = native(vec!["bounds".into(), app.into()])
        .map_err(|_| format!("ERR: no native window matched '{app}'"))?;
    let w: Vec<_> = window.split('\t').collect();
    let pid = w.get(1).ok_or("invalid native window PID")?;
    let output = native(vec!["ax-tree".into(), app.into(), "--interactive".into()])?;
    let rows: Vec<_> = output
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|f| f.len() >= 7 && f[2] == target && (role.is_empty() || f[1] == role))
        .collect();
    let count = rows.len();
    if count == 0 {
        return Err(format!("ERR: no native AX element named '{target}'"));
    }
    if index == 0 && count > 1 {
        return Err(format!(
            "ERR: selector matched {count} native AX elements; add --role and/or --index"
        ));
    }
    let selected = rows
        .get(if index == 0 { 0 } else { index - 1 })
        .ok_or_else(|| {
            format!("ERR: selector matched {count} element(s); index {index} is out of range")
        })?;
    let mut args = vec!["ax-perform".into(), pid.to_string()];
    args.extend(selected[..7].iter().map(|s| s.to_string()));
    let result = native(args).map_err(|e| format!("ERR: native action failed: {e}"))?;
    Ok(format!(
        "PRESSED (NativeAX:{result}) {} | {} | matches={count}",
        selected[1], selected[2]
    ))
}
fn clickel(a: &[String], json_mode: bool) -> Result<(i32, String)> {
    if a.len() < 2 {
        return Err(
            "usage: cu clickel \"App Name\" \"Element name\" [--role ROLE] [--index N] [--json]"
                .into(),
        );
    }
    let app = &a[0];
    let target = &a[1];
    let mut role = "";
    let mut index = 0;
    let mut i = 2;
    while i < a.len() {
        match a[i].as_str() {
            "--role" => {
                i += 1;
                role = a.get(i).ok_or("--role requires a role")?;
            }
            "--index" => {
                i += 1;
                index = a
                    .get(i)
                    .ok_or("--index requires a number")?
                    .parse::<usize>()
                    .map_err(|_| "--index must be a non-negative integer")?;
            }
            s => return Err(format!("unknown clickel option '{s}'")),
        }
        i += 1;
    }
    let result = if program("CU_NATIVE", "cu-native").is_file() {
        native_press(app, target, role, index)
    } else {
        let script = PRESS_SCRIPT
            .replace("$app_as", &escaped(app))
            .replace("$target_as", &escaped(target))
            .replace("$role_as", &escaped(role))
            .replace("$wanted_index", &index.to_string());
        osa(script).and_then(|s| {
            if bad(&s) || s.starts_with("FOUND") {
                Err(s)
            } else {
                Ok(s)
            }
        })
    };
    let result = match result {
        Ok(r) => r,
        Err(e) => {
            if e.starts_with("ERR: selector") || e.starts_with("ERR: native action failed") {
                return Err(e);
            }
            if !role.is_empty() && role != "AXOCRText" {
                return Err(format!(
                    "ERR: AX failed and OCR cannot verify role '{role}'; use clicktext or omit --role"
                ));
            }
            let args = vec![
                "clicktext".into(),
                app.clone(),
                target.clone(),
                "--index".into(),
                index.to_string(),
            ];
            let (code, r) = crate::cli_media::dispatch(&args).ok_or("OCR route unavailable")??;
            if code != 0 {
                return Err(r);
            }
            r
        }
    };
    log(&format!("clickel {app} role={role} index={index}"));
    if json_mode {
        Ok((
            0,
            json!({"ok":true,"app":app,"name":target,"role":role,"index":index,"result":result})
                .to_string(),
        ))
    } else {
        Ok((0, result))
    }
}
pub fn dispatch(raw: &[String]) -> Option<Result<(i32, String)>> {
    let json_mode = raw.iter().any(|s| s == "--json");
    let args: Vec<_> = raw
        .iter()
        .filter(|s| s.as_str() != "--json")
        .cloned()
        .collect();
    let cmd = args.first()?.as_str();
    let a = &args[1..];
    Some(match cmd {
        "click" | "dclick" | "rclick" => click(a, cmd).map(|s| (0, s)),
        "tree" | "find" => tree(a, json_mode, cmd == "find"),
        "clickel" => clickel(a, json_mode),
        _ => return None,
    })
}

const TREE_SCRIPT: &str = r####"on walkEl(el, depth)
  set acc to ""
  if depth > 6 then return acc
  tell application "System Events"
    try
      repeat with childRef in (UI elements of el)
        set child to contents of childRef
        try
          set r to (role of child) as text
          set nm to ""
          try
            set nm to (name of child) as text
          end try
          if nm is "missing value" then set nm to ""
          if nm is "" then
            try
              set nm to (description of child) as text
            end try
          end if
          if nm is "missing value" then set nm to ""
          set p to position of child
          set s to size of child
          set acc to acc & r & " | " & nm & " | " & ((item 1 of p) + ((item 1 of s) div 2)) & "," & ((item 2 of p) + ((item 2 of s) div 2)) & linefeed
        end try
        set acc to acc & my walkEl(child, depth + 1)
      end repeat
    end try
  end tell
  return acc
end walkEl

tell application "System Events"
  if not (exists process "$app_as") then return "ERR: process '$app_as' is not running"
end tell
tell application "System Events" to tell process "$app_as" to set frontmost to true
set wc to 0
repeat 10 times
  delay 0.3
  tell application "System Events"
    tell process "$app_as"
      try
        set wc to count of windows
      on error
        set wc to 0
      end try
    end tell
  end tell
  if wc > 0 then exit repeat
end repeat
if wc is 0 then return "ERR: '$app_as' has no window (System Events saw none after 3s)"
tell application "System Events"
  tell process "$app_as"
    set w to front window
  end tell
end tell
return walkEl(w, 0)"####;

const PRESS_SCRIPT: &str = r####"on collectMatches(el, depth, target, wantedRole, found)
  if depth > 6 then return found
  set acc to found
  tell application "System Events"
    try
      repeat with childRef in (UI elements of el)
        set child to contents of childRef
        set nm to ""
        set r to ""
        try
          set nm to (name of child) as text
        end try
        if nm is "missing value" then set nm to ""
        if nm is "" then
          try
            set nm to (description of child) as text
          end try
        end if
        try
          set r to (role of child) as text
        end try
        if nm is target and (wantedRole is "" or r is wantedRole) then set end of acc to child
        set acc to my collectMatches(child, depth + 1, target, wantedRole, acc)
      end repeat
    end try
  end tell
  return acc
end collectMatches

tell application "System Events"
  if not (exists process "$app_as") then return "ERR: process '$app_as' is not running"
end tell
tell application "System Events" to tell process "$app_as" to set frontmost to true
set wc to 0
repeat 10 times
  delay 0.3
  tell application "System Events"
    tell process "$app_as"
      try
        set wc to count of windows
      on error
        set wc to 0
      end try
    end tell
  end tell
  if wc > 0 then exit repeat
end repeat
if wc is 0 then return "ERR: '$app_as' has no window"
tell application "System Events"
  tell process "$app_as"
    set w to front window
  end tell
end tell
set matches to collectMatches(w, 0, "$target_as", "$role_as", {})
set matchCount to count of matches
set chosenIndex to $wanted_index
  if matchCount is 0 then return "ERR: no matching element named '$target_as'"
  if chosenIndex is 0 and matchCount > 1 then return "ERR: selector matched " & matchCount & " elements; add --role and/or --index"
  if chosenIndex is 0 then set chosenIndex to 1
  if chosenIndex < 1 or chosenIndex > matchCount then return "ERR: selector matched " & matchCount & " element(s); index " & chosenIndex & " is out of range"
set child to item chosenIndex of matches
tell application "System Events"
  set r to "?"
  try
    set r to (role of child) as text
  end try
  set nm to "$target_as"
  try
    perform action "AXPress" of child
    return "PRESSED (AXPress) " & r & " | " & nm & " | matches=" & matchCount
  end try
  try
    perform action "AXOpen" of child
    return "PRESSED (AXOpen) " & r & " | " & nm & " | matches=" & matchCount
  end try
  try
    click child
    return "PRESSED (click) " & r & " | " & nm & " | matches=" & matchCount
  end try
end tell
return "FOUND but no supported action: " & r & " | " & nm & " | matches=" & matchCount"####;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_points() {
        for s in ["NaN", "inf", "1e2", "1.", ".1", "+2", "100001", "--1"] {
            assert!(point(s).is_err());
        }
        assert_eq!(point("-0.5").unwrap(), -0.5);
    }
    #[test]
    fn parse_names_without_delimiter_loss() {
        let e = parse_native("w0\tAXButton\tSave | Done\t10\t20\t40\t20\tAXPress\t");
        assert_eq!(e[0].name, "Save | Done");
        assert_eq!((e[0].x, e[0].y), (30, 30));
    }
    #[test]
    fn filters_match_legacy_role_and_text() {
        let e = parse_legacy("AXButton | Save | 10,20\nAXStaticText | Hello | 0,0");
        assert_eq!(filtered(e.clone(), false, "", "").len(), 1);
        assert_eq!(filtered(e, true, "AXStaticText", "hello").len(), 1);
    }
    #[test]
    fn bad_pointer_rejected_before_execution() {
        assert!(
            click(&["nope".into(), "20".into()], "click")
                .unwrap_err()
                .contains("numeric logical points")
        );
        assert!(
            click(
                &[
                    "--app".into(),
                    "A".into(),
                    "--window".into(),
                    "B".into(),
                    "1".into(),
                    "2".into()
                ],
                "click"
            )
            .is_err()
        );
    }
}
