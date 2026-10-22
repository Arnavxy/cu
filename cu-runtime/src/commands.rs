//! Rust snapshot, action, wait and batch orchestration. Native-only routes never
//! capture pixels; compatible OCR and visual verification remain available.
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
type Reply = Result<(i32, String), String>;
fn helper() -> PathBuf {
    std::env::var_os("CU_NATIVE")
        .map(PathBuf::from)
        .unwrap_or_else(|| cu_tools::sibling("cu-native"))
}
fn native(args: &[String]) -> Reply {
    cu_tools::execute(
        &helper(),
        &cu_tools::Invocation {
            args: args.to_vec(),
            stdin: None,
        },
        Duration::from_secs(5),
    )
    .map_err(|e| e.to_string())
}
fn native_ok(args: &[String]) -> Result<String, String> {
    let (code, out) = native(args)?;
    if code == 0 { Ok(out) } else { Err(out) }
}
fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}
fn fail(code: &str, message: impl AsRef<str>) -> Reply {
    Ok((
        1,
        json!({"ok":false,"error":{"code":code,"message":message.as_ref()}}).to_string(),
    ))
}
fn success(value: Value) -> Reply {
    Ok((0, value.to_string()))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn number(value: &str) -> Result<i64, String> {
    value
        .parse()
        .map_err(|_| "Invalid native numeric field".into())
}
fn state_dir() -> Result<PathBuf, String> {
    let dir = std::env::var_os("CU_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cu"));
    fs::create_dir_all(dir.join("snapshots")).map_err(|e| e.to_string())?;
    for path in [&dir, &dir.join("snapshots")] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}
fn record(message: &str) {
    if let Ok(dir) = state_dir()
        && let Ok(mut file) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(dir.join("log"))
    {
        let _ = writeln!(file, "{}  {}", now(), message);
    }
}
fn prune_snapshots(dir: &std::path::Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(id) = path
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_suffix(".tsv"))
        else {
            continue;
        };
        if valid_snapshot(id)
            && entry.file_type().is_ok_and(|t| t.is_file())
            && entry.metadata().and_then(|m| m.modified()).is_ok_and(|t| {
                t.elapsed()
                    .is_ok_and(|age| age > Duration::from_secs(86400))
            })
        {
            let _ = fs::remove_file(path);
        }
    }
}
fn valid_snapshot(id: &str) -> bool {
    let p: Vec<_> = id.split('_').collect();
    p.len() == 3
        && p[0] == "s"
        && p[1..]
            .iter()
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}
fn valid_element(id: &str) -> bool {
    id.strip_prefix("e_")
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}
#[derive(Clone, Debug)]
struct Element {
    source: String,
    confidence: f64,
    id: String,
    path: String,
    role: String,
    name: String,
    bounds: [i64; 4],
    actions: String,
    value: String,
    enabled: Option<bool>,
}
impl Element {
    fn parse(line: &str) -> Result<Self, String> {
        let f: Vec<_> = line.split('\t').collect();
        if f.len() < 8 {
            return Err("Invalid native element record".into());
        }
        Ok(Self {
            source: "native_ax".into(),
            confidence: 1.0,
            id: String::new(),
            path: f[0].into(),
            role: f[1].into(),
            name: f[2].into(),
            bounds: [number(f[3])?, number(f[4])?, number(f[5])?, number(f[6])?],
            actions: f[7].into(),
            value: f.get(8).unwrap_or(&"").to_string(),
            enabled: match f.get(9).copied().unwrap_or("") {
                "true" => Some(true),
                "false" => Some(false),
                "" => None,
                _ => return Err("Invalid native enabled state".into()),
            },
        })
    }
    fn json(&self) -> Value {
        let mut value = json!({"id":self.id,"source":self.source,"role":self.role,"name":self.name,"value":self.value,"bounds":self.bounds,"confidence":self.confidence,"enabled":self.enabled,"actions":self.actions.split(',').filter(|s|!s.is_empty()).collect::<Vec<_>>()});
        // Only actual native paths establish AX parent/child relationships.
        // OCR labels and coordinate targets never gain an invented AX path.
        if self.source == "native_ax" && native_path(&self.path) {
            value["path"] = json!(self.path);
        }
        value
    }
    fn signature(&self) -> Value {
        json!([
            self.role,
            self.name,
            self.bounds,
            self.actions,
            self.value,
            self.enabled
        ])
    }
    fn identity(&self) -> String {
        if self.path == "-" {
            format!(
                "{}:{}",
                self.source,
                json!([self.role, self.name, self.bounds])
            )
        } else {
            format!("{}:{}", self.source, self.path)
        }
    }
    fn tsv(&self) -> String {
        format!(
            "EL\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{}\t{}\n",
            self.id,
            self.source,
            self.path,
            self.role,
            self.name,
            self.bounds[0],
            self.bounds[1],
            self.bounds[2],
            self.bounds[3],
            self.confidence,
            self.actions,
            match self.enabled {
                Some(enabled) => format!("{}\t{}", self.value, enabled),
                None => self.value.clone(),
            }
        )
    }
}
#[derive(Clone, Debug)]
struct Window {
    id: i64,
    pid: i64,
    bounds: [i64; 4],
    app: String,
    title: String,
    bundle: String,
}
impl Window {
    fn parse(line: &str) -> Result<Self, String> {
        let f: Vec<_> = line.split('\t').collect();
        if f.len() < 7 {
            return Err("Invalid native window record".into());
        }
        Ok(Self {
            id: number(f[0])?,
            pid: number(f[1])?,
            bounds: [number(f[2])?, number(f[3])?, number(f[4])?, number(f[5])?],
            app: f[6].into(),
            title: f.get(7).copied().unwrap_or("").into(),
            bundle: f.get(8).unwrap_or(&"").to_string(),
        })
    }
}
fn snapshot_window(rows: &str, expected: &Window) -> Option<Window> {
    let mut matches = rows
        .lines()
        .filter_map(|line| Window::parse(line).ok())
        .filter(|window| window.id == expected.id && window.pid == expected.pid);
    let window = matches.next()?;
    matches.next().is_none().then_some(window)
}

