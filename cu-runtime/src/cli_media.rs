//! Bounded Rust orchestration of explicit capture, OCR fallback and scrolling.
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    thread,
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
fn call(program: &Path, args: Vec<String>) -> Result<String> {
    call_with_timeout(program, args, Duration::from_secs(15))
}
fn call_with_timeout(program: &Path, args: Vec<String>, timeout: Duration) -> Result<String> {
    let (code, text) = cu_tools::execute(
        program,
        &cu_tools::Invocation { args, stdin: None },
        timeout,
    )
    .map_err(|e| e.to_string())?;
    if code == 0 {
        Ok(text)
    } else {
        Err(text.trim().to_string())
    }
}
fn native(args: Vec<String>) -> Result<String> {
    call(&program("CU_NATIVE", "cu-native"), args)
}
fn input(args: Vec<String>) -> Result<String> {
    call(&program("CLICLICK", "cu-input"), args)
}
fn base() -> PathBuf {
    std::env::var_os("CU_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cu"))
}
fn shots() -> Result<PathBuf> {
    let root = base();
    let path = root.join("shots");
    for dir in [&root, &path] {
        if fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("CU storage must not be a symlink".into());
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| e.to_string())?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(path)
}
fn log(message: &str) {
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(base().join("log"))
    {
        let _ = writeln!(file, "{message}");
    }
}
fn safe_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
        || name.len() > 200
        || name.chars().any(char::is_control)
    {
        Err("screenshot name must be a simple filename".into())
    } else {
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds {
    pub(crate) x: i64,
    pub(crate) y: i64,
    pub(crate) w: i64,
    pub(crate) h: i64,
}
fn bounds_row(text: &str, offset: usize) -> Result<Bounds> {
    let f: Vec<_> = text.trim_end().split('\t').collect();
    let n = |i: usize| {
        f.get(offset + i)
            .ok_or("invalid native bounds")?
            .parse::<i64>()
            .map_err(|_| "invalid native bounds")
    };
    let b = Bounds {
        x: n(0)?,
        y: n(1)?,
        w: n(2)?,
        h: n(3)?,
    };
    if b.w <= 0
        || b.h <= 0
        || b.w > 100000
        || b.h > 100000
        || b.x.unsigned_abs() > 100000
        || b.y.unsigned_abs() > 100000
    {
        return Err("invalid native bounds".into());
    }
    Ok(b)
}
fn window(app: &str) -> Result<(String, String, Bounds)> {
    let row = native(vec!["bounds".into(), app.into()])?;
    let fields: Vec<_> = row.trim_end().split('\t').collect();
    if fields.len() < 6 {
        return Err("invalid native window".into());
    }
    Ok((fields[0].into(), fields[1].into(), bounds_row(&row, 2)?))
}
pub(crate) fn bounds(app: &str) -> Result<Bounds> {
    window(app).map(|(_, _, bounds)| bounds)
}

fn display(app: &str) -> Result<Bounds> {
    bounds_row(&native(vec!["display".into(), app.into()])?, 1)
}
pub(crate) fn activate(app: &str) -> Result<()> {
    native(vec!["activate".into(), app.into()])
        .map(|_| ())
        .or_else(|_| {
            call(
                &program("CU_OSASCRIPT", "/usr/bin/osascript"),
                vec![
                    "-e".into(),
                    format!("tell application {} to activate", json!(app)),
                ],
            )
            .map(|_| ())
        })
}
fn settle() -> Result<u32> {
    let n = std::env::var("CU_CLICK_SETTLE_MS")
        .unwrap_or("35".into())
        .parse::<u32>()
        .map_err(|_| "invalid click settle delay")?;
    if n > 10000 {
        Err("invalid click settle delay".into())
    } else {
        Ok(n)
    }
}
fn capture(raw: &Path, region: Option<Bounds>) -> Result<()> {
    let mut args = vec!["-x".into(), "-o".into()];
    if let Some(b) = region {
        args.push(format!("-R{},{},{},{}", b.x, b.y, b.w, b.h));
    }
    args.push(raw.to_string_lossy().into_owned());
    call(
        &program("CU_SCREENCAPTURE", "/usr/sbin/screencapture"),
        args,
    )
    .map_err(|_| "screencapture failed".to_string())?;
    if !raw.is_file() {
        return Err("screencapture failed".into());
    }
    fs::set_permissions(raw, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    Ok(())
}
fn scale(raw: &Path, out: &Path, width: i64) -> Result<()> {
    if call(
        &program("CU_SIPS", "/usr/bin/sips"),
        vec![
            "--resampleWidth".into(),
            width.to_string(),
            raw.to_string_lossy().into_owned(),
            "--out".into(),
            out.to_string_lossy().into_owned(),
        ],
    )
    .is_err()
    {
        fs::copy(raw, out).map_err(|e| e.to_string())?;
    }
    fs::set_permissions(out, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    Ok(())
}
fn dimensions(path: &Path) -> Result<(i64, i64)> {
    let out = call(
        &program("CU_SIPS", "/usr/bin/sips"),
        vec![
            "-g".into(),
            "pixelWidth".into(),
            "-g".into(),
            "pixelHeight".into(),
            path.to_string_lossy().into_owned(),
        ],
    )?;
    let mut width = 0;
    let mut height = 0;
    for line in out.lines() {
        if let Some((key, value)) = line.trim().split_once(':') {
            match key {
                "pixelWidth" => width = value.trim().parse().unwrap_or(0),
                "pixelHeight" => height = value.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    if width > 0 && height > 0 {
        Ok((width, height))
    } else {
        Err("could not read screenshot dimensions".into())
    }
}
#[repr(C)]
struct Point {
    x: f64,
    y: f64,
}
#[repr(C)]
struct Size {
    w: f64,
    h: f64,
}
#[repr(C)]
struct Rect {
    p: Point,
    s: Size,
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(id: u32) -> Rect;
}
fn screen_size() -> Result<(i64, i64)> {
    let r = unsafe { CGDisplayBounds(CGMainDisplayID()) };
    let w = r.s.w as i64;
    let h = r.s.h as i64;
    if w > 0 && h > 0 {
        Ok((w, h))
    } else {
        Err("no primary display".into())
    }
}
fn prune(dir: &Path, keep: usize) -> Result<()> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            if e.file_type().ok()?.is_file() && e.path().extension().is_some_and(|s| s == "png") {
                Some((m.modified().ok()?, e.path()))
            } else {
                None
            }
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    for (_, path) in files.into_iter().skip(keep) {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn shot(a: &[String]) -> Result<String> {
    let mut app = None;
    let mut name = "shot";
    let mut i = 0;
    while i < a.len() {
        match a[i].as_str() {
            "-w" | "--window" => {
                i += 1;
                app = Some(a.get(i).ok_or("--window requires an app")?.as_str());
            }
            s if s.starts_with('-') => return Err(format!("unknown shot option '{s}'")),
            s => name = s,
        }
        i += 1;
    }
    safe_name(name)?;
    let keep = std::env::var("CU_KEEP_SHOTS")
        .unwrap_or("20".into())
        .parse::<usize>()
        .map_err(|_| "CU_KEEP_SHOTS must be a non-negative integer")?;
    let dir = shots()?;
    eprintln!(
        "cu: warning: screenshots may contain private data; storing output under {}",
        dir.display()
    );
    let temp = tempfile::Builder::new()
        .prefix("capture-")
        .tempdir_in(&dir)
        .map_err(|e| e.to_string())?;
    let raw = temp.path().join("raw.png");
    let scaled = temp.path().join("scaled.png");
    let out = dir.join(format!("{name}.png"));
    let details;
    if let Some(app) = app {
        let _ = activate(app);
        thread::sleep(Duration::from_millis(200));
        let b = bounds(app).map_err(|_| format!("no front window for '{app}'"))?;
        capture(&raw, Some(b))?;
        scale(&raw, &scaled, b.w)?;
        details = format!(
            "window : {app}  origin={},{}  size={}x{}\nNOTE   : image coords are WINDOW-relative; add origin before cu click",
            b.x, b.y, b.w, b.h
        );
    } else {
        capture(&raw, None)?;
        let (w, h) = screen_size()?;
        let (pw, _) = dimensions(&raw)?;
        scale(&raw, &scaled, w)?;
        details = format!(
            "screen : {w}x{h} pt   captured: {pw}px   scale: {}x",
            pw / w
        );
    }
    let (w, h) = dimensions(&scaled)?;
    fs::rename(&scaled, &out).map_err(|e| e.to_string())?;
    log(&format!("shot {}", out.display()));
    prune(&dir, keep)?;
    Ok(format!("{details}\nimage  : {}  ({w} {h})", out.display()))
}
fn identical(a: &Path, b: &Path) -> Result<bool> {
    let mut a = fs::File::open(a).map_err(|e| e.to_string())?;
    let mut b = fs::File::open(b).map_err(|e| e.to_string())?;
    if a.metadata().map_err(|e| e.to_string())?.len()
        != b.metadata().map_err(|e| e.to_string())?.len()
    {
        return Ok(false);
    }
    let mut aa = [0u8; 65536];
    let mut bb = [0u8; 65536];
    loop {
        let na = a.read(&mut aa).map_err(|e| e.to_string())?;
        let nb = b.read(&mut bb).map_err(|e| e.to_string())?;
        if na != nb || aa[..na] != bb[..nb] {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
    }
}
fn diff(a: &[String]) -> Result<String> {
    if a.len() > 2 {
        return Err("usage: cu diff [before] [after]".into());
    }
    let first = a.first().map(String::as_str).unwrap_or("before");
    let second = a.get(1).map(String::as_str).unwrap_or("after");
    safe_name(first)?;
    safe_name(second)?;
    let dir = shots()?;
    let a = dir.join(format!("{first}.png"));
    let b = dir.join(format!("{second}.png"));
    if !a.is_file() || !b.is_file() {
        return Err(
            "need two shots: cu shot before; <action>; cu shot after; cu diff before after".into(),
        );
    }
    if identical(&a, &b)? {
        Ok("IDENTICAL — screen did not change".into())
    } else {
        Ok(format!(
            "CHANGED — {} vs {} bytes",
            fs::metadata(a).map_err(|e| e.to_string())?.len(),
            fs::metadata(b).map_err(|e| e.to_string())?.len()
        ))
    }
}
const SCROLL_USAGE: &str = "usage: cu scroll [--app \"App\"] [--at X Y] up|down|top|bottom [count]";
fn integer(s: &str) -> Result<i64> {
    let n = s
        .parse::<i64>()
        .map_err(|_| "scroll coordinates must be integers")?;
    if n.unsigned_abs() > 100000 {
        return Err("scroll coordinates must be within logical display limits".into());
    }
    Ok(n)
}
fn scroll(a: &[String], json_mode: bool) -> Result<String> {
    let mut app = "";
    let mut point = None;
    let mut i = 0;
    while i < a.len() {
        match a[i].as_str() {
            "--app" => {
                i += 1;
                app = a.get(i).ok_or("--app requires a name")?;
            }
            "--at" => {
                point = Some((
                    integer(a.get(i + 1).ok_or("--at requires X Y")?)?,
                    integer(a.get(i + 2).ok_or("--at requires X Y")?)?,
                ));
                i += 2;
            }
            _ => break,
        }
        i += 1;
    }
    let rest = &a[i..];
    if rest.len() > 2 {
        return Err(SCROLL_USAGE.into());
    }
    let dir = rest.first().map(String::as_str).unwrap_or("down");
    if !["up", "down", "top", "bottom"].contains(&dir) {
        return Err(SCROLL_USAGE.into());
    }
    let n = rest
        .get(1)
        .map(String::as_str)
        .unwrap_or("1")
        .parse::<usize>()
        .map_err(|_| "scroll count must be a positive integer")?;
    if n == 0 || n > 4096 {
        return Err("scroll count must be a positive integer no greater than 4096".into());
    }
    let wait = settle()?;
    if !app.is_empty() {
        activate(app).map_err(|_| format!("could not activate '{app}'"))?;
        thread::sleep(Duration::from_millis(50));
        if point.is_none() {
            let b = bounds(app).map_err(|_| format!("no visible window matched '{app}'"))?;
            point = Some((b.x + b.w / 2, b.y + b.h / 2));
        }
    }
    let (x, y) = if let Some(p) = point {
        p
    } else {
        let p = input(vec!["p".into()])?;
        let (x, y) = p.trim().split_once(',').ok_or("could not read pointer")?;
        (integer(x)?, integer(y)?)
    };
    if dir == "up" || dir == "down" {
        call_with_timeout(
            &program("CU_NATIVE", "cu-native"),
            vec![
                "scroll".into(),
                x.to_string(),
                y.to_string(),
                if dir == "up" { "1" } else { "-1" }.into(),
                n.to_string(),
            ],
            Duration::from_millis(5000 + n as u64 * 35),
        )
        .map_err(|_| "wheel scrolling failed; grant Accessibility to your terminal")?;
    } else {
        input(vec![format!("m:{x},{y}"), format!("w:{wait}")])?;
        if !app.is_empty() {
            input(vec![
                format!("m:{x},{y}"),
                format!("w:{wait}"),
                format!("c:{x},{y}"),
            ])?;
        }
        let key = if dir == "top" { "home" } else { "end" };
        let events: Vec<String> = (0..n)
            .flat_map(|_| [format!("kp:{key}"), "w:80".into()])
            .collect();
        for chunk in events.chunks(200) {
            input(chunk.to_vec())?;
        }
    }
    log(&format!("scroll {dir} x{n} at={x},{y} app={app}"));
    if json_mode {
        Ok(json!({"ok":true,"direction":dir,"count":n,"point":[x,y],"app":app}).to_string())
    } else {
        Ok(format!(
            "scrolled {dir} x{n} at {x},{y}{}",
            if app.is_empty() {
                String::new()
            } else {
                format!(" in {app}")
            }
        ))
    }
}
#[derive(Clone)]
struct OcrMatch {
    name: String,
    x: i64,
    y: i64,
}
fn ocr_matches(output: &str, target: &str) -> Vec<OcrMatch> {
    let target = target.to_lowercase();
    let mut exact = Vec::new();
    let mut partial = Vec::new();
    for line in output.lines() {
        let f: Vec<_> = line.split('\t').collect();
        if f.len() < 3 {
            continue;
        }
        let (Ok(x), Ok(y)) = (f[1].parse::<i64>(), f[2].parse::<i64>()) else {
            continue;
        };
        let item = OcrMatch {
            name: f[0].into(),
            x,
            y,
        };
        let name = f[0].to_lowercase();
        if name == target {
            exact.push(item.clone());
        }
        if name.contains(&target) {
            partial.push(item);
        }
    }
    if exact.is_empty() { partial } else { exact }
}
pub(crate) fn ocr(b: Bounds) -> Result<String> {
    let dir = shots()?;
    let tmp = tempfile::Builder::new()
        .prefix("ocr-")
        .tempdir_in(dir)
        .map_err(|e| e.to_string())?;
    let raw = tmp.path().join("raw.png");
    let image = tmp.path().join("image.png");
    capture(&raw, Some(b))?;
    scale(&raw, &image, b.w)?;
    native(vec![
        "ocr".into(),
        image.to_string_lossy().into_owned(),
        b.x.to_string(),
        b.y.to_string(),
    ])
}
fn clicktext(a: &[String], json_mode: bool) -> Result<String> {
    if a.len() < 2 {
        return Err(
            "usage: cu clicktext \"App Name\" \"visible text\" [--index N] [--json]".into(),
        );
    }
    let app = &a[0];
    let target = &a[1];
    if target.trim().is_empty() {
        return Err("visible text must not be empty".into());
    }
    let mut index = 0;
    let mut i = 2;
    while i < a.len() {
        if a[i] != "--index" {
            return Err(format!("unknown clicktext option '{}'", a[i]));
        }
        i += 1;
        index = a
            .get(i)
            .ok_or("--index requires a number")?
            .parse::<usize>()
            .map_err(|_| "--index must be a non-negative integer")?;
        i += 1;
    }
    let wait = settle()?;
    activate(app)?;
    thread::sleep(Duration::from_millis(200));
    let (window_id, pid, before) =
        window(app).map_err(|_| format!("ERR: no AX or CoreGraphics window matched '{app}'"))?;
    let source = ocr(before)?;
    let (after_id, after_pid, source_bounds) =
        window(app).map_err(|_| "ERR: target window disappeared after OCR")?;
    if after_id != window_id || after_pid != pid || source_bounds != before {
        return Err("ERR: target window changed during OCR; retry the command".into());
    }
    let mut matches = ocr_matches(&source, target);
    let mut display_scope = false;
    if matches.is_empty() {
        display_scope = true;
        let b = display(app)?;
        matches = ocr_matches(&ocr(b)?, target);
    }
    let count = matches.len();
    if count == 0 {
        return Err(format!("ERR: OCR found no text containing '{target}'"));
    }
    if index == 0 && count > 1 {
        return Err(format!(
            "ERR: OCR text matched {count} elements; add --index"
        ));
    }
    if index == 0 {
        index = 1;
    }
    let mut matched = matches.get(index - 1).cloned().ok_or_else(|| {
        format!("ERR: OCR text matched {count} element(s); index {index} is out of range")
    })?;
    activate(app)?;
    thread::sleep(Duration::from_millis(100));
    let (current_id, current_pid, current_window) = window(app)?;
    if current_id != window_id || current_pid != pid {
        return Err("ERR: target window changed before click; retry the command".into());
    }
    let current = if display_scope {
        display(app)?
    } else {
        current_window
    };
    if !display_scope {
        matched.x += current.x - source_bounds.x;
        matched.y += current.y - source_bounds.y;
    }
    if matched.x < current.x
        || matched.y < current.y
        || matched.x > current.x + current.w
        || matched.y > current.y + current.h
    {
        return Err("ERR: OCR target moved outside the current window; retry the command".into());
    }
    input(vec![
        format!("m:{},{}", matched.x, matched.y),
        format!("w:{wait}"),
        format!("c:{},{}", matched.x, matched.y),
    ])?;
    let result = format!(
        "PRESSED (OCRClick) AXOCRText | {} | matches={count}",
        matched.name
    );
    log(&format!("clicktext {app} index={index} -> OCRClick"));
    if json_mode {
        Ok(json!({"ok":true,"source":"ocr","app":app,"text":target,"result":result}).to_string())
    } else {
        Ok(result)
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
    let result = match cmd {
        "shot" => shot(a),
        "diff" => diff(a),
        "scroll" => scroll(a, json_mode),
        "clicktext" => clicktext(a, json_mode),
        _ => return None,
    };
    Some(result.map(|s| (0, s)))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_escaping_capture_names() {
        for name in ["../secret", "/tmp/image", "a/b", "a\\b", "", "..", "a\nb"] {
            assert!(safe_name(name).is_err());
        }
        assert!(safe_name("my shot").is_ok());
    }
    #[test]
    fn byte_diff_and_prune() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.png");
        let b = dir.path().join("b.png");
        fs::write(&a, b"same").unwrap();
        fs::write(&b, b"same").unwrap();
        assert!(identical(&a, &b).unwrap());
        fs::write(&b, b"diff").unwrap();
        assert!(!identical(&a, &b).unwrap());
        let other = dir.path().join("keep.txt");
        fs::write(&other, b"keep").unwrap();
        prune(dir.path(), 1).unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        assert!(other.exists());
    }
    #[test]
    fn exact_ocr_before_partial() {
        let m = ocr_matches("Save as\t1\t2\nSave\t3\t4\n", "save");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].x, 3);
        assert_eq!(ocr_matches("Save\t3\t4\n", "missing").len(), 0);
    }
    #[test]
    fn validates_scroll_before_side_effects() {
        assert!(
            scroll(&["down".into(), "1".into(), "extra".into()], true)
                .unwrap_err()
                .contains("usage: cu scroll")
        );
        assert!(scroll(&["--at".into(), "NaN".into(), "0".into()], true).is_err());
    }
}