struct Snapshot {
    id: String,
    created: u64,
    window: Window,
    elements: Vec<Element>,
}
// Keep snapshot TSV interoperable with the existing CLI; parse explicitly so
// empty value/action fields do not collapse as they can in shell `read`.
fn load_snapshot(id: &str) -> Result<Snapshot, String> {
    if !valid_snapshot(id) {
        return Err("invalid snapshot ID".into());
    }
    let file = state_dir()?.join("snapshots").join(format!("{id}.tsv"));
    if fs::metadata(&file)
        .map_err(|_| "snapshot does not exist or expired")?
        .len()
        > cu_tools::MAX_OUTPUT as u64
    {
        return Err("snapshot exceeds size limit".into());
    }
    let body = fs::read_to_string(file).map_err(|_| "snapshot does not exist or expired")?;
    parse_snapshot(id, &body)
}
fn parse_snapshot(id: &str, body: &str) -> Result<Snapshot, String> {
    let mut lines = body.lines();
    let f: Vec<_> = lines.next().unwrap_or("").split('\t').collect();
    if f.len() < 12 || f[0] != "META" || f[1] != id {
        return Err("invalid snapshot metadata".into());
    }
    let window = Window {
        id: number(f[5])?,
        pid: number(f[4])?,
        bounds: [number(f[6])?, number(f[7])?, number(f[8])?, number(f[9])?],
        app: f[3].into(),
        bundle: f[10].into(),
        title: f[11].into(),
    };
    let mut elements = Vec::new();
    for line in lines {
        let p: Vec<_> = line.split('\t').collect();
        if p.len() < 12 || p[0] != "EL" {
            continue;
        }
        if !matches!(p[2], "native_ax" | "vision_ocr") {
            return Err("unsupported snapshot source".into());
        }
        let row = [
            p[3],
            p[4],
            p[5],
            p[6],
            p[7],
            p[8],
            p[9],
            p[11],
            p.get(12).copied().unwrap_or(""),
            p.get(13).copied().unwrap_or(""),
        ]
        .join("\t");
        let mut e = Element::parse(&row)?;
        e.id = p[1].into();
        e.source = p[2].into();
        e.confidence = p[10]
            .parse::<f64>()
            .map_err(|_| "invalid snapshot confidence")?;
        if !e.confidence.is_finite() {
            return Err("invalid snapshot confidence".into());
        }
        elements.push(e);
    }
    Ok(Snapshot {
        id: id.into(),
        created: f[2].parse().map_err(|_| "invalid snapshot time")?,
        window,
        elements,
    })
}
fn option_value(args: &[String], i: &mut usize) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("{} requires a value", args[*i - 1]))
}
fn interactive(role: &str) -> bool {
    matches!(
        role,
        "AXButton"
            | "AXCheckBox"
            | "AXRadioButton"
            | "AXTextField"
            | "AXTextArea"
            | "AXComboBox"
            | "AXPopUpButton"
            | "AXMenuButton"
            | "AXMenuItem"
            | "AXLink"
            | "AXSlider"
            | "AXTabGroup"
            | "AXCell"
            | "AXRow"
            | "AXOCRText"
            | "AXDisclosureTriangle"
    )
}
fn external(program: &str, args: &[String]) -> Result<String, String> {
    let (code, text) = cu_tools::execute(
        std::path::Path::new(program),
        &cu_tools::Invocation {
            args: args.to_vec(),
            stdin: None,
        },
        Duration::from_secs(5),
    )
    .map_err(|e| e.to_string())?;
    if code == 0 { Ok(text) } else { Err(text) }
}
fn capture(window: &Window, background: bool, path: &std::path::Path) -> Result<(), String> {
    let program =
        std::env::var("CU_SCREENCAPTURE").unwrap_or_else(|_| "/usr/sbin/screencapture".into());
    let region = if background {
        format!("-l{}", window.id)
    } else {
        format!(
            "-R{},{},{},{}",
            window.bounds[0], window.bounds[1], window.bounds[2], window.bounds[3]
        )
    };
    external(
        &program,
        &[
            "-x".into(),
            "-o".into(),
            region,
            path.to_string_lossy().into_owned(),
        ],
    )?;
    if !fs::metadata(path).is_ok_and(|m| m.len() > 0) {
        return Err("screencapture produced no image".into());
    }
    Ok(())
}
fn ocr_elements(app: &str, background: bool) -> Result<Vec<Element>, String> {
    if !background {
        let _ = native(&strings(&["activate", app]));
        std::thread::sleep(Duration::from_millis(150));
    }
    let window = Window::parse(&native_ok(&strings(&["bounds", app]))?)?;
    let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
    let raw = directory.path().join("raw.png");
    let image = directory.path().join("image.png");
    capture(&window, background, &raw)?;
    let sips = std::env::var("CU_SIPS").unwrap_or_else(|_| "/usr/bin/sips".into());
    if external(
        &sips,
        &[
            "--resampleWidth".into(),
            window.bounds[2].to_string(),
            raw.to_string_lossy().into_owned(),
            "--out".into(),
            image.to_string_lossy().into_owned(),
        ],
    )
    .is_err()
    {
        fs::copy(&raw, &image).map_err(|e| e.to_string())?;
    }
    let output = native_ok(&[
        "ocr".into(),
        image.to_string_lossy().into_owned(),
        window.bounds[0].to_string(),
        window.bounds[1].to_string(),
    ])?;
    let mut elements = Vec::new();
    for line in output.lines() {
        let f: Vec<_> = line.split('\t').collect();
        if f.len() < 6 || f[0].is_empty() {
            continue;
        }
        let w = number(f[3])?;
        let h = number(f[4])?;
        let confidence = f[5].parse::<f64>().map_err(|_| "invalid OCR confidence")?;
        if !confidence.is_finite() {
            return Err("invalid OCR confidence".into());
        }
        elements.push(Element {
            source: "vision_ocr".into(),
            confidence,
            id: String::new(),
            path: "-".into(),
            role: "AXOCRText".into(),
            name: f[0].into(),
            bounds: [number(f[1])? - w / 2, number(f[2])? - h / 2, w, h],
            actions: "OCRClick".into(),
            enabled: None,
            value: String::new(),
        });
    }
    Ok(elements)
}
fn fingerprint(app: &str) -> Result<String, String> {
    let window = Window::parse(&native_ok(&strings(&["bounds", app]))?)?;
    let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
    let image = directory.path().join("verify.png");
    capture(&window, true, &image)?;
    native_ok(&["fingerprint".into(), image.to_string_lossy().into_owned()])
}
fn window_ready(app: &str) -> Result<Window, String> {
    let delay = std::env::var("CU_WINDOW_READY_DELAY")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n >= 0.0)
        .unwrap_or(0.15)
        .min(5.0);
    let poll = std::env::var("CU_WINDOW_READY_POLL")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n > 0.0)
        .unwrap_or(0.03)
        .min(1.0);
    let start = Instant::now();
    loop {
        match native_ok(&strings(&["bounds", app])).and_then(|s| Window::parse(&s)) {
            Ok(window) => return Ok(window),
            Err(error) if start.elapsed() >= Duration::from_secs_f64(delay) => return Err(error),
            Err(_) => std::thread::sleep(
                Duration::from_secs_f64(poll)
                    .min(Duration::from_secs_f64(delay).saturating_sub(start.elapsed())),
            ),
        }
    }
}
fn native_path(path: &str) -> bool {
    path.strip_prefix('w').is_some_and(|tail| {
        tail.split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    })
}
fn select_elements(
    raw: Vec<Element>,
    all: bool,
    role: &str,
    name: &str,
    max: usize,
) -> (Vec<Element>, bool) {
    let mut elements = Vec::new();
    let mut seen = HashSet::new();
    for mut element in raw {
        if element.name.is_empty()
            || (element.source == "native_ax" && !all && !interactive(&element.role))
            || (element.source == "native_ax" && !role.is_empty() && element.role != role)
            || (!name.is_empty() && !element.name.contains(name))
        {
            continue;
        }
        let identity = if element.source == "native_ax" && native_path(&element.path) {
            element.identity()
        } else {
            json!([element.role, element.name, element.bounds]).to_string()
        };
        if !seen.insert(identity) {
            continue;
        }
        // Reaching the limit alone is not truncation: there must be another
        // matching unique element that would otherwise have been returned.
        if elements.len() == max {
            return (elements, true);
        }
        element.id = format!("e_{}", elements.len() + 1);
        elements.push(element);
    }
    (elements, false)
}
fn observation_scope(
    context: &str,
    source: &str,
    all: bool,
    role: &str,
    name: &str,
    max: usize,
    capped: bool,
) -> Value {
    let traversal = if source == "native_ax" {
        context
            .lines()
            .filter_map(|line| line.strip_prefix("QUALITY\t"))
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find_map(|quality| quality["traversal_truncated"].as_bool())
    } else {
        None
    };
    // Missing legacy helper metadata means unknown, never an assertion that
    // traversal completed. Review callers must require truncated == false.
    let truncated = if capped { Some(true) } else { traversal };
    json!({"truncated":truncated,"traversal_truncated":traversal,"output_capped":capped,
        "selection":{"all":all,"filtered":!all || !role.is_empty() || !name.is_empty(),
            "max_elements":if max == usize::MAX { None } else { Some(max) }}})
}
fn draft_capture(context: &str, requested: bool, source: &str) -> Value {
    if !requested || source != "native_ax" {
        return json!({"requested":requested,"complete":false,"fields":[]});
    }
    context
        .lines()
        .filter_map(|line| line.strip_prefix("QUALITY\t"))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|quality| quality.get("draft_values").cloned())
        .unwrap_or_else(|| json!({"requested":true,"complete":false,"fields":[]}))
}
fn apply_draft_values(result: &mut Value, capture: &Value) {
    let mut complete = capture["requested"] == true && capture["complete"] == true;
    let mut used = HashSet::new();
    let mut total = 0usize;
    let fields = capture["fields"].as_array();
    if fields.is_none_or(|fields| fields.len() > 12) {
        complete = false;
    }
    if let Some(fields) = fields.filter(|fields| fields.len() <= 12) {
        for field in fields {
            let path = field["path"].as_str().unwrap_or("");
            let Some(value) = field["value"].as_str() else {
                complete = false;
                continue;
            };
            let count = value.chars().count();
            total = total.saturating_add(count);
            if !native_path(path)
                || !used.insert(path)
                || count > 8000
                || total > 24000
                || value
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            {
                complete = false;
                continue;
            }
            let target = result["elements"].as_array_mut().and_then(|elements| {
                elements.iter_mut().find(|element| {
                    element["source"] == "native_ax"
                        && element["path"] == path
                        && element["role"] == field["role"]
                        && (element["name"] == field["name"]
                            || field["name"] == "" && element["name"] == "Draft field")
                })
            });
            if let Some(target) = target {
                target["value"] = json!(value);
            } else {
                complete = false;
            }
        }
    }
    result["draft_values"] = json!({"requested":true,"complete":complete});
    if !complete {
        result["truncated"] = json!(true);
    }
}
fn observe(args: &[String]) -> Reply {
    let app = args
        .first()
        .filter(|a| !a.is_empty() && !a.starts_with('-'))
        .ok_or("usage: cu observe APP --native-only")?;
    let (mut all, mut background, mut draft_values) = (false, false, false);
    let (mut since, mut role, mut name) = (String::new(), String::new(), String::new());
    let mut max = usize::MAX;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--all" => all = true,
            "--draft-values" => draft_values = true,
            "--background" => background = true,
            "--native-only" => {}
            "--since" => since = option_value(args, &mut i)?,
            "--role" => role = option_value(args, &mut i)?,
            "--name" | "--filter" => name = option_value(args, &mut i)?,
            "--max-elements" => {
                max = option_value(args, &mut i)?
                    .parse()
                    .map_err(|_| "--max-elements must be a positive integer")?;
                if max == 0 {
                    return Err("--max-elements must be a positive integer".into());
                }
            }
            _ => return Err(format!("unknown observe option '{}'", args[i])),
        }
        i += 1;
    }
    let prior = if since.is_empty() {
        None
    } else {
        Some(load_snapshot(&since)?)
    };
    if !background {
        let _ = native(&strings(&["activate", app]));
    }
    let mut call = strings(&["context", app]);
    if !all {
        call.push("--interactive".into())
    }
    if draft_values {
        call.push("--draft-values".into());
    }
    let context = native_ok(&call).unwrap_or_default();
    let mut lines = context.lines();
    let first = lines.next().unwrap_or("");
    let (window, tree) = if let Some(meta) = first.strip_prefix("META\t") {
        let window = Window::parse(meta)?;
        let tree = lines
            .filter_map(|l| l.strip_prefix("AX\t"))
            .map(str::to_string)
            .collect::<Vec<_>>();
        (window, tree)
    } else {
        let mut window = window_ready(app)?;
        if let Ok(info) = native_ok(&strings(&["app-info", app])) {
            window.bundle = info.split('\t').nth(2).unwrap_or("").into();
        }
        let mut a = strings(&["ax-tree", app]);
        if !all {
            a.push("--interactive".into())
        }
        let tree = native_ok(&a)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect();
        (window, tree)
    };
    if prior
        .as_ref()
        .is_some_and(|p| p.window.app != window.app || p.window.pid != window.pid)
    {
        return Err(format!(
            "snapshot '{since}' belongs to a different application instance"
        ));
    }
    let quality_missing = context
        .lines()
        .filter_map(|l| l.strip_prefix("QUALITY\t"))
        .filter_map(|s| serde_json::from_str::<Value>(s).ok())
        .any(|v| v["browser"] == true && v["web_content_unavailable"] == true);
    let mut raw = Vec::new();
    for line in &tree {
        raw.push(Element::parse(line)?)
    }
    let teams_empty = window.bundle == "com.microsoft.teams2"
        && !raw.iter().any(|e| {
            !matches!(e.role.as_str(), "AXWindow" | "AXGroup")
                && !e.name.is_empty()
                && !(e.role == "AXButton"
                    && [
                        "zoom the window",
                        "close",
                        "minimize",
                        "minimise",
                        "full screen",
                    ]
                    .iter()
                    .any(|s| e.name.to_lowercase().contains(s)))
        });
    let source = if raw.is_empty() || quality_missing || teams_empty {
        if draft_values || args.iter().any(|s| s == "--native-only") {
            let on_screen = context
                .lines()
                .filter_map(|line| line.strip_prefix("QUALITY\t"))
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .find_map(|quality| quality["on_screen"].as_bool());
            if on_screen == Some(false) {
                return Ok((1,json!({"ok":false,"error":{"code":"native_window_inactive",
                    "message":"The app window exists off screen but exposes no readable native tree. Background reading cannot recover it; an authorized foreground task may reopen the app and read again. No screenshot was taken."},
                    "visibility":{"on_screen":false}}).to_string()));
            }
            return fail(
                "native_ax_unavailable",
                "Native Accessibility has no readable content. No screenshot was taken. Try --all or a native document API; omit --native-only to explicitly allow local OCR fallback.",
            );
        }
        raw = match ocr_elements(app, background) {
            Ok(elements) => elements,
            Err(e) => {
                return fail(
                    if background {
                        "background_ocr_unavailable"
                    } else {
                        "ocr_unavailable"
                    },
                    e,
                );
            }
        };
        "vision_ocr"
    } else {
        "native_ax"
    };
    let capture = draft_capture(&context, draft_values, source);
    if draft_values && let Some(fields) = capture["fields"].as_array() {
        for element in &mut raw {
            if element.source == "native_ax"
                && element.name.is_empty()
                && element.role == "AXTextArea"
                && fields.iter().any(|field| {
                    field["path"] == element.path
                        && field["role"] == element.role
                        && field["name"] == ""
                })
            {
                element.name = "Draft field".into();
            }
        }
    }
    let (elements, output_capped) = select_elements(raw, all, &role, &name, max);
    if elements.is_empty() {
        return Err("observation produced no named elements".into());
    }
    let created = now();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let id = format!("s_{created}_{nonce}");
    let dir = state_dir()?.join("snapshots");
    let mut temp = tempfile::NamedTempFile::new_in(&dir).map_err(|e| e.to_string())?;
    writeln!(
        temp,
        "META\t{id}\t{created}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        window.app,
        window.pid,
        window.id,
        window.bounds[0],
        window.bounds[1],
        window.bounds[2],
        window.bounds[3],
        window.bundle,
        window.title
    )
    .map_err(|e| e.to_string())?;
    for e in &elements {
        temp.write_all(e.tsv().as_bytes())
            .map_err(|e| e.to_string())?
    }
    temp.persist_noclobber(dir.join(format!("{id}.tsv")))
        .map_err(|e| e.to_string())?;
    let mut result = json!({"ok":true,"snapshot":id,"created_at":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64(),"source":source,"background":background,"visibility":{"scope":if source=="native_ax"{"current_accessibility_view"}else{"captured_window"},"may_contain_scrolled_out":true,"complete":false},"app":{"name":window.app,"pid":window.pid,"bundle_id":window.bundle},"window":{"id":window.id,"title":window.title,"bounds":window.bounds},"elements":[]});
    result["focus_within_window"] = if source == "native_ax" {
        context
            .lines()
            .find_map(|line| line.strip_prefix("QUALITY\t"))
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|quality| quality["focus_within_window"].as_bool())
            .map(Value::Bool)
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let scope = observation_scope(&context, source, all, &role, &name, max, output_capped);
    result
        .as_object_mut()
        .unwrap()
        .extend(scope.as_object().unwrap().clone());
    if let Some(on_screen) = context
        .lines()
        .filter_map(|line| line.strip_prefix("QUALITY\t"))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|quality| quality["on_screen"].as_bool())
    {
        result["visibility"]["on_screen"] = json!(on_screen);
    }
    if let Some(document) = context
        .lines()
        .filter_map(|line| line.strip_prefix("QUALITY\t"))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|quality| quality.get("document").cloned())
        .find(|document| {
            document["source"] == "window_document"
                && document["read_only"] == true
                && document["content_read"] == false
                && document["pid"] == window.pid
                && document["window_id"] == window.id
        })
    {
        result["document"] = document;
    }
    let mut output = Vec::new();
    let (mut added, mut changed, mut unchanged) = (0, 0, 0);
    if let Some(prior) = prior {
        let old: BTreeMap<_, _> = prior.elements.iter().map(|e| (e.identity(), e)).collect();
        let current: HashSet<_> = elements.iter().map(Element::identity).collect();
        for e in &elements {
            let change = match old.get(&e.identity()) {
                None => {
                    added += 1;
                    "added"
                }
                Some(p) if p.signature() == e.signature() => {
                    unchanged += 1;
                    continue;
                }
                _ => {
                    changed += 1;
                    "changed"
                }
            };
            let mut v = e.json();
            v["change"] = json!(change);
            output.push(v)
        }
        let removed: Vec<_> = prior
            .elements
            .iter()
            .filter(|e| !current.contains(&e.identity()))
            .map(|e| json!({"role":e.role,"name":e.name,"bounds":e.bounds}))
            .collect();
        result["delta"] = json!({"from":since,"added":added,"changed":changed,"removed":removed.len(),"unchanged":unchanged});
        result["removed"] = json!(removed);
    } else {
        output = elements.iter().map(Element::json).collect()
    }
    result["elements"] = json!(output);
    // Exact multiline/tabbed values travel only in this opt-in JSON result.
    // Keep editable contents out of the legacy TSV action-snapshot format.
    if draft_values {
        apply_draft_values(&mut result, &capture);
    }

    record(&format!(
        "observe {app} snapshot={id} source={source} elements={}",
        elements.len()
    ));
    prune_snapshots(&dir);
    success(result)
}
fn click_helper() -> PathBuf {
    std::env::var_os("CLICLICK")
        .map(PathBuf::from)
        .unwrap_or_else(|| cu_tools::sibling("cu-input"))
}
fn settled_click(x: i64, y: i64) -> Result<(), String> {
    let delay = std::env::var("CU_CLICK_SETTLE_MS")
        .unwrap_or_else(|_| "35".into())
        .parse::<u64>()
        .map_err(|_| "CU_CLICK_SETTLE_MS must be a non-negative integer")?;
    let (code, text) = cu_tools::execute(
        &click_helper(),
        &cu_tools::Invocation {
            args: vec![
                format!("m:{x},{y}"),
                format!("w:{delay}"),
                format!("c:{x},{y}"),
            ],
            stdin: None,
        },
        Duration::from_secs(5),
    )
    .map_err(|e| e.to_string())?;
    if code == 0 { Ok(()) } else { Err(text) }
}
fn semantic_element(app: &str, path: &str) -> Result<Option<Element>, String> {
    let tree = native_ok(&strings(&["ax-tree", app, "--all"]))?;
    tree.lines()
        .find(|line| line.split('\t').next() == Some(path))
        .map(Element::parse)
        .transpose()
}
fn semantic_effect(app: &str, e: &Element) -> &'static str {
    match semantic_element(app, &e.path) {
        Ok(Some(current)) if current.signature() == e.signature() => "none",
        Ok(_) => "changed",
        Err(_) => "unknown",
    }
}
fn verify_pause() {
    let seconds = std::env::var("CU_VERIFY_DELAY")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(0.2)
        .min(5.0);
    std::thread::sleep(Duration::from_secs_f64(seconds));
}
fn act(args: &[String], json_mode: bool) -> Reply {
    let id = args
        .first()
        .ok_or("usage: cu act ELEMENT --snapshot SNAPSHOT")?;
    let (mut snapshot, mut background, mut verify) = (String::new(), false, true);
    let mut native_only = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--snapshot" => snapshot = option_value(args, &mut i)?,
            "--background" => background = true,
            "--no-verify" => verify = false,
            "--native-only" => native_only = true,
            _ => return Err(format!("unknown act option '{}'", args[i])),
        }
        i += 1;
    }
    if !valid_snapshot(&snapshot) {
        return fail("invalid_snapshot", "invalid or missing snapshot ID");
    }
    if !valid_element(id) {
        return fail("invalid_element", "invalid element ID");
    }
    let s = match load_snapshot(&snapshot) {
        Ok(s) => s,
        Err(e) => return fail("snapshot_not_found", e),
    };
    let ttl = std::env::var("CU_SNAPSHOT_TTL")
        .unwrap_or_else(|_| "120".into())
        .parse::<u64>()
        .map_err(|_| "CU_SNAPSHOT_TTL must be a non-negative integer")?;
    if now().saturating_sub(s.created) > ttl {
        return fail(
            "stale_snapshot",
            format!("snapshot is older than {ttl}s; observe again"),
        );
    }
    let Some(e) = s.elements.iter().find(|e| e.id == *id) else {
        return fail(
            "element_not_found",
            format!("element '{id}' is not in snapshot '{snapshot}'"),
        );
    };
    if e.enabled == Some(false) {
        return fail(
            "control_disabled",
            "This control is disabled; no action was taken.",
        );
    }
    if native_only && e.source != "native_ax" {
        return fail(
            "native_action_unavailable",
            "This snapshot contains an OCR target; observe a native element before requesting --native-only.",
        );
    }
    if background && e.source != "native_ax" {
        return fail(
            "background_action_unavailable",
            "OCR targets require foreground pointer input. Observe natively or omit --background explicitly.",
        );
    }
    let mut current =
        match native_ok(&strings(&["bounds", &s.window.app])).and_then(|r| Window::parse(&r)) {
            Ok(w) => w,
            Err(_) => return fail("stale_snapshot", "application window disappeared"),
        };
    if current.id != s.window.id || current.pid != s.window.pid {
        // A modal sheet can become the front CG window while its controls stay
        // AX descendants of the observed parent. Background semantic actions
        // are bound to that exact surviving parent, not an arbitrary front
        // window. Foreground/pointer actions retain their stricter focus gate.
        let parent = if background && e.source == "native_ax" {
            native_ok(&strings(&["windows", &s.window.app]))
                .ok()
                .and_then(|rows| snapshot_window(&rows, &s.window))
        } else {
            None
        };
        let Some(parent) = parent else {
            return fail(
                "stale_snapshot",
                "the observed window is unavailable; observe again",
            );
        };
        current = parent;
    }
    if background && !native_only {
        verify = false
    } else if !background {
        if native_ok(&strings(&["activate", &s.window.app])).is_err() {
            return fail(
                "activation_failed",
                format!("could not activate '{}'", s.window.app),
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let expected = [
        e.bounds[0] + current.bounds[0] - s.window.bounds[0],
        e.bounds[1] + current.bounds[1] - s.window.bounds[1],
    ];
    let (mut visual, mut semantic, mut fallback) = ("unknown", "unknown", "none");
    let mut before = None;
    if verify && !native_only {
        before = fingerprint(&s.window.app).ok();
        std::thread::sleep(Duration::from_millis(50));
        let baseline = fingerprint(&s.window.app).ok();
        if let (Some(a), Some(b)) = (&before, &baseline) {
            visual = if a == b { "stable" } else { "animated" }
        }
    }
    let mut action = if e.source == "native_ax" {
        let call = vec![
            "ax-perform".into(),
            s.window.pid.to_string(),
            e.path.clone(),
            e.role.clone(),
            e.name.clone(),
            expected[0].to_string(),
            expected[1].to_string(),
            e.bounds[2].to_string(),
            e.bounds[3].to_string(),
        ];
        match native_ok(&call) {
            Ok(a) => a,
            Err(error) if error.contains("control_disabled:") => {
                return fail("control_disabled", error);
            }
            Err(error) => return fail("stale_element", error),
        }
    } else {
        let fresh = match ocr_elements(&s.window.app, false) {
            Ok(v) => v,
            Err(_) => {
                return fail(
                    "verification_failed",
                    "could not refresh OCR before clicking",
                );
            }
        };
        let target = [expected[0] + e.bounds[2] / 2, expected[1] + e.bounds[3] / 2];
        let chosen = fresh
            .iter()
            .filter(|el| el.name == e.name)
            .min_by_key(|el| {
                let dx = i128::from(el.bounds[0] + el.bounds[2] / 2 - target[0]);
                let dy = i128::from(el.bounds[1] + el.bounds[3] / 2 - target[1]);
                dx * dx + dy * dy
            });
        let Some(chosen) = chosen else {
            return fail(
                "stale_element",
                format!("OCR text '{}' is no longer visible", e.name),
            );
        };
        let point = [
            chosen.bounds[0] + chosen.bounds[2] / 2,
            chosen.bounds[1] + chosen.bounds[3] / 2,
        ];
        let tolerance = e.bounds[2].max(e.bounds[3]).max(30);
        if (point[0] - target[0]).abs() > tolerance || (point[1] - target[1]).abs() > tolerance {
            return fail(
                "stale_element",
                "OCR target moved unexpectedly; observe again",
            );
        }
        settled_click(point[0], point[1])?;
        "OCRClick".into()
    };
    let (mut window_state, mut changed, mut available) = ("same", None, false);
    if verify {
        if !native_only {
            verify_pause();
        }
        if let Ok(after_window) =
            native_ok(&strings(&["bounds", &s.window.app])).and_then(|out| Window::parse(&out))
        {
            if after_window.id != s.window.id {
                window_state = "changed";
                changed = Some(true);
                available = true;
            } else if native_only {
                semantic = semantic_effect(&s.window.app, e);
                available = semantic != "unknown";
                changed = match semantic {
                    "changed" => Some(true),
                    "none" => Some(false),
                    _ => None,
                };
            } else if visual == "animated" {
                available = true;
            } else if let (Some(a), Ok(b)) = (&before, fingerprint(&s.window.app)) {
                available = true;
                changed = Some(a != &b);
            }
            if e.source == "native_ax" && !native_only {
                semantic = semantic_effect(&s.window.app, e);
                if semantic == "changed" {
                    changed = Some(true)
                }
            }
            if !native_only
                && changed != Some(true)
                && semantic != "changed"
                && e.source == "native_ax"
                && action.starts_with("AXPress")
                && std::env::var("CU_AX_COORDINATE_FALLBACK").unwrap_or_else(|_| "1".into()) == "1"
                && click_helper().is_file()
            {
                let center = semantic_element(&s.window.app, &e.path)
                    .ok()
                    .flatten()
                    .map(|el| {
                        [
                            el.bounds[0] + el.bounds[2] / 2,
                            el.bounds[1] + el.bounds[3] / 2,
                        ]
                    })
                    .unwrap_or([expected[0] + e.bounds[2] / 2, expected[1] + e.bounds[3] / 2]);
                settled_click(center[0], center[1])?;
                fallback = "used";
                action = "AXPress+CoordinateClick".into();
                verify_pause();
                if let (Some(a), Ok(b)) = (&before, fingerprint(&s.window.app)) {
                    changed = if visual == "animated" {
                        None
                    } else {
                        Some(a != &b)
                    };
                }
                semantic = semantic_effect(&s.window.app, e);
                if semantic == "changed" {
                    changed = Some(true)
                }
            }
        } else {
            window_state = "closed";
            changed = Some(true);
            available = true;
        }
    }
    record(&format!(
        "act snapshot={} element={id} action={action} changed={changed:?} window={window_state}",
        s.id
    ));
    if json_mode {
        success(
            json!({"ok":true,"snapshot":s.id,"element":id,"action":action,"source":e.source,"background":background,"verification":{"available":available,"changed":changed,"window_state":window_state,"visual_activity":visual,"semantic_effect":semantic,"coordinate_fallback":fallback}}),
        )
    } else {
        Ok((
            0,
            format!(
                "acted on {id} via {action}; background={} verification changed={} activity={visual} semantic={semantic} fallback={fallback} window={window_state}",
                u8::from(background),
                changed
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "null".into())
            ),
        ))
    }
}
fn audit(args: &[String]) -> Reply {
    let app = args.first().ok_or("usage: cu audit APP [--max-issues N]")?;
    let mut max = 50;
    let mut i = 1;
    while i < args.len() {
        if args[i] != "--max-issues" {
            return Err(format!("unknown audit option '{}'", args[i]));
        }
        max = option_value(args, &mut i)?
            .parse::<usize>()
            .map_err(|_| "--max-issues must be a positive integer")?;
        if max == 0 {
            return Err("--max-issues must be a positive integer".into());
        }
        i += 1;
    }
    let out = match native_ok(&strings(&["ax-tree", app, "--all"])) {
        Ok(o) => o,
        Err(_) => {
            return fail(
                "audit_failed",
                format!("could not read Accessibility tree for '{app}'"),
            );
        }
    };
    let (mut elements, mut count_interactive, mut generic, mut unnamed) = (0, 0, 0, 0);
    let mut labels: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut issues = Vec::new();
    for line in out.lines() {
        let e = Element::parse(line)?;
        elements += 1;
        if e.role == "AXGenericElement" {
            generic += 1
        }
        if !e.actions.is_empty() {
            count_interactive += 1;
            if e.name.is_empty() {
                unnamed += 1;
                if issues.len() < max {
                    issues
                        .push(json!({"kind":"unnamed_interactive","role":e.role,"bounds":e.bounds}))
                }
            } else {
                *labels.entry((e.role, e.name)).or_default() += 1
            }
        }
    }
    for ((role, name), count) in labels {
        if count > 1 && issues.len() < max {
            issues.push(
                json!({"kind":"duplicate_accessible_name","role":role,"name":name,"count":count}),
            )
        }
    }
    record(&format!(
        "audit {app} elements={elements} interactive={count_interactive} unnamed={unnamed} generic={generic}"
    ));
    success(
        json!({"ok":true,"app":app,"summary":{"elements":elements,"interactive":count_interactive,"generic_elements":generic,"unnamed_interactive":unnamed},"issues":issues}),
    )
}
fn wait(args: &[String], json_mode: bool) -> Reply {
    if args.len() == 1 && args[0].parse::<u64>().is_ok() {
        let ms = args[0].parse::<u64>().unwrap();
        return cu_tools::execute(
            &click_helper(),
            &cu_tools::Invocation {
                args: vec![format!("w:{ms}")],
                stdin: None,
            },
            Duration::from_millis(ms).saturating_add(Duration::from_secs(5)),
        )
        .map_err(|e| e.to_string());
    }
    let mode = args
        .first()
        .ok_or("usage: cu wait MS | --element|--value|--window|--stable|--changed APP [TARGET]")?;
    if !matches!(
        mode.as_str(),
        "--window" | "--element" | "--value" | "--stable" | "--changed"
    ) {
        return Err(
            "usage: cu wait MS | --window|--element|--value|--stable|--changed APP [TARGET]".into(),
        );
    }
    let visual = matches!(mode.as_str(), "--stable" | "--changed");
    let app = args.get(1).ok_or("wait requires an app")?;
    let target = if visual {
        ""
    } else {
        args.get(2).ok_or("wait requires a target")?
    };
    let mut role = String::new();
    let mut gone = false;
    let mut timeout = std::env::var("CU_WAIT_TIMEOUT_MS")
        .unwrap_or_else(|_| "2000".into())
        .parse::<u64>()
        .map_err(|_| "wait timeout must be milliseconds")?;
    let mut i = if visual { 2 } else { 3 };
    while i < args.len() {
        match args[i].as_str() {
            "--role" if mode != "--window" => role = option_value(args, &mut i)?,
            "--gone" => gone = true,
            "--timeout" => {
                timeout = option_value(args, &mut i)?
                    .parse()
                    .map_err(|_| "wait timeout must be milliseconds")?
            }
            _ => return Err(format!("unknown wait option '{}'", args[i])),
        }
        i += 1;
    }
    let start = Instant::now();
    let mut previous = None;
    loop {
        let mut ready = None;
        if visual {
            if let Ok(current) = fingerprint(app) {
                if let Some(last) = &previous
                    && ((mode == "--stable" && last == &current)
                        || (mode == "--changed" && last != &current))
                {
                    ready = Some(format!("window {}: {app}", mode.trim_start_matches('-')))
                }
                if previous.is_none() || mode == "--stable" {
                    previous = Some(current)
                }
            }
        } else {
            let command = if mode == "--window" {
                "windows"
            } else {
                "ax-tree"
            };
            // Failed inspection is unknown state, never proof of disappearance.
            let remaining = Duration::from_millis(timeout)
                .saturating_sub(start.elapsed())
                .max(Duration::from_millis(1))
                .min(Duration::from_secs(5));
            if let Ok((0, out)) = cu_tools::execute(
                &helper(),
                &cu_tools::Invocation {
                    args: strings(&[command, app]),
                    stdin: None,
                },
                remaining,
            ) {
                let found = out.lines().any(|line| {
                    let f: Vec<_> = line.split('\t').collect();
                    match mode.as_str() {
                        "--window" => f.get(7) == Some(&target),
                        "--element" => {
                            f.get(2) == Some(&target)
                                && (role.is_empty() || f.get(1) == Some(&role.as_str()))
                        }
                        "--value" => f.get(8) == Some(&target),
                        _ => false,
                    }
                });
                if found != gone {
                    ready = Some(format!(
                        "{} {}: {app} — {target}",
                        if mode == "--window" {
                            "window"
                        } else {
                            "condition"
                        },
                        if gone { "gone" } else { "ready" }
                    ))
                }
            }
        }
        if let Some(text) = ready {
            return if json_mode && mode != "--window" {
                success(json!({"ok":true,"condition":text}))
            } else {
                Ok((0, text))
            };
        }
        if start.elapsed() >= Duration::from_millis(timeout) {
            return Err(format!(
                "timed out waiting for {} in '{app}'",
                mode.trim_start_matches('-')
            ));
        }
        std::thread::sleep(
            Duration::from_millis(30)
                .min(Duration::from_millis(timeout).saturating_sub(start.elapsed())),
        );
    }
}
fn open(args: &[String]) -> Reply {
    let app = args
        .first()
        .filter(|s| !s.is_empty())
        .ok_or("usage: cu open APP")?;
    if native_ok(&strings(&["activate", app])).is_err() {
        let result = cu_tools::execute(
            std::path::Path::new("/usr/bin/open"),
            &cu_tools::Invocation {
                args: strings(&["-a", app]),
                stdin: None,
            },
            Duration::from_secs(5),
        )
        .map_err(|e| e.to_string())?;
        if result.0 != 0 {
            return Err(format!("could not activate '{app}'"));
        }
    }
    record(&format!("activate {app}"));
    Ok((0, format!("activated {app}")))
}
fn batch_words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut started = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some('\'') => {
                if c == '\'' {
                    quote = None
                } else {
                    current.push(c)
                }
            }
            Some('"') => match c {
                '"' => quote = None,
                '\\' => {
                    let next = chars.next().ok_or("unfinished escape")?;
                    if !matches!(next, '"' | '\\' | '$' | '`' | '\n') {
                        current.push('\\');
                    }
                    if next != '\n' {
                        current.push(next);
                    }
                }
                _ => current.push(c),
            },
            _ => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    started = true
                }
                '\\' => {
                    current.push(chars.next().ok_or("unfinished escape")?);
                    started = true;
                }
                c if c.is_whitespace() => {
                    if started {
                        words.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                ';' | '|' | '&' | '<' | '>' | '`' | '(' | ')' => {
                    return Err("shell operators are not supported; quote literal text".into());
                }
                _ => {
                    current.push(c);
                    started = true;
                }
            },
        }
    }
    if quote.is_some() {
        return Err("unterminated quote".into());
    }
    if started {
        words.push(current)
    }
    Ok(words)
}
fn is_point(s: &str) -> bool {
    let text = s.strip_prefix('-').unwrap_or(s);
    let mut parts = text.split('.');
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next();
    !whole.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit())
        && fraction.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
        && parts.next().is_none()
        && s.parse::<f64>()
            .is_ok_and(|v| v.is_finite() && v.abs() <= 100000.0)
}
fn batch_options(
    a: &[String],
    flags: &[&str],
    values: &[&str],
    positive: &[&str],
    nonnegative: &[&str],
) -> bool {
    let mut i = 0;
    while i < a.len() {
        let flag = a[i].as_str();
        if flags.contains(&flag) {
            i += 1;
            continue;
        }
        if !values.contains(&flag) && !positive.contains(&flag) && !nonnegative.contains(&flag) {
            return false;
        }
        i += 1;
        let Some(value) = a.get(i) else { return false };
        if positive.contains(&flag) && !value.parse::<u64>().is_ok_and(|n| n > 0) {
            return false;
        }
        if nonnegative.contains(&flag) && value.parse::<u64>().is_err() {
            return false;
        }
        if matches!(flag, "--since" | "--snapshot") && !valid_snapshot(value) {
            return false;
        }
        i += 1;
    }
    true
}
fn batch_valid(words: &[String]) -> bool {
    let Some((command, a)) = words.split_first() else {
        return false;
    };
    match command.as_str() {
        "click" | "dclick" | "rclick" => {
            (a.len() == 2 && a.iter().all(|v| is_point(v)))
                || (a.len() == 4
                    && matches!(a[0].as_str(), "--window" | "--app")
                    && is_point(&a[2])
                    && is_point(&a[3]))
        }
        "move" => a.len() == 2 && a.iter().all(|v| is_point(v)),
        "drag" => a.len() == 4 && a.iter().all(|v| is_point(v)),
        "type" => {
            let mut i = 0;
            let mut text = 0;
            while i < a.len() {
                match a[i].as_str() {
                    "--secret" | "--paced" => {}
                    "--stdin" => return false,
                    "--interval" => {
                        i += 1;
                        if !a.get(i).is_some_and(|s| s.parse::<u64>().is_ok()) {
                            return false;
                        }
                    }
                    _ => text += 1,
                }
                i += 1;
            }
            text == 1
        }
        "key" => a.len() == 1 && !a[0].is_empty(),
        "combo" => a.len() >= 2,
        "clickel" => {
            a.len() >= 2 && batch_options(&a[2..], &["--json"], &["--role"], &[], &["--index"])
        }
        "clicktext" => a.len() >= 2 && batch_options(&a[2..], &["--json"], &[], &[], &["--index"]),
        "find" => {
            a.len() >= 2
                && a[2..]
                    .iter()
                    .all(|s| matches!(s.as_str(), "--all" | "--json"))
        }
        "open" => a.len() == 1 && !a[0].is_empty(),
        "observe" => {
            !a.is_empty()
                && batch_options(
                    &a[1..],
                    &["--all", "--background", "--native-only", "--json"],
                    &["--since", "--role", "--name", "--filter"],
                    &["--max-elements"],
                    &[],
                )
        }
        "act" => {
            !a.is_empty()
                && valid_element(&a[0])
                && a.windows(2).any(|w| w[0] == "--snapshot")
                && batch_options(
                    &a[1..],
                    &["--background", "--native-only", "--no-verify", "--json"],
                    &["--snapshot"],
                    &[],
                    &[],
                )
        }
        "shot" => {
            let mut i = 0;
            let mut name = false;
            while i < a.len() {
                if matches!(a[i].as_str(), "-w" | "--window") {
                    i += 1;
                    if i >= a.len() {
                        return false;
                    }
                } else {
                    if name
                        || a[i].contains(['/', '\\', '\n', '\r'])
                        || matches!(a[i].as_str(), "" | "." | "..")
                    {
                        return false;
                    }
                    name = true;
                }
                i += 1;
            }
            true
        }
        "wait" => a.first().is_some_and(|s| match s.as_str() {
            "--window" => {
                a.len() >= 3
                    && batch_options(&a[3..], &["--gone", "--json"], &[], &[], &["--timeout"])
            }
            "--element" | "--value" => {
                a.len() >= 3
                    && batch_options(
                        &a[3..],
                        &["--gone", "--json"],
                        &["--role"],
                        &[],
                        &["--timeout"],
                    )
            }
            "--stable" | "--changed" => {
                a.len() >= 2
                    && batch_options(
                        &a[2..],
                        &["--gone", "--json"],
                        &["--role"],
                        &[],
                        &["--timeout"],
                    )
            }
            _ => a.len() == 1 && s.parse::<u64>().is_ok(),
        }),
        "scroll" => {
            let mut i = 0;
            while i < a.len() {
                match a[i].as_str() {
                    "--app" => {
                        i += 2;
                        if i > a.len() {
                            return false;
                        }
                    }
                    "--at" => {
                        if i + 2 >= a.len() || !is_point(&a[i + 1]) || !is_point(&a[i + 2]) {
                            return false;
                        }
                        i += 3
                    }
                    _ => break,
                }
            }
            i < a.len()
                && matches!(a[i].as_str(), "up" | "down" | "top" | "bottom")
                && (a.len() == i + 1
                    || (a.len() == i + 2 && a[i + 1].parse::<u64>().is_ok_and(|n| n > 0)))
        }
        _ => false,
    }
}
fn batch(args: &[String], json_mode: bool) -> Reply {
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        return Ok((0,"usage: cu batch --stdin\n\nRead one action per line, validate the complete script, then execute it.\nQuote arguments containing spaces; shell evaluation is not supported.\nActions: click dclick rclick move drag type key combo scroll clickel clicktext open wait act observe find shot\nBatch validates syntax before any input is sent. UI-state errors can still occur during execution.".into()));
    }
    if args != ["--stdin"] {
        return Err("usage: cu batch --stdin (try: cu batch --help)".into());
    }
    let mut body = String::new();
    std::io::stdin()
        .take(1_048_577)
        .read_to_string(&mut body)
        .map_err(|_| "batch script must be UTF-8 text")?;
    if body.len() > 1_048_576 {
        return Err("batch script exceeds 1 MiB limit".into());
    }
    let mut actions = Vec::new();
    for (index, line) in body.lines().enumerate() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let words = batch_words(line).map_err(|e| format!("batch line {}: {e}", index + 1))?;
        if !batch_valid(&words) {
            return Err(format!(
                "batch line {}: invalid or unsupported '{}' action",
                index + 1,
                words.first().map(String::as_str).unwrap_or("")
            ));
        }
        actions.push((index + 1, line, words));
        if actions.len() > 1024 {
            return Err("batch exceeds 1024 action limit".into());
        }
    }
    let mut results = Vec::new();
    let mut text = Vec::new();
    let mut failed = false;
    for (line, source, mut args) in actions {
        let secret = args[0] == "type" && args.iter().any(|s| s == "--secret");
        if json_mode {
            args.insert(0, "--json".into());
        }
        let result = crate::cli_input::dispatch(&args)
            .or_else(|| crate::cli_media::dispatch(&args))
            .or_else(|| crate::cli_pointer::dispatch(&args))
            .or_else(|| crate::cli_misc::dispatch(&args))
            .or_else(|| dispatch(&args))
            .unwrap_or_else(|| Err("batch operation has no Rust handler".into()));
        let (code, output) = match result {
            Ok(r) => r,
            Err(e) => (1, json!({"ok":false,"error":e}).to_string()),
        };
        let mut item = json!({"line":line,"command":if secret{"type --secret [redacted]"}else{source},"ok":code==0});
        if let Ok(value) = serde_json::from_str::<Value>(&output) {
            item["result"] = value;
        } else {
            item["output"] = json!(output);
        }
        results.push(item);
        text.push(output);
        if code != 0 {
            failed = true;
            break;
        }
    }
    if json_mode {
        Ok((
            i32::from(failed),
            json!({"ok":!failed,"actions":results}).to_string(),
        ))
    } else {
        text.push(if failed {
            format!("batch stopped after {} action(s)", results.len())
        } else {
            format!("batch completed {} actions", results.len())
        });
        Ok((i32::from(failed), text.join("\n")))
    }
}
fn select_file_args(args: &[String]) -> Result<Vec<String>, String> {
    if !matches!(args.len(), 7 | 9) || args[0].is_empty() || args[0].starts_with('-') {
        return Err("usage: cu select-file APP --pid PID --window-id ID --path PATH [--expected-source JSON]".into());
    }
    let mut pid = None;
    let mut id = None;
    let mut path = None;
    let mut expected_source = None;
    for pair in args[1..].as_chunks::<2>().0 {
        match pair[0].as_str() {
            "--pid" if pid.is_none() => {
                pid = Some(
                    pair[1]
                        .parse::<i32>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or("invalid picker PID")?,
                )
            }
            "--window-id" if id.is_none() => {
                id = Some(
                    pair[1]
                        .parse::<u32>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or("invalid picker window ID")?,
                )
            }
            "--expected-source" if expected_source.is_none() => {
                if pair[1].len() > 4096
                    || !serde_json::from_str::<Value>(&pair[1]).is_ok_and(|value| value.is_object())
                {
                    return Err("invalid picker expected source".into());
                }
                expected_source = Some(pair[1].clone());
            }
            "--path" if path.is_none() => {
                if !std::path::Path::new(&pair[1]).is_absolute()
                    || pair[1].len() > 4096
                    || pair[1].chars().any(char::is_control)
                {
                    return Err("invalid picker file path".into());
                }
                path = Some(pair[1].clone());
            }
            _ => return Err("invalid or duplicate picker argument".into()),
        }
    }
    let mut native_args = vec![
        "select-file".into(),
        args[0].clone(),
        pid.ok_or("missing picker PID")?.to_string(),
        id.ok_or("missing picker window ID")?.to_string(),
        path.ok_or("missing picker path")?,
    ];
    if let Some(expected) = expected_source {
        native_args.push(expected);
    }
    Ok(native_args)
}
fn command_arguments(args: &[String]) -> (bool, Vec<String>) {
    let mut json_mode = false;
    let mut start = 0;
    while args.get(start).is_some_and(|s| s == "--json") {
        json_mode = true;
        start += 1;
    }
    let Some(command) = args.get(start) else {
        return (json_mode, Vec::new());
    };
    let mut output = vec![command.clone()];
    let mut value = false;
    let positional = if command == "wait" {
        match args.get(start + 1).map(String::as_str) {
            Some("--window" | "--element" | "--value") => 3,
            Some("--stable" | "--changed") => 2,
            _ => 1,
        }
    } else if command == "windows" {
        0
    } else {
        1
    };
    for (index, arg) in args[start + 1..].iter().enumerate() {
        if value || index < positional {
            output.push(arg.clone());
            value = false;
            continue;
        }
        if arg == "--json" {
            json_mode = true;
            continue;
        }
        value = matches!(
            arg.as_str(),
            "--name"
                | "--filter"
                | "--role"
                | "--since"
                | "--max-elements"
                | "--snapshot"
                | "--pid"
                | "--window-id"
                | "--path"
                | "--expected-source"
                | "--max-issues"
                | "--timeout"
        );
        output.push(arg.clone());
    }
    (json_mode, output)
}
/// Returns `None` for commands owned by the other Rust dispatcher modules.
pub fn dispatch(args: &[String]) -> Option<Reply> {
    let (json_mode, filtered) = command_arguments(args);
    let (command, tail) = filtered.split_first()?;
    match command.as_str() {
        "document" => Some(
            if tail.len() != 1
                || tail[0].is_empty()
                || tail[0].starts_with('-')
                || tail[0].chars().any(char::is_control)
            {
                Err("usage: cu document APP [--json]".into())
            } else {
                native(&strings(&["file-context", &tail[0]]))
            },
        ),
        "read" => Some(
            if !matches!(tail.len(), 1 | 3)
                || tail[0].is_empty()
                || tail[0].starts_with('-')
                || (tail.len() == 3 && tail[1] != "--url")
            {
                Err("usage: cu read APP [--url HTTP(S)_URL] [--json]".into())
            } else {
                let mut args = vec!["document-context".into()];
                args.extend_from_slice(tail);
                native(&args)
            },
        ),
        "select-file" => Some(select_file_args(tail).and_then(|args| native(&args))),
        "observe" => Some(observe(tail)),
        "act" => Some(act(tail, json_mode)),
        "audit" => Some(audit(tail)),
        "open" => Some(open(tail)),
        "batch" => Some(batch(tail, json_mode)),
        "wait" => Some(wait(tail, json_mode)),
        "windows" => Some(if tail.len() > 1 {
            Err("usage: cu windows [APP]".into())
        } else {
            let mut a = vec!["windows".into()];
            a.extend_from_slice(tail);
            native(&a)
        }),
        "bounds" if helper().is_file() => Some(if tail.len() != 1 || tail[0].is_empty() {
            Err("usage: cu bounds APP".into())
        } else {
            native_ok(&strings(&["bounds", &tail[0]]))
                .and_then(|s| Window::parse(&s))
                .map(|w| {
                    (
                        0,
                        w.bounds
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(","),
                    )
                })
        }),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn document_command_normalizes_json_and_rejects_extra_arguments_without_native_access() {
        for args in [
            strings(&["--json", "document", "QuickTime Player"]),
            strings(&["document", "QuickTime Player", "--json"]),
        ] {
            let (json, args) = command_arguments(&args);
            assert!(json);
            assert_eq!(args, strings(&["document", "QuickTime Player"]));
        }
        for args in [
            strings(&["document"]),
            strings(&["document", ""]),
            strings(&["document", "--all"]),
            strings(&["document", "QuickTime Player", "extra"]),
            strings(&["document", "Bad\nApp"]),
        ] {
            assert!(
                dispatch(&args)
                    .unwrap()
                    .unwrap_err()
                    .contains("usage: cu document")
            );
        }
    }
    fn sample(path: &str) -> Element {
        Element::parse(&format!(
            "{path}\tAXButton\tRemove attachment clip.mov\t10\t20\t30\t40\tAXPress\t"
        ))
        .unwrap()
    }
    #[test]
    fn select_file_command_is_literal_and_strictly_bound() {
        let args = strings(&[
            "Fixture",
            "--pid",
            "42",
            "--window-id",
            "99",
            "--path",
            "/tmp/clip ; $literal.mov",
        ]);
        assert_eq!(
            select_file_args(&args).unwrap(),
            strings(&[
                "select-file",
                "Fixture",
                "42",
                "99",
                "/tmp/clip ; $literal.mov"
            ])
        );
        let expected=json!({"bytes":3,"identity":{"device":1,"inode":2,"modified_seconds":3,"modified_nanoseconds":4,"changed_seconds":5,"changed_nanoseconds":6}}).to_string();
        let mut bound = args.clone();
        bound.extend(["--expected-source".to_owned(), expected.clone()]);
        assert_eq!(select_file_args(&bound).unwrap().last(), Some(&expected));
        for args in [
            strings(&[
                "Fixture",
                "--pid",
                "0",
                "--window-id",
                "99",
                "--path",
                "/tmp/file",
            ]),
            strings(&[
                "Fixture",
                "--pid",
                "42",
                "--pid",
                "99",
                "--path",
                "/tmp/file",
            ]),
            strings(&[
                "Fixture",
                "--pid",
                "42",
                "--window-id",
                "99",
                "--path",
                "relative",
            ]),
        ] {
            assert!(select_file_args(&args).is_err());
        }
        let (_, args) = command_arguments(&strings(&[
            "select-file",
            "Fixture",
            "--pid",
            "42",
            "--window-id",
            "99",
            "--path",
            "--json",
        ]));
        assert_eq!(args.last().unwrap(), "--json");
    }
    #[test]
    fn draft_values_preserve_exact_json_without_entering_action_snapshot() {
        let element =
            Element::parse("w0.2\tAXTextArea\tMessage\t10\t20\t30\t40\tAXFocus\t").unwrap();
        let stored = element.tsv();
        let caption = "Line one\r\nLine two\t😀";
        let mut result = json!({"elements":[element.json()],"truncated":false});
        apply_draft_values(
            &mut result,
            &json!({"requested":true,"complete":true,"fields":[{"path":"w0.2","role":"AXTextArea","name":"Message","value":caption}]}),
        );
        assert_eq!(result["elements"][0]["value"], caption);
        assert_eq!(result["draft_values"]["complete"], true);
        assert_eq!(element.tsv(), stored);
        assert!(!stored.contains(caption));
    }
    #[test]
    fn draft_capture_is_never_implicit_or_claimed_complete_when_unavailable() {
        let context =
            "QUALITY\t{\"draft_values\":{\"requested\":true,\"complete\":true,\"fields\":[]}}";
        assert_eq!(
            draft_capture(context, false, "native_ax")["requested"],
            false
        );
        assert_eq!(
            draft_capture(context, true, "vision_ocr")["complete"],
            false
        );
        let mut result = json!({"elements":[],"truncated":false});
        apply_draft_values(&mut result, &draft_capture("", true, "native_ax"));
        assert_eq!(result["draft_values"]["complete"], false);
        assert_eq!(result["truncated"], true);
    }
    #[test]
    fn draft_capture_missing_changed_or_oversized_fields_marks_incomplete() {
        for field in [
            json!({"path":"w0.9","role":"AXTextArea","name":"Message","value":"caption"}),
            json!({"path":"w0.2","role":"AXTextArea","name":"Changed field","value":"caption"}),
            json!({"path":"w0.2","role":"AXTextArea","name":"Message","value":"x".repeat(8001)}),
        ] {
            let element =
                Element::parse("w0.2\tAXTextArea\tMessage\t10\t20\t30\t40\tAXFocus\t").unwrap();
            let mut result = json!({"elements":[element.json()],"truncated":false});
            apply_draft_values(
                &mut result,
                &json!({"requested":true,"complete":true,"fields":[field]}),
            );
            assert_eq!(result["draft_values"]["complete"], false);
            assert_eq!(result["truncated"], true);
        }
    }
    #[test]
    fn json_exposes_only_valid_native_ax_paths() {
        assert_eq!(sample("w0.12.3").json()["path"], "w0.12.3");
        for invalid in ["-", "w", "w0..1", "w0.a", "../w0", "w0.1/"] {
            assert!(sample(invalid).json().get("path").is_none());
        }
        let mut ocr = sample("w0.12.3");
        ocr.source = "vision_ocr".into();
        assert!(ocr.json().get("path").is_none());
    }
    #[test]
    fn actual_output_cap_is_distinct_from_exact_limit_and_duplicate_nodes() {
        let (elements, capped) =
            select_elements(vec![sample("w0.1"), sample("w0.2")], true, "", "", 2);
        assert_eq!(elements.len(), 2);
        assert!(!capped);
        let (elements, capped) =
            select_elements(vec![sample("w0.1"), sample("w0.1")], true, "", "", 1);
        assert_eq!(elements.len(), 1);
        assert!(!capped);
        let (elements, capped) =
            select_elements(vec![sample("w0.1"), sample("w0.2")], true, "", "", 1);
        assert_eq!(elements.len(), 1);
        assert!(capped);
        assert_eq!(elements[0].id, "e_1");
        // Same label and bounds in distinct native card groups are distinct.
        let (elements, _) = select_elements(vec![sample("w0.1"), sample("w0.2")], true, "", "", 10);
        assert_eq!(elements.len(), 2);
    }
    #[test]
    fn filters_do_not_falsely_report_output_truncation() {
        let mut other = sample("w0.2");
        other.name = "Other".into();
        let (_, capped) = select_elements(vec![sample("w0.1"), other], true, "", "clip.mov", 1);
        assert!(!capped);
        let scope = observation_scope(
            "QUALITY\t{\"traversal_truncated\":false}",
            "native_ax",
            false,
            "AXButton",
            "clip.mov",
            1,
            false,
        );
        assert_eq!(scope["truncated"], false);
        assert_eq!(scope["selection"]["filtered"], true);
        assert_eq!(scope["selection"]["max_elements"], 1);
    }
    #[test]
    fn traversal_quality_is_preserved_and_missing_quality_stays_unknown() {
        for truncated in [true, false] {
            let context = format!("QUALITY\t{{\"traversal_truncated\":{truncated}}}");
            let scope = observation_scope(&context, "native_ax", true, "", "", usize::MAX, false);
            assert_eq!(scope["truncated"], truncated);
            assert_eq!(scope["traversal_truncated"], truncated);
            assert_eq!(scope["selection"]["filtered"], false);
            assert!(scope["selection"]["max_elements"].is_null());
        }
        for context in ["", "QUALITY\t{}", "QUALITY\tnot-json"] {
            let scope = observation_scope(context, "native_ax", true, "", "", 10, false);
            assert!(scope["truncated"].is_null());
            assert!(scope["traversal_truncated"].is_null());
        }
        let capped = observation_scope("", "native_ax", true, "", "", 1, true);
        assert_eq!(capped["truncated"], true);
        assert_eq!(capped["output_capped"], true);
        let ocr = observation_scope(
            "QUALITY\t{\"traversal_truncated\":false}",
            "vision_ocr",
            true,
            "",
            "",
            10,
            false,
        );
        assert!(ocr["traversal_truncated"].is_null());
    }
    #[test]
    fn command_flags_do_not_consume_selector_values() {
        let (mode, args) = command_arguments(&strings(&["observe", "Demo", "--name", "--json"]));
        assert!(!mode);
        assert_eq!(args, strings(&["observe", "Demo", "--name", "--json"]));
        let (mode, args) = command_arguments(&strings(&[
            "--json",
            "wait",
            "--window",
            "Demo",
            "--json",
            "--timeout",
            "50",
        ]));
        assert!(mode);
        assert_eq!(args[3], "--json");
    }
    #[test]
    fn batch_quotes_are_literal_and_malformed_input_is_rejected() {
        assert_eq!(
            batch_words("type \"hello $HOME; $(date)\"").unwrap(),
            strings(&["type", "hello $HOME; $(date)"])
        );
        assert_eq!(
            batch_words("open 'App Name'").unwrap(),
            strings(&["open", "App Name"])
        );
        assert!(batch_words("type 'unfinished").is_err());
        assert!(batch_words("open Calculator; open Safari").is_err());
        assert!(!batch_valid(&strings(&["type", "--stdin"])));
        assert!(!batch_valid(&strings(&["not-a-command", "3"])));
    }
    #[test]
    fn snapshots_remain_compatible_with_shell_tsv() {
        let source = "META\ts_12_34\t12\tDemo\t4242\t42\t100\t200\t800\t600\tcom.example.Demo\tDemo Window\nEL\te_1\tnative_ax\tw0.0\tAXButton\tSave\t125\t235\t50\t30\t1.000\tAXPress\t\n";
        let snapshot = parse_snapshot("s_12_34", source).unwrap();
        assert_eq!(snapshot.window.pid, 4242);
        assert_eq!(snapshot.elements[0].id, "e_1");
        assert_eq!(snapshot.elements[0].path, "w0.0");
        assert_eq!(snapshot.elements[0].value, "");
        assert_eq!(
            snapshot.elements[0].tsv(),
            source.lines().nth(1).unwrap().to_owned() + "\n"
        );
    }
    #[test]
    fn ocr_snapshots_keep_source_for_guarded_actions() {
        let source = "META\ts_12_34\t12\tDemo\t4242\t42\t100\t200\t800\t600\tcom.example.Demo\tDemo Window\nEL\te_1\tvision_ocr\t-\tAXOCRText\tSave\t125\t235\t50\t30\t0.99\tOCRClick\t\n";
        assert_eq!(
            parse_snapshot("s_12_34", source).unwrap().elements[0].source,
            "vision_ocr"
        );
    }
    #[test]
    fn modal_background_actions_bind_the_surviving_parent_not_front_sheet() {
        let expected = Window::parse("42\t4242\t0\t0\t800\t600\tDemo").unwrap();
        let rows = "43\t4242\t100\t100\t400\t300\tDemo\tOpen\n42\t4242\t0\t0\t800\t600\tDemo\tMain";
        assert_eq!(snapshot_window(rows, &expected).unwrap().id, 42);
        assert!(snapshot_window("42\t9999\t0\t0\t800\t600\tDemo", &expected).is_none());
        assert!(snapshot_window("43\t4242\t0\t0\t800\t600\tDemo", &expected).is_none());
        assert!(
            snapshot_window(
                &format!("{rows}\n42\t4242\t0\t0\t800\t600\tDemo"),
                &expected
            )
            .is_none()
        );
    }
    #[test]
    fn empty_window_title_is_not_a_missing_window() {
        let window = Window::parse("42\t4242\t0\t0\t800\t600\tDemo").unwrap();
        assert_eq!(window.title, "");
    }
    #[test]
    fn snapshot_names_reject_traversal() {
        for s in ["../file", "s_1/2_3", "s_1_", "s__2", "s_1_2.tsv"] {
            assert!(!valid_snapshot(s))
        }
        assert!(valid_snapshot("s_123_456"));
    }
    #[test]
    fn empty_native_values_remain_empty() {
        let e = Element::parse("w0.1\tAXButton\tSave\t10\t20\t30\t40\tAXPress\t").unwrap();
        assert_eq!(e.value, "");
        assert_eq!(e.bounds, [10, 20, 30, 40]);
        assert_eq!(e.json()["actions"], json!(["AXPress"]));
    }
    #[test]
    fn enabled_state_survives_snapshot_and_changes_signature() {
        let mut e =
            Element::parse("w0.1\tAXButton\tCheckout\t10\t20\t30\t40\tAXPress\t\tfalse").unwrap();
        assert_eq!(e.json()["enabled"], false);
        let disabled_signature = e.signature();
        e.id = "e_1".into();
        let source = format!(
            "META\ts_12_34\t12\tDemo\t4242\t42\t100\t200\t800\t600\tcom.example.Demo\tDemo Window\n{}",
            e.tsv()
        );
        assert_eq!(
            parse_snapshot("s_12_34", &source).unwrap().elements[0].enabled,
            Some(false)
        );
        e.enabled = Some(true);
        assert_ne!(disabled_signature, e.signature());
        assert!(
            Element::parse("w0.1\tAXButton\tCheckout\t10\t20\t30\t40\tAXPress\t\tmaybe").is_err()
        );
    }
    #[test]
    fn invalid_native_bounds_are_rejected() {
        assert!(Element::parse("w0\tAXButton\tName\tx\t2\t3\t4\tAXPress").is_err());
    }
    #[test]
    fn unowned_media_commands_remain_unhandled() {
        assert!(dispatch(&strings(&["shot"])).is_none());
    }
}
