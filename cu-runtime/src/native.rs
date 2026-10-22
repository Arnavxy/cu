//! Native macOS inspection. No shell, screenshots, or model dependency.
#![allow(non_snake_case)]
use cu_tools::input_lease;
use serde_json::{Value, json};
use std::{
    ffi::{CStr, CString, c_void},
    ptr,
    time::{Duration, Instant},
};
mod native_events;
mod native_file_picker;
mod native_finder_source;
mod native_media;
mod native_office_source;
type Ref = *const c_void;
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct Point {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct Size {
    w: f64,
    h: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct Rect {
    p: Point,
    s: Size,
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(v: Ref);
    fn CFRetain(v: Ref) -> Ref;
    fn CFGetTypeID(v: Ref) -> usize;
    fn CFEqual(a: Ref, b: Ref) -> bool;
    fn CFStringGetTypeID() -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn CFURLGetTypeID() -> usize;
    fn CFURLGetString(v: Ref) -> Ref;
    fn CFURLCreateWithString(a: Ref, s: Ref, base: Ref) -> Ref;
    fn CFURLCopyScheme(v: Ref) -> Ref;
    fn CFURLCopyUserName(v: Ref) -> Ref;
    fn CFURLCopyPassword(v: Ref) -> Ref;
    fn CFURLCopyHostName(v: Ref) -> Ref;
    fn CFURLCopyFileSystemPath(v: Ref, style: i32) -> Ref;
    fn CFBooleanGetTypeID() -> usize;
    fn CFStringCreateWithBytes(a: Ref, b: *const u8, n: isize, e: u32, x: bool) -> Ref;
    fn CFStringGetLength(v: Ref) -> isize;
    fn CFStringGetMaximumSizeForEncoding(n: isize, e: u32) -> isize;
    fn CFStringGetCString(v: Ref, b: *mut i8, n: isize, e: u32) -> bool;
    fn CFArrayGetCount(v: Ref) -> isize;
    fn CFArrayGetValueAtIndex(v: Ref, i: isize) -> Ref;
    fn CFDictionaryGetValue(v: Ref, k: Ref) -> Ref;
    fn CFNumberGetValue(v: Ref, t: i32, p: *mut c_void) -> bool;
    fn CFBooleanGetValue(v: Ref) -> bool;
    static kCFBooleanTrue: Ref;
}
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> Ref;
    fn AXUIElementCreateSystemWide() -> Ref;
    fn AXUIElementSetMessagingTimeout(e: Ref, t: f32) -> i32;
    fn AXUIElementCopyAttributeValue(e: Ref, k: Ref, v: *mut Ref) -> i32;
    fn AXUIElementCopyActionNames(e: Ref, v: *mut Ref) -> i32;
    fn AXUIElementSetAttributeValue(e: Ref, k: Ref, v: Ref) -> i32;
    fn AXUIElementPerformAction(e: Ref, a: Ref) -> i32;
    fn AXUIElementGetTypeID() -> usize;
    fn AXUIElementGetPid(e: Ref, pid: *mut i32) -> i32;
    fn AXUIElementIsAttributeSettable(e: Ref, k: Ref, out: *mut bool) -> i32;
    fn AXValueCreate(t: i32, v: *const c_void) -> Ref;
    fn AXValueGetTypeID() -> usize;
    fn AXValueGetValue(v: Ref, t: i32, p: *mut c_void) -> bool;
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(options: u32, id: u32) -> Ref;
    fn CGRectMakeWithDictionaryRepresentation(v: Ref, r: *mut Rect) -> bool;
    fn CGGetActiveDisplayList(n: u32, ids: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayBounds(id: u32) -> Rect;
    fn CGDisplayIsBuiltin(id: u32) -> u32;
    fn CGDisplayIsMain(id: u32) -> u32;
    fn CGDisplayMirrorsDisplay(id: u32) -> u32;
}
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}
#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(n: *const i8) -> Ref;
    fn sel_registerName(n: *const i8) -> Ref;
    fn objc_msgSend();
}
struct Owned(Ref);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) }
        }
    }
}
fn owned(p: Ref) -> Option<Owned> {
    (!p.is_null()).then_some(Owned(p))
}
fn string(s: &str) -> Owned {
    Owned(unsafe {
        CFStringCreateWithBytes(ptr::null(), s.as_ptr(), s.len() as isize, 0x08000100, false)
    })
}
fn text(v: Ref) -> String {
    unsafe {
        if v.is_null() {
            return String::new();
        }
        if CFGetTypeID(v) == CFURLGetTypeID() {
            return text(CFURLGetString(v));
        }
        if CFGetTypeID(v) != CFStringGetTypeID() {
            return String::new();
        }
        let n = CFStringGetMaximumSizeForEncoding(CFStringGetLength(v), 0x08000100) + 1;
        if !(1..=4_000_001).contains(&n) {
            return String::new();
        }
        let mut b = vec![0i8; n as usize];
        if !CFStringGetCString(v, b.as_mut_ptr(), n, 0x08000100) {
            return String::new();
        }
        CStr::from_ptr(b.as_ptr()).to_string_lossy().into_owned()
    }
}
fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}
fn canon(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}
fn array(v: Ref) -> Vec<Owned> {
    unsafe {
        if v.is_null() || CFGetTypeID(v) != CFArrayGetTypeID() {
            return vec![];
        }
        (0..CFArrayGetCount(v).min(10000))
            .map(|i| Owned(CFRetain(CFArrayGetValueAtIndex(v, i))))
            .collect()
    }
}
fn dict(v: Ref, k: &str) -> Ref {
    unsafe { CFDictionaryGetValue(v, string(k).0) }
}
fn number(v: Ref) -> f64 {
    let mut n = 0.;
    if !v.is_null() {
        unsafe {
            CFNumberGetValue(v, 6, &mut n as *mut _ as *mut c_void);
        }
    }
    n
}
fn attr(e: Ref, k: &str) -> Option<Owned> {
    let mut v = ptr::null();
    unsafe {
        if AXUIElementCopyAttributeValue(e, string(k).0, &mut v) != 0 {
            return None;
        }
    }
    owned(v)
}
fn astr(e: Ref, k: &str) -> String {
    attr(e, k).map(|v| clean(&text(v.0))).unwrap_or_default()
}
fn children(e: Ref, k: &str) -> Vec<Owned> {
    attr(e, k)
        .map(|v| {
            let values = array(v.0);
            if values
                .iter()
                .any(|child| unsafe { CFGetTypeID(child.0) != AXUIElementGetTypeID() })
            {
                vec![]
            } else {
                values
            }
        })
        .unwrap_or_default()
}
fn app(pid: i32) -> Owned {
    let a = Owned(unsafe { AXUIElementCreateApplication(pid) });
    unsafe {
        AXUIElementSetMessagingTimeout(a.0, 0.2);
    }
    a
}
// Chromium/Electron can expose only browser chrome until an assistive client
// requests the web accessibility tree. Request it once per inspection, never
// disable it or toggle user/browser scripting preferences. Some Chromium builds
// return NotImplemented after applying EnhancedUserInterface; only fresh tree
// content, not the setter result, proves that this succeeded.
fn prepare_web_accessibility(window: &Window) {
    if !chromium(&window.bundle) && !embedded(&window.bundle) {
        return;
    }
    let application = app(window.pid);
    let _ = attr(application.0, "AXRole");
    for attribute in ["AXManualAccessibility", "AXEnhancedUserInterface"] {
        if boolean(application.0, attribute) == Some(true) {
            return;
        }
        let name = string(attribute);
        let result = unsafe { AXUIElementSetAttributeValue(application.0, name.0, kCFBooleanTrue) };
        if result == 0 {
            return;
        }
    }
}

fn axwindows(pid: i32) -> Vec<Owned> {
    children(app(pid).0, "AXWindows")
}
fn frame(e: Ref) -> Option<Rect> {
    let p = attr(e, "AXPosition")?;
    let s = attr(e, "AXSize")?;
    let mut r = Rect::default();
    unsafe {
        if CFGetTypeID(p.0) != AXValueGetTypeID()
            || CFGetTypeID(s.0) != AXValueGetTypeID()
            || !AXValueGetValue(p.0, 1, &mut r.p as *mut _ as *mut c_void)
            || !AXValueGetValue(s.0, 2, &mut r.s as *mut _ as *mut c_void)
        {
            return None;
        }
    }
    (r.s.w > 0. && r.s.h > 0.).then_some(r)
}
fn near(a: Rect, b: Rect, t: f64) -> bool {
    [a.p.x - b.p.x, a.p.y - b.p.y, a.s.w - b.s.w, a.s.h - b.s.h]
        .iter()
        .all(|n| n.abs() <= t)
}
fn intersects(a: Rect, b: Rect) -> bool {
    a.p.x < b.p.x + b.s.w && b.p.x < a.p.x + a.s.w && a.p.y < b.p.y + b.s.h && b.p.y < a.p.y + a.s.h
}
fn sel(s: &str) -> Ref {
    unsafe { sel_registerName(CString::new(s).unwrap().as_ptr()) }
}
fn class(s: &str) -> Ref {
    unsafe { objc_getClass(CString::new(s).unwrap().as_ptr()) }
}
fn msg0(o: Ref, s: &str) -> Ref {
    unsafe {
        let f: unsafe extern "C" fn(Ref, Ref) -> Ref =
            std::mem::transmute(objc_msgSend as *const ());
        f(o, sel(s))
    }
}
fn running(pid: i32) -> Ref {
    unsafe {
        let f: unsafe extern "C" fn(Ref, Ref, i32) -> Ref =
            std::mem::transmute(objc_msgSend as *const ());
        f(
            class("NSRunningApplication"),
            sel("runningApplicationWithProcessIdentifier:"),
            pid,
        )
    }
}
fn application_version(pid: i32) -> Option<String> {
    let url = msg0(running(pid), "bundleURL");
    if url.is_null() {
        return None;
    }
    let value = unsafe {
        let message: unsafe extern "C" fn(Ref, Ref, Ref) -> Ref =
            std::mem::transmute(objc_msgSend as *const ());
        let app_bundle = message(class("NSBundle"), sel("bundleWithURL:"), url);
        if app_bundle.is_null() {
            return None;
        }
        message(
            app_bundle,
            sel("objectForInfoDictionaryKey:"),
            string("CFBundleShortVersionString").0,
        )
    };
    let version = text(value);
    (!version.is_empty()
        && version.len() <= 32
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())))
    .then_some(version)
}
fn bundle(pid: i32) -> String {
    text(msg0(running(pid), "bundleIdentifier"))
}
#[derive(Clone)]
struct Window {
    id: u32,
    pid: i32,
    name: String,
    title: String,
    b: Rect,
    bundle: String,
}
fn windows() -> Vec<Window> {
    window_inventory(true)
}
fn window_inventory(on_screen_only: bool) -> Vec<Window> {
    let Some(rows) =
        owned(unsafe { CGWindowListCopyWindowInfo(if on_screen_only { 17 } else { 16 }, 0) })
    else {
        return vec![];
    };
    array(rows.0)
        .iter()
        .filter_map(|row| {
            let v = row.0;
            let mut b = Rect::default();
            let br = dict(v, "kCGWindowBounds");
            if br.is_null()
                || !unsafe { CGRectMakeWithDictionaryRepresentation(br, &mut b) }
                || number(dict(v, "kCGWindowLayer")) != 0.
                || on_screen_only && number(dict(v, "kCGWindowAlpha")) <= 0.
                || b.s.w < 40.
                || b.s.h < 40.
            {
                return None;
            }
            let pid = number(dict(v, "kCGWindowOwnerPID")) as i32;
            let name = text(msg0(running(pid), "localizedName"));
            Some(Window {
                id: number(dict(v, "kCGWindowNumber")) as u32,
                pid,
                name: if name.is_empty() {
                    text(dict(v, "kCGWindowOwnerName"))
                } else {
                    name
                },
                title: text(dict(v, "kCGWindowName")),
                b,
                bundle: bundle(pid),
            })
        })
        .collect()
}
fn score(w: &Window, q: &str) -> i32 {
    let q = q.to_lowercase();
    let c = canon(&q);
    if c.is_empty() {
        return 0;
    }
    let a = w.name.to_lowercase();
    let b = w.bundle.to_lowercase();
    let t = w.title.to_lowercase();
    if a == q {
        100
    } else if canon(&a) == c {
        98
    } else if b == q {
        95
    } else if canon(&b).ends_with(&c) {
        92
    } else if a.starts_with(&q) {
        85
    } else if canon(&a).starts_with(&c) {
        82
    } else if a.contains(&q) {
        75
    } else if b.contains(&q) {
        70
    } else if t == q {
        65
    } else if t.contains(&q) {
        55
    } else {
        0
    }
}
fn strip(w: &Window, all: &[Window]) -> bool {
    w.title.trim().is_empty()
        && w.b.s.h <= 160.
        && w.b.s.w >= w.b.s.h * 4.
        && all.iter().any(|x| {
            x.id != w.id
                && x.pid == w.pid
                && x.b.s.h >= 240f64.max(w.b.s.h * 3.)
                && x.b.s.w >= w.b.s.w * 0.8
                && (w.b.p.x + w.b.s.w).min(x.b.p.x + x.b.s.w) - w.b.p.x.max(x.b.p.x)
                    >= w.b.s.w * 0.8
                && w.b.p.y >= x.b.p.y - 160.
                && w.b.p.y + w.b.s.h <= x.b.p.y + x.b.s.h + 160.
        })
}
fn choose<'a>(ws: &'a [Window], q: &str) -> Option<&'a Window> {
    ws.iter()
        .filter(|w| score(w, q) > 0)
        .filter(|w| {
            !strip(w, ws)
                || !ws
                    .iter()
                    .any(|x| x.pid == w.pid && score(x, q) >= score(w, q) && !strip(x, ws))
        })
        .max_by_key(|w| {
            (
                score(w, q),
                std::cmp::Reverse(ws.iter().position(|x| x.id == w.id).unwrap_or(0)),
            )
        })
}
fn coords(r: Rect) -> String {
    format!(
        "{}\t{}\t{}\t{}",
        r.p.x.floor() as i64,
        r.p.y.floor() as i64,
        (r.p.x + r.s.w).ceil() as i64 - r.p.x.floor() as i64,
        (r.p.y + r.s.h).ceil() as i64 - r.p.y.floor() as i64
    )
}
fn row(w: &Window) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}",
        w.id,
        w.pid,
        coords(w.b),
        clean(&w.name),
        clean(&w.title)
    )
}
const SAFE: &[&str] = &[
    "AXStaticText",
    "AXHeading",
    "AXLink",
    "AXButton",
    "AXMenuItem",
    // A popup selection is displayed read-only state, unlike editable text.
    "AXPopUpButton",
    "AXCell",
];
const INTERACTIVE: &[&str] = &[
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
];
struct Record {
    path: String,
    role: String,
    name: String,
    value: String,
    b: Rect,
    actions: Vec<String>,
    enabled: Option<bool>,
}
fn actions(e: Ref) -> Vec<String> {
    let mut p = ptr::null();
    if unsafe { AXUIElementCopyActionNames(e, &mut p) } != 0 {
        return vec![];
    }
    owned(p)
        .map(|a| {
            array(a.0)
                .iter()
                .map(|x| text(x.0))
                .filter(|x| x.starts_with("AX"))
                .collect()
        })
        .unwrap_or_default()
}
fn record(e: Ref, path: String) -> Option<Record> {
    record_with_text_limit(e, path, 200)
}
fn static_text_value(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        value.to_owned()
    } else if limit > 200 {
        value.chars().take(limit).collect()
    } else {
        String::new()
    }
}
fn record_with_text_limit(e: Ref, path: String, text_limit: usize) -> Option<Record> {
    let role = astr(e, "AXRole");
    if role.is_empty() {
        return None;
    }
    let b = frame(e)?;
    let value = if SAFE.contains(&role.as_str()) {
        let v = astr(e, "AXValue");
        static_text_value(&v, text_limit)
    } else {
        String::new()
    };
    let name = ["AXTitle", "AXDescription", "AXHelp"]
        .iter()
        .map(|k| astr(e, k))
        .find(|s| !s.is_empty())
        .unwrap_or_else(|| value.clone());
    Some(Record {
        path,
        role,
        name,
        value,
        b,
        actions: actions(e),
        enabled: boolean(e, "AXEnabled"),
    })
}
const DRAFT_FIELDS: usize = 12;
const DRAFT_FIELD_CHARS: usize = 8000;
const DRAFT_TOTAL_CHARS: usize = 24000;
fn draft_field_allowed(role: &str, name: &str, subrole: &str) -> bool {
    let lowered = name.to_lowercase();
    if subrole.to_lowercase().contains("secure")
        || [
            "password", "passcode", "secret", "search", "address", "url", "filter",
        ]
        .iter()
        .any(|part| lowered.contains(part))
    {
        return false;
    }
    role == "AXTextArea"
        || role == "AXTextField"
            && lowered
                .split(|c: char| !c.is_alphanumeric())
                .any(|word| ["message", "caption", "subject", "to"].contains(&word))
}
fn draft_value_fits(value: &str, count: usize, total: usize) -> bool {
    let chars = value.chars().count();
    count < DRAFT_FIELDS
        && chars <= DRAFT_FIELD_CHARS
        && total.saturating_add(chars) <= DRAFT_TOTAL_CHARS
        && !value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}
fn draft_values(
    pid: i32,
    records: &[Record],
    root_path: &str,
    root: Ref,
    deadline: Instant,
) -> Value {
    let mut fields = Vec::new();
    let mut total = 0usize;
    let mut complete = true;
    for observed in records
        .iter()
        .filter(|r| draft_field_allowed(&r.role, &r.name, ""))
    {
        if fields.len() >= DRAFT_FIELDS || Instant::now() >= deadline {
            complete = false;
            break;
        }
        if resolve(pid, root_path).is_none_or(|current| !unsafe { CFEqual(current.0, root) }) {
            complete = false;
            break;
        }
        let Some(element) = resolve(pid, &observed.path) else {
            complete = false;
            continue;
        };
        unsafe {
            AXUIElementSetMessagingTimeout(element.0, 0.04);
        }
        // Resolve the exact path again and compare its semantics and geometry;
        // a remounted field cannot contribute a stale caption to send review.
        let Some(current) = record(element.0, observed.path.clone()) else {
            complete = false;
            continue;
        };
        if current.role != observed.role
            || current.name != observed.name
            || !near(current.b, observed.b, 1.)
        {
            complete = false;
            continue;
        }
        if !draft_field_allowed(&current.role, &current.name, &astr(element.0, "AXSubrole")) {
            continue;
        }
        let Some(value) = attr(element.0, "AXValue") else {
            complete = false;
            continue;
        };
        if unsafe {
            CFGetTypeID(value.0) != CFStringGetTypeID()
                || CFStringGetLength(value.0) > (DRAFT_FIELD_CHARS * 2) as isize
        } {
            complete = false;
            continue;
        }
        let value = text(value.0);
        if Instant::now() >= deadline || !draft_value_fits(&value, fields.len(), total) {
            complete = false;
            continue;
        }
        total += value.chars().count();
        fields.push(
            json!({"path":current.path,"role":current.role,"name":current.name,"value":value}),
        );
    }
    json!({"requested":true,"complete":complete,"fields":fields})
}
struct Budget {
    deadline: Instant,
    visited: usize,
    truncated: bool,
    skipped: Vec<String>,
    draft_values: bool,
    document_text: bool,
}
impl Budget {
    fn new() -> Self {
        Self {
            deadline: Instant::now() + Duration::from_millis(1500),
            visited: 0,
            truncated: false,
            skipped: vec![],
            draft_values: false,
            document_text: false,
        }
    }
    fn consume(&mut self, depth: usize) -> bool {
        if depth > 64 || self.visited >= 3000 || Instant::now() >= self.deadline {
            self.truncated = true;
            false
        } else {
            self.visited += 1;
            true
        }
    }
}
fn walk(
    e: Ref,
    path: String,
    depth: usize,
    interactive: bool,
    budget: &mut Budget,
    rs: &mut Vec<Record>,
) {
    // Read siblings before descending into large browser/table subtrees. Native
    // file panels host their list through a remote service; depth-first walking
    // can spend the whole budget there and omit the neighboring Cancel/Open
    // controls. Original AX indices remain in every path regardless of order.
    let mut pending =
        std::collections::VecDeque::from([(Owned(unsafe { CFRetain(e) }), path, depth)]);
    while let Some((element, path, depth)) = pending.pop_front() {
        if budget.skipped.contains(&path) {
            continue;
        }
        if rs.len() >= 1500 {
            budget.truncated = true;
            return;
        }
        if !budget.consume(depth) {
            return;
        }
        let text_limit = if budget.document_text { 6000 } else { 200 };
        let observed = record_with_text_limit(element.0, path.clone(), text_limit);
        if budget.document_text
            && observed
                .as_ref()
                .is_some_and(|r| r.value.chars().count() >= text_limit)
        {
            budget.truncated = true;
        }
        if let Some(r) = observed
            && (!r.name.is_empty()
                || r.role == "AXSheet"
                || budget.draft_values && r.role == "AXTextArea")
            && (!interactive || INTERACTIVE.contains(&r.role.as_str()))
        {
            rs.push(r);
        }
        let descendants = children(element.0, "AXChildren");
        if pending.len().saturating_add(descendants.len()) > 3000 {
            budget.truncated = true;
            return;
        }
        for (index, child) in descendants.into_iter().enumerate() {
            pending.push_back((child, format!("{path}.{index}"), depth + 1));
        }
    }
}
fn visible_areas(
    e: Ref,
    path: String,
    depth: usize,
    b: Rect,
    budget: &mut Budget,
    out: &mut Vec<(Owned, String, Rect)>,
) {
    if !budget.consume(depth) || boolean(e, "AXHidden") == Some(true) {
        return;
    }
    if astr(e, "AXRole") == "AXWebArea" {
        if let Some(f) = frame(e)
            && intersects(f, b)
            && f.s.w >= b.s.w * 0.4
            && f.s.h > 100.
        {
            out.push((Owned(unsafe { CFRetain(e) }), path, f));
        }
        return;
    }
    for (i, c) in children(e, "AXChildren").iter().enumerate() {
        visible_areas(c.0, format!("{path}.{i}"), depth + 1, b, budget, out);
        if budget.truncated || out.len() >= 4 {
            break;
        }
    }
}
fn chromium(b: &str) -> bool {
    [
        "com.google.Chrome",
        "com.google.Chrome.canary",
        "com.google.Chrome.beta",
        "com.google.Chrome.dev",
        "com.google.Chrome.forTesting",
        "org.chromium.Chromium",
        "com.microsoft.edgemac",
        "com.microsoft.edgemac.Beta",
        "com.microsoft.edgemac.Dev",
        "company.thebrowser.Browser",
        "com.brave.Browser",
    ]
    .contains(&b)
}
fn embedded(b: &str) -> bool {
    [
        "com.microsoft.teams",
        "com.microsoft.teams2",
        "com.tinyspeck.slackmacgap",
        "com.hnc.Discord",
        "com.microsoft.VSCode",
    ]
    .contains(&b)
}
fn browser(b: &str) -> bool {
    b == "com.apple.Safari" || chromium(b)
}
fn records(w: &Window, interactive: bool, draft_values: bool) -> (Vec<Record>, Value) {
    let deadline = Instant::now() + Duration::from_millis(1500);
    // The generic observe/context path needs the same assistive-client request
    // as document-context. Electron apps can otherwise expose window chrome
    // only, incorrectly forcing every task through OCR and screenshot loops.
    prepare_web_accessibility(w);
    loop {
        let result = records_until(w, interactive, draft_values, deadline);
        if result.1["web_content_unavailable"] != true
            || result.1["traversal_truncated"] == true
            || Instant::now() + Duration::from_millis(25) >= deadline
        {
            return result;
        }
        // Web trees arrive asynchronously. Re-read the same process/window,
        // keeping one deadline and never activating it or falling back to pixels.
        std::thread::sleep(Duration::from_millis(25));
    }
}
// Positive-only focus proof for remote modal sheets. AppKit's panel service
// may own the focused text field even when the client's NSApp.isActive is false.
fn focus_within(root: Ref) -> bool {
    let Some(system) = owned(unsafe { AXUIElementCreateSystemWide() }) else {
        return false;
    };
    unsafe {
        AXUIElementSetMessagingTimeout(system.0, 0.05);
    }
    let Some(mut current) = attr(system.0, "AXFocusedUIElement") else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_millis(75);
    for _ in 0..24 {
        if unsafe { CFEqual(current.0, root) } {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        let Some(parent) = attr(current.0, "AXParent") else {
            return false;
        };
        if unsafe { CFEqual(parent.0, current.0) } {
            return false;
        }
        current = parent;
    }
    false
}
fn records_until(
    w: &Window,
    interactive: bool,
    draft_values: bool,
    deadline: Instant,
) -> (Vec<Record>, Value) {
    let mut rs = vec![];
    let mut budget = Budget::new();
    budget.deadline = deadline;
    budget.draft_values = draft_values;
    let ws = axwindows(w.pid);
    let mut selected: Vec<(String, Owned)> = ws
        .iter()
        .enumerate()
        .filter(|(_, e)| frame(e.0).map(|f| near(f, w.b, 12.)).unwrap_or(false))
        .map(|(i, e)| (format!("w{i}"), Owned(unsafe { CFRetain(e.0) })))
        .collect();
    if selected.is_empty() {
        // AppKit open/save panels can be separate CG windows but AXSheet
        // descendants of the app's main AXWindow (including a remote panel
        // service). Match the exact observed CG rectangle to a native sheet;
        // never silently substitute the parent or a different front window.
        let until = deadline.min(Instant::now() + Duration::from_millis(300));
        let mut queue: std::collections::VecDeque<_> = ws
            .iter()
            .enumerate()
            .map(|(i, e)| (Owned(unsafe { CFRetain(e.0) }), format!("w{i}"), 0usize))
            .collect();
        let mut visited = 0;
        while let Some((element, path, depth)) = queue.pop_front() {
            if Instant::now() >= until || visited >= 120 {
                break;
            }
            visited += 1;
            let role = astr(element.0, "AXRole");
            if matches!(role.as_str(), "AXSheet" | "AXDialog")
                && frame(element.0).is_some_and(|f| near(f, w.b, 12.))
            {
                selected.push((path, element));
                continue;
            }
            if depth >= 5
                || matches!(
                    role.as_str(),
                    "AXBrowser" | "AXOutline" | "AXTable" | "AXWebArea"
                )
            {
                continue;
            }
            for (index, child) in children(element.0, "AXChildren")
                .into_iter()
                .enumerate()
                .take(120)
            {
                queue.push_back((child, format!("{path}.{index}"), depth + 1));
            }
        }
    }
    if selected.len() != 1 {
        selected.clear();
    }
    let draft_root = selected
        .first()
        .map(|(path, element)| (path.clone(), Owned(unsafe { CFRetain(element.0) })));
    let focused = selected
        .first()
        .is_some_and(|(_, element)| focus_within(element.0));
    let document = selected.first().and_then(|(_, element)| {
        let raw = attr(element.0, "AXDocument")?;
        local_file_document(&text(raw.0)).map(|mut data| {
            data["pid"] = json!(w.pid);
            data["window_id"] = json!(w.id);
            data
        })
    });
    let mut areas = vec![];
    if chromium(&w.bundle) || embedded(&w.bundle) {
        for (path, e) in &selected {
            let mut area_budget = Budget::new();
            area_budget.deadline = deadline;
            visible_areas(e.0, path.clone(), 0, w.b, &mut area_budget, &mut areas);
            budget.truncated |= area_budget.truncated;
        }
    }
    let before = budget.visited;
    for (e, p, _) in &areas {
        walk(e.0, p.clone(), 0, interactive, &mut budget, &mut rs)
    }
    let named = rs.len().saturating_sub(
        areas
            .iter()
            .filter(|(_, p, _)| rs.iter().any(|r| r.path == *p))
            .count(),
    );
    let descendants = (budget.visited - before).saturating_sub(areas.len());
    budget.skipped = areas.iter().map(|(_, p, _)| p.clone()).collect();
    for (path, e) in selected {
        walk(e.0, path, 0, interactive, &mut budget, &mut rs);
    }
    let mut quality = json!({"browser":chromium(&w.bundle),"embedded_web":embedded(&w.bundle),"web_area_count":areas.len(),"web_descendant_count":descendants,"sparse_web_area":(chromium(&w.bundle)||embedded(&w.bundle))&&!areas.is_empty()&&descendants==0,"web_content_unavailable":(chromium(&w.bundle)||embedded(&w.bundle))&&(areas.is_empty()||descendants==0||named==0),"traversal_truncated":budget.truncated,"web_content_bounds":areas.first().map(|(_,_,b)|json!({"x":b.p.x,"y":b.p.y,"width":b.s.w,"height":b.s.h}))});
    quality["focus_within_window"] = json!(focused);
    if draft_values {
        quality["draft_values"] = if let Some((path, root)) = draft_root {
            self::draft_values(
                w.pid,
                &rs,
                &path,
                root.0,
                Instant::now() + Duration::from_millis(350),
            )
        } else {
            json!({"requested":true,"complete":false,"fields":[]})
        };
    }

    if let Some(document) = document {
        quality["document"] = document;
    }
    (rs, quality)
}
// Resolve the represented file, never guess from a window title or read its
// contents. This lets media/document tasks find the actual attachment natively.
fn local_file_document(raw: &str) -> Option<Value> {
    if !valid_url(raw) {
        return None;
    }
    let url = owned(unsafe { CFURLCreateWithString(ptr::null(), string(raw).0, ptr::null()) })?;
    let scheme = owned(unsafe { CFURLCopyScheme(url.0) }).map(|s| text(s.0))?;
    let host = owned(unsafe { CFURLCopyHostName(url.0) })
        .map(|s| text(s.0))
        .unwrap_or_default();
    if !scheme.eq_ignore_ascii_case("file")
        || !["", "localhost"].contains(&host.to_ascii_lowercase().as_str())
    {
        return None;
    }
    let path = text(owned(unsafe { CFURLCopyFileSystemPath(url.0, 0) })?.0);
    if path.chars().any(char::is_control) || !std::path::Path::new(&path).is_absolute() {
        return None;
    }
    let metadata = std::fs::metadata(&path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    use std::os::unix::fs::MetadataExt;
    Some(
        json!({"source":"window_document","file_url":raw,"path":path,
        "identity":{"device":metadata.dev(),"inode":metadata.ino(),"modified_seconds":metadata.mtime(),"modified_nanoseconds":metadata.mtime_nsec(),"changed_seconds":metadata.ctime(),"changed_nanoseconds":metadata.ctime_nsec()},
        "bytes":metadata.len(),"read_only":true,"content_read":false,
        "instruction":"Represented local file from this exact app window, not a filename guess. File contents were not read. Revalidate the source before attaching; obtain explicit send approval before sending."}),
    )
}
fn same_file_window(a: &Window, b: &Window) -> bool {
    a.id == b.id && a.pid == b.pid && a.bundle == b.bundle && near(a.b, b.b, 1.)
}
fn file_context_envelope(w: &Window, mut document: Value) -> Value {
    document["pid"] = json!(w.pid);
    document["window_id"] = json!(w.id);
    json!({"ok":true,"read_only":true,"content_read":false,"actionable":false,
        "source":document["source"],"app":{"name":w.name,"pid":w.pid,"bundle_id":w.bundle},
        "window":{"id":w.id,"title":w.title},"document":document})
}
// Read only the represented-file attribute of one geometry-bound native
// window. This deliberately does not enumerate descendants, activate an app,
// capture pixels, read document bytes or manufacture actionable handles.
fn file_context(w: &Window, query: &str) -> Result<Value, &'static str> {
    let alias = |s: &str| match canon(s).as_str() {
        "quicktime" => "quicktimeplayer".into(),
        "chrome" => "googlechrome".into(),
        "applenotes" => "notes".into(),
        "teams" => "microsoftteams".into(),
        _ => canon(s),
    };
    if alias(query) != alias(&w.name) && !query.eq_ignore_ascii_case(&w.bundle) {
        return Err("file_context_owner_mismatch");
    }
    if !unsafe { AXIsProcessTrusted() } {
        return Err("file_context_permission_denied");
    }
    let matches: Vec<_> = axwindows(w.pid)
        .into_iter()
        .filter(|e| frame(e.0).is_some_and(|b| near(b, w.b, 12.)))
        .collect();
    if matches.len() != 1 {
        return Err("file_context_window_ambiguous");
    }
    let element = &matches[0];
    if w.bundle == "com.apple.finder" {
        return native_finder_source::resolve(w, element.0)
            .map(|document| file_context_envelope(w, document));
    }
    let raw = attr(element.0, "AXDocument").map(|v| text(v.0));
    let Some(document) = raw.as_deref().and_then(local_file_document) else {
        return native_office_source::resolve(w, element.0)
            .map(|document| file_context_envelope(w, document));
    };
    let second = attr(element.0, "AXDocument")
        .map(|v| text(v.0))
        .and_then(|raw| local_file_document(&raw));
    if second.as_ref() != Some(&document) {
        return Err("file_context_source_changed");
    }
    let current = window_inventory(false);
    if !current.iter().any(|next| same_file_window(w, next))
        || !frame(element.0).is_some_and(|b| near(b, w.b, 12.))
    {
        return Err("file_context_source_changed");
    }
    Ok(file_context_envelope(w, document))
}
fn rrow(r: &Record) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
        r.path,
        r.role,
        clean(&r.name),
        coords(r.b),
        r.actions.join(","),
        clean(&r.value),
        r.enabled.map(|v| v.to_string()).unwrap_or_default()
    )
}
// Context deliberately excludes credentials, query tokens and fragments. A
// selector therefore identifies an origin/path; duplicate matches are ambiguous.
fn document_key(s: &str) -> Option<String> {
    if !valid_url(s) || !(s.starts_with("https://") || s.starts_with("http://")) {
        return None;
    }
    let key = s.split(['?', '#']).next()?;
    let authority = key.split_once("://")?.1;
    Some(if authority.contains('/') {
        key.to_owned()
    } else {
        format!("{key}/")
    })
}
fn valid_url(s: &str) -> bool {
    if s.len() > 8192 || s.chars().any(|c| c.is_control()) {
        return false;
    }
    let Some(url) = owned(unsafe { CFURLCreateWithString(ptr::null(), string(s).0, ptr::null()) })
    else {
        return false;
    };
    let scheme = owned(unsafe { CFURLCopyScheme(url.0) })
        .map(|v| text(v.0).to_ascii_lowercase())
        .unwrap_or_default();
    if !["http", "https", "file"].contains(&scheme.as_str()) {
        return false;
    }
    if owned(unsafe { CFURLCopyUserName(url.0) }).is_some()
        || owned(unsafe { CFURLCopyPassword(url.0) }).is_some()
    {
        return false;
    }
    scheme == "file"
        || owned(unsafe { CFURLCopyHostName(url.0) })
            .map(|v| !text(v.0).is_empty())
            .unwrap_or(false)
}
fn unique_closest_window(frames: &[(usize, Rect)], target: Rect) -> Option<usize> {
    let mut candidates: Vec<_> = frames
        .iter()
        .map(|(i, b)| {
            (
                *i,
                (b.p.x - target.p.x).abs()
                    + (b.p.y - target.p.y).abs()
                    + (b.s.w - target.s.w).abs()
                    + (b.s.h - target.s.h).abs(),
            )
        })
        .filter(|(_, d)| d.is_finite() && *d <= 12.)
        .collect();
    candidates.sort_by(|a, b| a.1.total_cmp(&b.1));
    let first = candidates.first()?;
    if candidates
        .get(1)
        .map(|x| (x.1 - first.1).abs() < 0.001)
        .unwrap_or(false)
    {
        return None;
    }
    Some(first.0)
}
fn location(w: &Window) -> Option<Value> {
    location_until(w, Instant::now() + Duration::from_secs(2))
}
fn location_until(w: &Window, deadline: Instant) -> Option<Value> {
    if !unsafe { AXIsProcessTrusted() } || Instant::now() >= deadline {
        return None;
    }
    let ws = axwindows(w.pid);
    let frames: Vec<_> = ws
        .iter()
        .enumerate()
        .filter_map(|(i, e)| frame(e.0).map(|f| (i, f)))
        .collect();
    let e = &ws[unique_closest_window(&frames, w.b)?];
    let u = attr(e.0, "AXDocument")
        .map(|v| text(v.0))
        .unwrap_or_default();
    let (u, source) = if valid_url(&u) {
        (u, "window_document")
    } else {
        if w.bundle == "com.apple.finder" {
            return None;
        }
        let mut areas = vec![];
        visible_areas(
            e.0,
            "w0".into(),
            0,
            w.b,
            &mut Budget {
                deadline,
                visited: 0,
                truncated: false,
                skipped: vec![],
                draft_values: false,
                document_text: false,
            },
            &mut areas,
        );
        let u = areas.iter().find_map(|(e, _, _)| {
            ["AXURL", "AXDocument"]
                .iter()
                .map(|k| attr(e.0, k).map(|v| text(v.0)).unwrap_or_default())
                .find(|u| valid_url(u))
        })?;
        (u, "webarea_document")
    };
    Some(json!({"ok":true,"app":w.name,"pid":w.pid,"window_id":w.id,"url":u,"source":source}))
}
fn parse_ax_path(path: &str) -> Option<Vec<usize>> {
    if path.len() > 512 {
        return None;
    }
    let body = path.strip_prefix('w')?;
    let pieces: Vec<_> = body.split('.').collect();
    if pieces.len() > 65 {
        return None;
    }
    pieces
        .iter()
        .map(|s| {
            if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                None
            } else {
                s.parse::<usize>().ok().filter(|i| *i < 10000)
            }
        })
        .collect()
}
fn resolve(pid: i32, path: &str) -> Option<Owned> {
    if pid <= 0 {
        return None;
    }
    let indices = parse_ax_path(path)?;
    let mut e = axwindows(pid).into_iter().nth(*indices.first()?)?;
    for i in indices.iter().skip(1) {
        e = children(e.0, "AXChildren").into_iter().nth(*i)?;
    }
    Some(e)
}
fn perform(e: Ref) -> Option<String> {
    let role = astr(e, "AXRole");
    let focus =
        || unsafe { AXUIElementSetAttributeValue(e, string("AXFocused").0, kCFBooleanTrue) == 0 };
    if ["AXTextField", "AXTextArea", "AXComboBox"].contains(&role.as_str()) && focus() {
        return Some("AXFocus".into());
    }
    let a = actions(e);
    for k in ["AXPress", "AXOpen", "AXConfirm"] {
        if a.iter().any(|x| x == k) && unsafe { AXUIElementPerformAction(e, string(k).0) } == 0 {
            return Some(k.into());
        }
    }
    if focus() {
        return Some("AXFocus".into());
    }
    if a.iter().any(|x| x == "AXShowMenu")
        && unsafe { AXUIElementPerformAction(e, string("AXShowMenu").0) } == 0
    {
        Some("AXShowMenu".into())
    } else {
        None
    }
}
fn display_name(id: u32) -> String {
    let screens = msg0(class("NSScreen"), "screens");
    for screen in array(screens) {
        let dictionary = msg0(screen.0, "deviceDescription");
        let screen_number = unsafe {
            let f: unsafe extern "C" fn(Ref, Ref, Ref) -> Ref =
                std::mem::transmute(objc_msgSend as *const ());
            f(dictionary, sel("objectForKey:"), string("NSScreenNumber").0)
        };
        if number(screen_number) == id as f64 {
            return text(msg0(screen.0, "localizedName"));
        }
    }
    format!("Display {id}")
}
fn displays() -> Vec<u32> {
    let mut count = 0;
    unsafe {
        CGGetActiveDisplayList(0, ptr::null_mut(), &mut count);
    }
    let mut ids = vec![0; count as usize];
    if unsafe { CGGetActiveDisplayList(count, ids.as_mut_ptr(), &mut count) } != 0 {
        return vec![];
    }
    ids.truncate(count as usize);
    ids
}
fn error(code: i32, s: &str) -> Result<(), (i32, String)> {
    Err((code, s.into()))
}
fn frontmost() -> Ref {
    msg0(
        msg0(class("NSWorkspace"), "sharedWorkspace"),
        "frontmostApplication",
    )
}
fn pid_of(o: Ref) -> i32 {
    unsafe {
        let f: unsafe extern "C" fn(Ref, Ref) -> i32 =
            std::mem::transmute(objc_msgSend as *const ());
        f(o, sel("processIdentifier"))
    }
}
fn settable(e: Ref, key: &str) -> bool {
    let mut yes = false;
    unsafe { AXUIElementIsAttributeSettable(e, string(key).0, &mut yes) == 0 && yes }
}
fn boolean(e: Ref, key: &str) -> Option<bool> {
    attr(e, key).and_then(|v| {
        if unsafe { CFGetTypeID(v.0) == CFBooleanGetTypeID() } {
            Some(unsafe { CFBooleanGetValue(v.0) })
        } else {
            None
        }
    })
}
fn notes_target(requested: &str) -> Option<(i32, Owned)> {
    if !["notes", "comapplenotes"].contains(&canon(requested).as_str())
        || !unsafe { AXIsProcessTrusted() }
    {
        return None;
    }
    let front = frontmost();
    let pid = pid_of(front);
    if text(msg0(front, "bundleIdentifier")) != "com.apple.Notes" {
        return None;
    }
    let element = attr(app(pid).0, "AXFocusedUIElement")?;
    if unsafe { CFGetTypeID(element.0) != AXUIElementGetTypeID() } {
        return None;
    }
    let mut owner = 0;
    if unsafe { AXUIElementGetPid(element.0, &mut owner) } != 0 || owner != pid {
        return None;
    }
    let role = astr(element.0, "AXRole");
    let sub = astr(element.0, "AXSubrole").to_lowercase();
    if role != "AXTextArea"
        || sub.contains("secure")
        || sub.contains("password")
        || boolean(element.0, "AXProtectedContent") == Some(true)
        || boolean(element.0, "AXFocused") != Some(true)
        || boolean(element.0, "AXEnabled") == Some(false)
        || !settable(element.0, "AXSelectedText")
        || !settable(element.0, "AXSelectedTextRange")
        || pid_of(frontmost()) != pid
    {
        return None;
    }
    Some((pid, element))
}
#[repr(C)]
#[derive(Default, PartialEq)]
struct Range {
    location: isize,
    length: isize,
}
fn selection(e: Ref) -> Option<Range> {
    let v = attr(e, "AXSelectedTextRange")?;
    let mut r = Range::default();
    if unsafe { CFGetTypeID(v.0) } != unsafe { AXValueGetTypeID() }
        || !unsafe { AXValueGetValue(v.0, 4, &mut r as *mut _ as *mut c_void) }
    {
        return None;
    }
    Some(r)
}
fn typed_text(e: Ref, key: &str) -> Option<String> {
    let v = attr(e, key)?;
    (unsafe { CFGetTypeID(v.0) == CFStringGetTypeID() }).then(|| text(v.0))
}
fn notes_replacement(original: &str, range: &Range, insert: &str) -> Option<String> {
    let mut units: Vec<u16> = original.encode_utf16().collect();
    if units.len() > 1_000_000
        || range.location < 0
        || range.length < 0
        || range.location as usize > units.len()
        || range.length as usize > units.len() - range.location as usize
    {
        return None;
    }
    units.splice(
        range.location as usize..(range.location + range.length) as usize,
        insert.encode_utf16(),
    );
    String::from_utf16(&units).ok()
}
fn focused_notes(requested: &str, insert: Option<&str>) -> Value {
    let denied = json!({"ok":false,"focused":false,"editable":false,"secure":true,"error_code":"not_editable_notes_target"});
    let Some((pid, e)) = notes_target(requested) else {
        return denied;
    };
    let mut result = json!({"ok":true,"app":{"name":"Notes","bundle_id":"com.apple.Notes","pid":pid},"focused":true,"editable":true,"role":"AXTextArea","secure":false});
    if let Some(insert) = insert {
        result["pasted"] = json!(false);
        let fail = |mut v: Value, code: &str| {
            v["ok"] = json!(false);
            v["error_code"] = json!(code);
            v
        };
        if insert.chars().count() > 4000
            || insert.encode_utf16().count() > 8000
            || insert.contains('\0')
        {
            return fail(result, "invalid_notes_text");
        }
        let Some(original) = typed_text(e.0, "AXValue") else {
            return fail(result, "notes_target_changed");
        };
        let Some(range) = selection(e.0) else {
            return fail(result, "notes_target_changed");
        };
        let Some(expected) = notes_replacement(&original, &range, insert) else {
            return fail(result, "notes_target_changed");
        };
        if !notes_target(requested)
            .map(|(p, f)| p == pid && unsafe { CFEqual(f.0, e.0) })
            .unwrap_or(false)
            || pid_of(frontmost()) != pid
            || selection(e.0) != Some(range)
            || typed_text(e.0, "AXValue") != Some(original)
        {
            return fail(result, "notes_target_changed");
        }
        if unsafe {
            AXUIElementSetAttributeValue(e.0, string("AXSelectedText").0, string(insert).0)
        } != 0
        {
            return fail(result, "notes_write_rejected");
        }
        result["pasted"] = json!(true);
        let verified = typed_text(e.0, "AXValue") == Some(expected);
        result["verified"] = json!(verified);
        result["ok"] = json!(verified);
        if !verified {
            result["error_code"] = json!("notes_write_unverified")
        }
    }
    result
}
fn move_window(w: &Window, id: u32) -> Value {
    let fail = |s: &str| json!({"ok":false,"error":s});
    if !displays().contains(&id)
        || unsafe { CGDisplayMirrorsDisplay(id) } != 0
        || !unsafe { AXIsProcessTrusted() }
    {
        return fail("Target display unavailable, mirrored, or Accessibility disabled.");
    }
    let ws = axwindows(w.pid);
    let selected: Vec<_> = ws
        .iter()
        .filter(|e| frame(e.0).map(|f| near(f, w.b, 12.)).unwrap_or(false))
        .collect();
    if selected.len() != 1 {
        return fail("Use one identifiable window.");
    };
    let e = selected[0].0;
    if boolean(e, "AXFullScreen") == Some(true) || !settable(e, "AXPosition") {
        return fail("This window does not expose native movement.");
    }
    let b = unsafe { CGDisplayBounds(id) };
    let area = Rect {
        p: Point {
            x: b.p.x + 32.,
            y: b.p.y + 48.,
        },
        s: Size {
            w: b.s.w - 64.,
            h: b.s.h - 96.,
        },
    };
    if area.s.w < 200. || area.s.h < 150. {
        return fail("Display work area too small.");
    }
    let size = Size {
        w: w.b.s.w.min(area.s.w),
        h: w.b.s.h.min(area.s.h),
    };
    if size.w != w.b.s.w || size.h != w.b.s.h {
        if !settable(e, "AXSize") {
            return fail("Window cannot be fitted on display.");
        }
        let Some(v) = owned(unsafe { AXValueCreate(2, &size as *const _ as *const c_void) }) else {
            return fail("Size creation failed.");
        };
        if unsafe { AXUIElementSetAttributeValue(e, string("AXSize").0, v.0) } != 0 {
            return fail("Window resize rejected.");
        }
    }
    let p = Point {
        x: area.p.x + (area.s.w - size.w) / 2.,
        y: area.p.y + (area.s.h - size.h) / 2.,
    };
    let Some(v) = owned(unsafe { AXValueCreate(1, &p as *const _ as *const c_void) }) else {
        return fail("Position creation failed.");
    };
    if unsafe { AXUIElementSetAttributeValue(e, string("AXPosition").0, v.0) } != 0 {
        return fail("Native window movement failed; inspect before retrying.");
    }
    for _ in 0..10 {
        if let Some(f) = frame(e)
            && f.p.x >= b.p.x - 2.
            && f.p.y >= b.p.y - 2.
            && f.p.x + f.s.w <= b.p.x + b.s.w + 2.
            && f.p.y + f.s.h <= b.p.y + b.s.h + 2.
        {
            return json!({"ok":true,"verified":true,"app":w.name,"window_id":w.id,"display_id":id,"display_name":display_name(id),"bounds":[f.p.x,f.p.y,f.s.w,f.s.h],"previous_bounds":[w.b.p.x,w.b.p.y,w.b.s.w,w.b.s.h]});
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    fail("Window movement requested but destination unverified. Inspect before retrying.")
}
#[derive(Debug)]
struct TeamsDraftArgs {
    pid: i32,
    window: u32,
    conversation: String,
    path: String,
    name: String,
    bounds: Rect,
}
fn teams_draft_args(args: &[String], body: &str) -> Option<TeamsDraftArgs> {
    if args.len() != 9
        || body.trim().is_empty()
        || body.chars().count() > 8000
        || body
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return None;
    }
    let pid = args[0].parse::<i32>().ok().filter(|p| *p > 1)?;
    let window = args[1].parse::<u32>().ok().filter(|id| *id > 0)?;
    if args[2].trim().is_empty()
        || args[2].len() > 300
        || args[2].chars().any(char::is_control)
        || parse_ax_path(&args[3])?.len() < 3
        || args[4].len() > 300
        || args[4].chars().any(char::is_control)
        || !teams_composer_name(&args[4])
    {
        return None;
    }
    let coords = args[5..]
        .iter()
        .map(|v| {
            v.parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && n.abs() <= 100000.)
        })
        .collect::<Option<Vec<_>>>()?;
    if coords[2] <= 0. || coords[3] <= 0. {
        return None;
    }
    Some(TeamsDraftArgs {
        pid,
        window,
        conversation: args[2].clone(),
        path: args[3].clone(),
        name: args[4].clone(),
        bounds: Rect {
            p: Point {
                x: coords[0],
                y: coords[1],
            },
            s: Size {
                w: coords[2],
                h: coords[3],
            },
        },
    })
}
fn teams_composer_name(name: &str) -> bool {
    let name = name.to_lowercase();
    (name.contains("message") || name.contains("compose"))
        && ![
            "search", "password", "passcode", "secret", "address", "filter",
        ]
        .iter()
        .any(|part| name.contains(part))
}
fn teams_draft_existing(existing: &str, wanted: &str) -> Option<bool> {
    if existing == wanted {
        Some(true)
    } else if existing.is_empty() {
        Some(false)
    } else {
        None
    }
}
fn teams_draft_value(element: Ref) -> Option<String> {
    let value = attr(element, "AXValue")?;
    if unsafe { CFGetTypeID(value.0) != CFStringGetTypeID() || CFStringGetLength(value.0) > 16000 }
    {
        return None;
    }
    let value = text(value.0);
    (value.chars().count() <= 8000).then_some(value)
}
fn teams_draft(args: &[String], body: &str) -> Value {
    let failure = |code: &str, attempted: bool| json!({"ok":false,"error_code":code,"mutation_attempted":attempted,"uncertain":attempted,"sent":false});
    let Some(args) = teams_draft_args(args, body) else {
        return failure("teams_draft_invalid_arguments", false);
    };
    if !unsafe { AXIsProcessTrusted() } {
        return failure("teams_draft_permission", false);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let inventory = window_inventory(false);
    let Some(window) = inventory.iter().find(|w| {
        w.pid == args.pid
            && w.id == args.window
            && ["com.microsoft.teams", "com.microsoft.teams2"].contains(&w.bundle.as_str())
    }) else {
        return failure("teams_draft_source_changed", false);
    };
    prepare_web_accessibility(window);
    let (rows, quality) = records_until(window, false, true, deadline);
    if quality["traversal_truncated"] == true
        || quality["web_content_unavailable"] == true
        || quality["draft_values"]["complete"] != true
    {
        return failure("teams_draft_scope_incomplete", false);
    }
    let identity_rows = |rows: &[Record]| {
        rows.iter()
            .map(|r| json!({"source":"native_ax","role":r.role,"name":r.name,"path":r.path}))
            .collect::<Vec<_>>()
    };
    let initial_identity = identity_rows(&rows);
    let Some(header_proof) =
        cu_tools::teams_identity::conversation_proof(&initial_identity, &args.conversation)
    else {
        return failure("teams_draft_source_changed", false);
    };
    let Some(composer) = rows.iter().find(|r| r.path == args.path) else {
        return failure("teams_draft_source_changed", false);
    };
    if composer.role != "AXTextArea"
        || composer.name != args.name
        || composer.enabled == Some(false)
        || !near(composer.b, args.bounds, 4.)
    {
        return failure("teams_draft_source_changed", false);
    }
    let root_indices = parse_ax_path(&args.path).unwrap();
    let roots = axwindows(args.pid);
    let Some(root) = roots.get(root_indices[0]) else {
        return failure("teams_draft_source_changed", false);
    };
    if !frame(root.0).is_some_and(|b| near(b, window.b, 12.)) {
        return failure("teams_draft_source_changed", false);
    }
    let Some(element) = resolve(args.pid, &args.path) else {
        return failure("teams_draft_source_changed", false);
    };
    let mut owner = 0;
    if unsafe { AXUIElementGetPid(element.0, &mut owner) } != 0
        || owner != args.pid
        || !draft_field_allowed("AXTextArea", &args.name, &astr(element.0, "AXSubrole"))
        || boolean(element.0, "AXProtectedContent") == Some(true)
    {
        return failure("teams_draft_source_changed", false);
    }
    let Some(existing) = teams_draft_value(element.0) else {
        return failure("teams_draft_scope_incomplete", false);
    };
    let Some(already_present) = teams_draft_existing(&existing, body) else {
        return failure("teams_draft_existing", false);
    };
    if !already_present && !settable(element.0, "AXValue") {
        return failure("teams_draft_not_settable", false);
    }
    // Re-resolve both the heading and composer immediately before the one write.
    // Neither focus, clipboard, keyboard events nor an AXPress/Send is used.
    let Some(current) = resolve(args.pid, &args.path) else {
        return failure("teams_draft_source_changed", false);
    };
    // Re-resolve every identity-bearing native node before the atomic write;
    // this validates the document title, participant header/list/group together.
    let fresh_identity: Vec<Value> = initial_identity
        .iter()
        .filter(|r| {
            r["path"].as_str().is_some_and(|p| {
                p == header_proof.header_path
                    || p.starts_with(&format!("{}.", header_proof.header_path))
                    || header_proof.document_path.as_deref() == Some(p)
            })
        })
        .filter_map(|r| {
            let p = r["path"].as_str()?;
            resolve(args.pid, p)
                .and_then(|e| record(e.0, p.into()))
                .map(|r| json!({"source":"native_ax","role":r.role,"name":r.name,"path":r.path}))
        })
        .collect();
    let heading_unchanged =
        cu_tools::teams_identity::conversation_proof(&fresh_identity, &args.conversation)
            .is_some_and(|proof| proof == header_proof);
    if !unsafe { CFEqual(current.0, element.0) }
        || !heading_unchanged
        || teams_draft_value(current.0).as_deref() != Some(existing.as_str())
        || Instant::now() >= deadline
    {
        return failure("teams_draft_source_changed", false);
    }
    let attempted = !already_present;
    if attempted
        && unsafe { AXUIElementSetAttributeValue(current.0, string("AXValue").0, string(body).0) }
            != 0
    {
        return failure("teams_draft_unconfirmed", true);
    }
    let readback = teams_draft_value(current.0);
    if readback.as_deref() != Some(body) {
        return failure("teams_draft_unconfirmed", attempted);
    }
    let fresh_inventory = window_inventory(false);
    let Some(fresh_window) = fresh_inventory
        .iter()
        .find(|w| w.id == args.window && w.pid == args.pid && w.bundle == window.bundle)
    else {
        return failure("teams_draft_unconfirmed", attempted);
    };
    let (fresh, quality) = records_until(fresh_window, false, true, deadline);
    let same_header =
        cu_tools::teams_identity::conversation_proof(&identity_rows(&fresh), &args.conversation)
            .is_some_and(|proof| proof == header_proof);
    let same_field = fresh.iter().any(|r| {
        r.path == args.path
            && r.role == "AXTextArea"
            && r.name == args.name
            && near(r.b, args.bounds, 4.)
    });
    let exact = quality["draft_values"]["fields"]
        .as_array()
        .is_some_and(|fields| {
            fields
                .iter()
                .filter(|f| {
                    f["path"] == args.path
                        && f["role"] == "AXTextArea"
                        && f["name"] == args.name
                        && f["value"] == body
                })
                .count()
                == 1
        });
    if Instant::now() >= deadline
        || quality["traversal_truncated"] == true
        || quality["draft_values"]["complete"] != true
        || !same_header
        || !same_field
        || !exact
    {
        return failure("teams_draft_unconfirmed", attempted);
    }
    json!({"ok":true,"verified":true,"source":"native_ax","pid":args.pid,"window_id":args.window,"conversation":args.conversation,"composer_path":args.path,"exact_value_verified":true,"characters":body.chars().count(),"already_present":already_present,"mutation_attempted":attempted,"requires_send_review":true,"sent":false})
}

fn teams_search_name(name: &str) -> bool {
    matches!(
        name.trim().to_lowercase().as_str(),
        "search"
            | "search teams"
            | "search microsoft teams"
            | "search (⌘ e)"
            | "search (⌘e)"
            | "search (command+e)"
    )
}
struct TeamsSearchArgs {
    pid: i32,
    window: u32,
    path: String,
    role: String,
    name: String,
    bounds: Rect,
}
fn teams_search_args(args: &[String], query: &str) -> Option<TeamsSearchArgs> {
    if args.len() != 9
        || query.trim().is_empty()
        || query != query.trim()
        || query.chars().count() > 100
        || query.chars().any(char::is_control)
        || !matches!(args[3].as_str(), "AXTextField" | "AXComboBox")
        || !teams_search_name(&args[4])
    {
        return None;
    }
    // Share numeric/path validation with the guarded composer operation only;
    // search fields never use its broader text/composer acceptance rules.
    let mapped = vec![
        args[0].clone(),
        args[1].clone(),
        "search".into(),
        args[2].clone(),
        "Type a message".into(),
        args[5].clone(),
        args[6].clone(),
        args[7].clone(),
        args[8].clone(),
    ];
    let parsed = teams_draft_args(&mapped, query)?;
    Some(TeamsSearchArgs {
        pid: parsed.pid,
        window: parsed.window,
        path: parsed.path,
        role: args[3].clone(),
        name: args[4].clone(),
        bounds: parsed.bounds,
    })
}
fn teams_search_target<'a>(rows: &'a [Record], args: &TeamsSearchArgs) -> Option<&'a Record> {
    let mut candidates = rows.iter().filter(|r| {
        matches!(r.role.as_str(), "AXTextField" | "AXComboBox")
            && teams_search_name(&r.name)
            && r.enabled != Some(false)
    });
    let target = candidates.next()?;
    (candidates.next().is_none()
        && target.path == args.path
        && target.name == args.name
        && target.role == args.role
        && near(target.b, args.bounds, 4.))
    .then_some(target)
}
fn teams_search(args: &[String], query: &str) -> Value {
    let failure = |code: &str, attempted: bool| {
        json!({"ok":false,"error_code":code,
        "mutation_attempted":attempted,"uncertain":attempted,"sent":false})
    };
    let Some(args) = teams_search_args(args, query) else {
        return failure("teams_search_invalid_arguments", false);
    };
    if !unsafe { AXIsProcessTrusted() } {
        return failure("teams_search_permission", false);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let inventory = window_inventory(false);
    let Some(window) = inventory.iter().find(|w| {
        w.pid == args.pid
            && w.id == args.window
            && ["com.microsoft.teams", "com.microsoft.teams2"].contains(&w.bundle.as_str())
    }) else {
        return failure("teams_search_source_changed", false);
    };
    prepare_web_accessibility(window);
    let (rows, quality) = records_until(window, false, false, deadline);
    if quality["traversal_truncated"] == true || quality["web_content_unavailable"] == true {
        return failure("teams_search_scope_incomplete", false);
    }
    if teams_search_target(&rows, &args).is_none() {
        return failure("teams_search_source_changed", false);
    }
    let valid_element = |element: Ref| {
        let mut owner = 0;
        (unsafe { AXUIElementGetPid(element, &mut owner) }) == 0
            && owner == args.pid
            && astr(element, "AXRole") == args.role
            && !astr(element, "AXSubrole").to_lowercase().contains("secure")
            && boolean(element, "AXProtectedContent") != Some(true)
            && boolean(element, "AXEnabled") != Some(false)
            && record(element, args.path.clone())
                .is_some_and(|r| r.name == args.name && near(r.b, args.bounds, 4.))
    };
    let Some(element) = resolve(args.pid, &args.path) else {
        return failure("teams_search_source_changed", false);
    };
    let indices = parse_ax_path(&args.path).unwrap();
    let roots = axwindows(args.pid);
    if !roots
        .get(indices[0])
        .and_then(|root| frame(root.0))
        .is_some_and(|b| near(b, window.b, 12.))
        || !valid_element(element.0)
    {
        return failure("teams_search_source_changed", false);
    }
    let Some(existing) = teams_draft_value(element.0) else {
        return failure("teams_search_scope_incomplete", false);
    };
    let Some(already_present) = teams_draft_existing(&existing, query) else {
        return failure("teams_search_existing", false);
    };
    if !already_present && !settable(element.0, "AXValue") {
        return failure("teams_search_not_settable", false);
    }
    let Some(current) = resolve(args.pid, &args.path) else {
        return failure("teams_search_source_changed", false);
    };
    let fresh = window_inventory(false);
    if !fresh.iter().any(|w| {
        w.pid == args.pid
            && w.id == args.window
            && w.bundle == window.bundle
            && near(w.b, window.b, 4.)
    }) || !unsafe { CFEqual(current.0, element.0) }
        || !valid_element(current.0)
        || teams_draft_value(current.0).as_deref() != Some(existing.as_str())
        || Instant::now() >= deadline
    {
        return failure("teams_search_source_changed", false);
    }
    // Exactly one AXValue write: never focuses, types keys, submits, or sends.
    let attempted = !already_present;
    if attempted
        && unsafe { AXUIElementSetAttributeValue(current.0, string("AXValue").0, string(query).0) }
            != 0
    {
        return failure("teams_search_unconfirmed", true);
    }
    let after = window_inventory(false);
    let same_window = after.iter().any(|w| {
        w.pid == args.pid
            && w.id == args.window
            && w.bundle == window.bundle
            && near(w.b, window.b, 4.)
    });
    let same_target =
        resolve(args.pid, &args.path).is_some_and(|fresh| unsafe { CFEqual(fresh.0, current.0) });
    if !same_window
        || !same_target
        || teams_draft_value(current.0).as_deref() != Some(query)
        || !valid_element(current.0)
        || Instant::now() >= deadline
    {
        return failure("teams_search_unconfirmed", attempted);
    }
    json!({"ok":true,"verified":true,"source":"native_ax","pid":args.pid,
        "window_id":args.window,"query":query,"target_path":args.path,
        "exact_value_verified":true,"already_present":already_present,
        "mutation_attempted":attempted,"sent":false})
}

fn run(a: &[String]) -> Result<(), (i32, String)> {
    let cmd = a.first().map(String::as_str).unwrap_or("");
    let valid = match cmd {
        "ax-status" | "displays" => a.len() == 1,
        "windows" => a.len() <= 2,
        "bounds" | "activate" | "display" | "app-info" | "app-version" | "document-location"
        | "file-context" | "fingerprint" | "paste" => a.len() == 2,
        "document-context" => a.len() == 2 || (a.len() == 4 && a[2] == "--url"),
        "context" => {
            (2..=4).contains(&a.len())
                && a[2..]
                    .iter()
                    .all(|flag| ["--interactive", "--draft-values"].contains(&flag.as_str()))
                && a[2..]
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    == a.len() - 2
        }
        "ax-tree" => a.len() == 2 || (a.len() == 3 && a[2] == "--interactive"),
        "focused-text-target" => a.len() == 2 || (a.len() == 3 && a[2] == "--paste-stdin"),
        "move-window" => a.len() == 3,
        "select-file" => matches!(a.len(), 5 | 6),
        "ocr" => a.len() == 4,
        "scroll" => a.len() == 5,
        "ax-perform" => a.len() == 9,
        "teams-draft" | "teams-search" => a.len() == 10,
        "wait-ax-change" => a.len() == 5,
        _ => false,
    };
    if !valid {
        return error(
            64,
            "usage: cu-native teams-search PID WINDOW_ID AX_PATH ROLE NAME X Y W H < QUERY | wait-ax-change APP PID WINDOW_ID TIMEOUT_MS | teams-draft PID WINDOW_ID CONVERSATION AX_PATH NAME X Y W H < TEXT | select-file APP PID WINDOW_ID PATH [EXPECTED_SOURCE_JSON] | windows [APP] | bounds APP | context APP [--interactive] [--draft-values] | ax-tree APP | ax-perform PID PATH ROLE NAME X Y W H | file-context APP | document-context APP | document-location APP | app-info APP | activate APP | displays | display APP | move-window APP ID | focused-text-target APP [--paste-stdin] | ocr IMAGE X Y | fingerprint IMAGE | paste FILE | scroll X Y DELTA COUNT | ax-status",
        );
    }
    if cmd == "wait-ax-change" {
        println!("{}", native_events::wait(&a[1..]));
        return Ok(());
    }
    if cmd == "teams-search" {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(401)
            .read_to_end(&mut bytes)
            .map_err(|_| (3, "teams_search_invalid_arguments".into()))?;
        let result = if bytes.len() <= 400 {
            String::from_utf8(bytes)
                .ok()
                .map(|query| teams_search(&a[1..], &query))
        } else {
            None
        }
        .unwrap_or_else(|| {
            json!({"ok":false,
            "error_code":"teams_search_invalid_arguments","mutation_attempted":false,"sent":false})
        });
        println!("{result}");
        return if result["ok"] == true {
            Ok(())
        } else {
            Err((3, "Native Teams search not confirmed".into()))
        };
    }
    if cmd == "teams-draft" {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(32001)
            .read_to_end(&mut bytes)
            .map_err(|_| (3, "teams_draft_invalid_arguments".into()))?;
        let result = if bytes.len() > 32000 {
            json!({"ok":false,"error_code":"teams_draft_invalid_arguments","mutation_attempted":false,"sent":false})
        } else {
            match String::from_utf8(bytes) {
                Ok(body) => teams_draft(&a[1..], &body),
                Err(_) => {
                    json!({"ok":false,"error_code":"teams_draft_invalid_arguments","mutation_attempted":false,"sent":false})
                }
            }
        };
        println!("{result}");
        return if result["ok"] == true {
            Ok(())
        } else {
            Err((3, "Native Teams draft not confirmed".into()))
        };
    }
    if cmd == "select-file" {
        let result = native_file_picker::run(&a[1..]);
        println!("{result}");
        return if result["ok"] == true {
            Ok(())
        } else {
            Err((3, "File selection incomplete".into()))
        };
    }
    if cmd == "ax-status" {
        let trusted = unsafe { AXIsProcessTrusted() };
        println!("{}", if trusted { "trusted" } else { "untrusted" });
        return if trusted {
            Ok(())
        } else {
            error(1, "Accessibility permission unavailable")
        };
    }
    if cmd == "displays" {
        let ds:Vec<_>=displays().iter().map(|&id|{let b=unsafe{CGDisplayBounds(id)};json!({"id":id,"name":display_name(id),"built_in":unsafe{CGDisplayIsBuiltin(id)}!=0,"main":unsafe{CGDisplayIsMain(id)}!=0,"mirrors_display":unsafe{CGDisplayMirrorsDisplay(id)},"bounds":[b.p.x,b.p.y,b.s.w,b.s.h]})}).collect();
        println!(
            "{}",
            json!({"ok":true,"source":"native_displays","displays":ds})
        );
        return Ok(());
    }
    if cmd == "ax-perform" {
        if a.len() != 9 {
            return error(64, "invalid ax-perform arguments");
        }
        let pid = a[1].parse().map_err(|_| (64, "invalid pid".into()))?;
        let e = resolve(pid, &a[2]).ok_or((
            4,
            "stale_element: accessibility path no longer resolves".into(),
        ))?;
        let r =
            record(e.0, a[2].clone()).ok_or((4, "stale_element: element unavailable".into()))?;
        let nums: Vec<f64> = a[5..9]
            .iter()
            .map(|s| s.parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| (64, "invalid bounds".into()))?;
        if nums.iter().any(|n| !n.is_finite())
            || r.role != a[3]
            || r.name != a[4]
            || !near(
                r.b,
                Rect {
                    p: Point {
                        x: nums[0],
                        y: nums[1],
                    },
                    s: Size {
                        w: nums[2],
                        h: nums[3],
                    },
                },
                4.,
            )
        {
            return error(4, "stale_element: role, name, or bounds changed");
        }
        if boolean(e.0, "AXEnabled") == Some(false) {
            return error(6, "control_disabled: element is disabled");
        }
        println!(
            "{}",
            perform(e.0).ok_or((
                5,
                "unsupported_action: element exposes no supported action".into()
            ))?
        );
        return Ok(());
    }
    if cmd == "focused-text-target" {
        let q = a.get(1).ok_or((64, "expected APP".into()))?;
        let insert = if a.get(2).map(String::as_str) == Some("--paste-stdin") {
            use std::io::Read;
            let mut bytes = vec![];
            std::io::stdin()
                .take(20001)
                .read_to_end(&mut bytes)
                .map_err(|_| (3, "stdin read failed".into()))?;
            if bytes.len() > 20000 {
                return error(3, "invalid_notes_text");
            }
            Some(String::from_utf8(bytes).map_err(|_| (3, "invalid_notes_text".into()))?)
        } else {
            None
        };
        let result = focused_notes(q, insert.as_deref());
        println!("{result}");
        return if result["ok"] == true {
            Ok(())
        } else {
            error(3, "Native Notes text target unavailable")
        };
    }
    match cmd {
        "ocr" if a.len() == 4 => {
            let x = a[2].parse().map_err(|_| (64, "invalid x".into()))?;
            let y = a[3].parse().map_err(|_| (64, "invalid y".into()))?;
            print!("{}", native_media::ocr(&a[1], x, y).map_err(|e| (1, e))?);
            return Ok(());
        }
        "fingerprint" if a.len() == 2 => {
            println!("{}", native_media::fingerprint(&a[1]).map_err(|e| (1, e))?);
            return Ok(());
        }
        "paste" if a.len() == 2 => {
            native_media::paste(&a[1]).map_err(|e| (1, e))?;
            println!("pasted");
            return Ok(());
        }
        "scroll" if a.len() == 5 => {
            let x = a[1].parse().map_err(|_| (64, "invalid x".into()))?;
            let y = a[2].parse().map_err(|_| (64, "invalid y".into()))?;
            let delta = a[3].parse().map_err(|_| (64, "invalid delta".into()))?;
            let count = a[4].parse().map_err(|_| (64, "invalid count".into()))?;
            native_media::scroll(x, y, delta, count).map_err(|e| (1, e))?;
            println!("scrolled");
            return Ok(());
        }
        _ => {}
    }
    let ws = windows();
    if cmd == "windows" {
        for w in &ws {
            if a.len() == 1 || score(w, &a[1]) > 0 {
                println!("{}", row(w))
            }
        }
        return Ok(());
    }
    let q = a.get(1).ok_or((64, "expected APP".into()))?;
    let visible_ids: Vec<_> = ws.iter().map(|window| window.id).collect();
    let ws = if matches!(
        cmd,
        "context" | "ax-tree" | "document-location" | "document-context" | "file-context"
    ) && !ws.iter().any(|window| score(window, q) >= 70)
    {
        // Background native inspection does not require a rendered window on
        // the current Space. Pointer input, movement and pixel capture retain
        // their on-screen inventory; never activate an app just to read it.
        window_inventory(false)
    } else {
        ws
    };
    let w = choose(&ws, q).ok_or((1, "no CoreGraphics window matched requested app".into()))?;
    match cmd {
        "file-context" => match file_context(w, q) {
            Ok(value) => println!("{value}"),
            Err(code) => return error(3, code),
        },
        "move-window" => {
            let id = a
                .get(2)
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or((64, "expected display ID".into()))?;
            let alias = |s: &str| match canon(s).as_str() {
                "chrome" => "googlechrome".into(),
                "applenotes" => "notes".into(),
                "teams" => "microsoftteams".into(),
                _ => canon(s),
            };
            if alias(q) != alias(&w.name) && !q.eq_ignore_ascii_case(&w.bundle) {
                return error(1, "Window owner mismatch");
            }
            let result = move_window(w, id);
            println!("{result}");
            if result["ok"] != true {
                return error(1, "Window move unsuccessful");
            }
        }
        "bounds" => println!("{}", row(w)),
        "app-info" => println!("{}\t{}\t{}", w.pid, clean(&w.name), clean(&w.bundle)),
        "app-version" => {
            let version =
                application_version(w.pid).ok_or((1, "Application version unavailable".into()))?;
            println!(
                "{}",
                json!({"ok":true,"pid":w.pid,"bundle_id":w.bundle,"version":version})
            );
        }
        "activate" => {
            let ok = unsafe {
                let f: unsafe extern "C" fn(Ref, Ref, usize) -> bool =
                    std::mem::transmute(objc_msgSend as *const ());
                // A CLI is a background client. activateAllWindows alone may
                // accept the request without transferring keyboard focus.
                // Explicit foreground actions also need activateIgnoringOtherApps.
                const ACTIVATE_ALL_WINDOWS: usize = 1;
                const ACTIVATE_IGNORING_OTHER_APPS: usize = 2;
                f(
                    running(w.pid),
                    sel("activateWithOptions:"),
                    ACTIVATE_ALL_WINDOWS | ACTIVATE_IGNORING_OTHER_APPS,
                )
            };
            if !ok {
                return error(1, "could not activate app");
            }
            // Activation is asynchronous. A pre-existing window is not proof
            // that it owns keyboard input; don't exit before focus transfers.
            let deadline = Instant::now() + Duration::from_millis(800);
            loop {
                let focused = owned(unsafe { AXUIElementCreateSystemWide() })
                    .and_then(|system| attr(system.0, "AXFocusedApplication"))
                    .is_some_and(|application| {
                        let mut pid = 0;
                        (unsafe { AXUIElementGetPid(application.0, &mut pid) }) == 0 && pid == w.pid
                    });
                if focused {
                    break;
                }
                if Instant::now() >= deadline {
                    return error(1, "app activation did not transfer keyboard focus");
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            println!("{}", row(w))
        }
        "display" => {
            let ids = displays();
            let id = ids
                .iter()
                .find(|&&id| {
                    let b = unsafe { CGDisplayBounds(id) };
                    let x = w.b.p.x + w.b.s.w / 2.;
                    let y = w.b.p.y + w.b.s.h / 2.;
                    x >= b.p.x && x < b.p.x + b.s.w && y >= b.p.y && y < b.p.y + b.s.h
                })
                .or(ids.first())
                .ok_or((1, "no active display".into()))?;
            println!("{}\t{}", id, coords(unsafe { CGDisplayBounds(*id) }));
        }
        "context" | "ax-tree" => {
            if cmd == "context" {
                println!("META\t{}\t{}", row(w), clean(&w.bundle))
            }
            if !unsafe { AXIsProcessTrusted() } {
                return if cmd == "context" {
                    Ok(())
                } else {
                    error(2, "Accessibility permission unavailable")
                };
            }
            let requested_drafts = a.iter().any(|flag| flag == "--draft-values");
            let (rs, mut quality) = records(
                w,
                a.iter().any(|flag| flag == "--interactive"),
                requested_drafts,
            );
            quality["on_screen"] = json!(visible_ids.contains(&w.id));
            if cmd == "context" {
                println!("QUALITY\t{quality}")
            }
            if rs.is_empty() && cmd == "ax-tree" {
                return error(3, "Accessibility exposed no named elements");
            }
            for r in rs {
                println!("{}{}", if cmd == "context" { "AX\t" } else { "" }, rrow(&r));
            }
        }
        "document-location" => println!(
            "{}",
            location(w).ok_or((3, "The selected window exposes no document location".into()))?
        ),
        "document-context" => {
            if !browser(&w.bundle) {
                return error(3, "Unsupported browser document");
            }
            let selected = w;
            let selection_deadline = Instant::now() + Duration::from_millis(1300);
            let valid_document = |window: &Window| {
                location_until(window, selection_deadline).filter(|v| {
                    v["url"]
                        .as_str()
                        .map(|u| {
                            u.get(..8)
                                .map(|s| s.eq_ignore_ascii_case("https://"))
                                .unwrap_or(false)
                                || u.get(..7)
                                    .map(|s| s.eq_ignore_ascii_case("http://"))
                                    .unwrap_or(false)
                        })
                        .unwrap_or(false)
                })
            };
            let mut candidates = vec![];
            if let Some(expected) = a.get(3) {
                let expected = document_key(expected)
                    .ok_or((64, "expected a credential-free HTTP(S) document URL".into()))?;
                let eligible: Vec<_> = ws
                    .iter()
                    .filter(|candidate| candidate.pid == selected.pid && !strip(candidate, &ws))
                    .collect();
                if eligible.len() > 12 {
                    return error(
                        3,
                        "Too many browser windows to resolve the requested document safely",
                    );
                }
                for candidate in eligible {
                    if Instant::now() >= selection_deadline {
                        return error(3, "Browser document lookup timed out");
                    }
                    if let Some(loc) = valid_document(candidate)
                        && loc["url"].as_str().and_then(document_key).as_deref()
                            == Some(expected.as_str())
                    {
                        candidates.push((candidate, loc, candidate.id != selected.id));
                    }
                    if candidates.len() > 1 {
                        break;
                    }
                }
            } else if let Some(loc) = valid_document(selected) {
                candidates.push((selected, loc, false));
            } else {
                let index = ws
                    .iter()
                    .position(|x| x.id == selected.id && x.pid == selected.pid)
                    .unwrap_or(ws.len());
                let eligible: Vec<_> = ws
                    .iter()
                    .skip(index + 1)
                    .filter(|x| {
                        x.pid == selected.pid
                            && x.id != selected.id
                            && x.b.s.w * x.b.s.h > selected.b.s.w * selected.b.s.h * 1.2
                            && x.b.p.x <= selected.b.p.x + 2.
                            && x.b.p.y <= selected.b.p.y + 2.
                            && x.b.p.x + x.b.s.w >= selected.b.p.x + selected.b.s.w - 2.
                            && x.b.p.y + x.b.s.h >= selected.b.p.y + selected.b.s.h - 2.
                            && !strip(x, &ws)
                    })
                    .collect();
                if eligible.len() > 4 {
                    return error(3, "Browser document identity is ambiguous");
                }
                for candidate in eligible {
                    if Instant::now() >= selection_deadline {
                        return error(3, "Browser document lookup timed out");
                    }
                    if let Some(loc) = valid_document(candidate) {
                        candidates.push((candidate, loc, true));
                    }
                    if candidates.len() > 1 {
                        break;
                    }
                }
            }
            if candidates.len() > 1 {
                return error(3, "Browser document identity is ambiguous");
            }
            if candidates.len() != 1 || Instant::now() >= selection_deadline {
                return error(3, "No unique visible browser document could be identified");
            }
            let (w, loc, occluded) = candidates.pop().unwrap();
            let started = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            let mut budget = Budget::new();
            budget.deadline = selection_deadline;
            let ax = axwindows(w.pid);
            let matching: Vec<_> = ax
                .iter()
                .enumerate()
                .filter(|(_, e)| frame(e.0).map(|f| near(f, w.b, 12.)).unwrap_or(false))
                .collect();
            if matching.len() != 1 {
                return error(3, "Browser document window identity is ambiguous");
            }
            let mut areas = vec![];
            let mut requested_web_tree = false;
            loop {
                visible_areas(
                    matching[0].1.0,
                    format!("w{}", matching[0].0),
                    0,
                    w.b,
                    &mut budget,
                    &mut areas,
                );
                if !areas.is_empty()
                    || budget.truncated
                    || Instant::now() + Duration::from_millis(25) >= selection_deadline
                {
                    break;
                }
                if !requested_web_tree {
                    prepare_web_accessibility(w);
                    requested_web_tree = true;
                }
                std::thread::sleep(Duration::from_millis(25));
                // Re-read, retaining the original document/window identity and deadline.
                budget = Budget::new();
                budget.deadline = selection_deadline;
            }
            if areas.len() != 1 || budget.truncated {
                return error(3, "Browser document exposes no unique readable web area");
            }
            let mut rs = vec![];
            budget.document_text = true;
            walk(
                areas[0].0.0,
                areas[0].1.clone(),
                0,
                false,
                &mut budget,
                &mut rs,
            );
            let mut bytes = 0;
            let mut elements = vec![];
            let mut truncated = budget.truncated;
            for r in rs
                .iter()
                .filter(|r| SAFE.contains(&r.role.as_str()) && r.path.contains('.'))
            {
                let name: String = r.name.chars().take(2000).collect();
                bytes += name.len() + r.value.len();
                if bytes > 12000 || elements.len() >= 100 {
                    truncated = true;
                    break;
                }
                elements
                    .push(json!({"role":r.role,"name":name,"value":r.value,"enabled":r.enabled}));
            }
            if elements.is_empty() || Instant::now() >= budget.deadline {
                return error(3, "Native browser document text unavailable");
            }
            let after = windows();
            if choose(&after, q).map(|x| (x.id, x.pid)) != Some((selected.id, selected.pid))
                || !after
                    .iter()
                    .any(|x| x.id == w.id && x.pid == w.pid && near(x.b, w.b, 4.))
                || location_until(w, selection_deadline)
                    .and_then(|v| v["url"].as_str().map(str::to_owned))
                    != loc["url"].as_str().map(str::to_owned)
                || Instant::now() >= selection_deadline
            {
                return error(3, "The browser document changed while reading");
            }
            println!(
                "{}",
                json!({"ok":true,"source":"native_ax","snapshot":format!("document_read_{}_{}",std::process::id(),started),"created_at":started,"readonly":true,"actionable":false,"app":{"name":w.name,"pid":w.pid,"bundle_id":w.bundle},"window":{"id":w.id,"title":w.title,"bounds":[w.b.p.x,w.b.p.y,w.b.s.w,w.b.s.h]},"verified_document":loc,"occluded":occluded,"overlay_window_id":if occluded{Some(selected.id)}else{None},"visibility":{"scope":"current_accessibility_document","complete":false,"may_contain_scrolled_out":true,"occluded":occluded},"truncated":truncated,"elements":elements})
            );
        }
        _ => return error(64, "unsupported native command"),
    }
    Ok(())
}
fn requires_input_lease(args: &[String]) -> bool {
    match args.first().map(String::as_str) {
        Some(
            "teams-draft" | "teams-search" | "select-file" | "ax-perform" | "activate" | "paste"
            | "scroll" | "move-window",
        ) => true,
        Some("focused-text-target") => args.iter().any(|arg| arg == "--paste-stdin"),
        _ => false,
    }
}
fn run_with_input_lease(args: &[String]) -> Result<(), (i32, String)> {
    let lease = if requires_input_lease(args) {
        input_lease::acquire().map(Some)
    } else {
        input_lease::acquire_if_delegated()
    }
    .map_err(|error| (3, error.code().into()))?;
    match lease {
        Some(lease) => input_lease::with_current_lease(lease, || run(args))
            .map_err(|error| (3, error.code().into()))?,
        None => run(args),
    }
}
fn main() {
    let pool = msg0(class("NSAutoreleasePool"), "new");
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = run_with_input_lease(&args);
    let _ = msg0(pool, "drain");
    if let Err((code, message)) = result {
        if ["input_busy", "input_lease_unavailable"].contains(&message.as_str()) {
            println!(
                "{}",
                json!({"ok":false,"error_code":message,"mutation_attempted":false,"sent":false})
            );
            std::process::exit(3);
        }
        if args.first().map(String::as_str) == Some("file-context") && code != 64 {
            let code = if message.starts_with("file_context_") {
                message.as_str()
            } else {
                "file_context_window_unavailable"
            };
            println!(
                "{}",
                json!({"ok":false,"error_code":code,"read_only":true,
                "content_read":false,"actionable":false})
            );
            std::process::exit(3)
        }
        if message == "Window move unsuccessful" || message == "File selection incomplete" {
            std::process::exit(code)
        }
        if args.first().map(String::as_str) == Some("document-context") && code != 64 {
            println!("{}", json!({"ok":false,"error":message}));
            std::process::exit(3)
        }
        if args.first().map(String::as_str) == Some("move-window") && code != 64 {
            println!("{}", json!({"ok":false,"error":message}));
            std::process::exit(1)
        }
        if args.first().map(String::as_str) == Some("focused-text-target")
            && message == "invalid_notes_text"
        {
            println!(
                "{}",
                json!({"ok":false,"pasted":false,"error_code":"invalid_notes_text"})
            );
            std::process::exit(3)
        }
        if message == "no CoreGraphics window matched requested app"
            && args
                .first()
                .map(|s| {
                    ["document-location", "app-info", "context", "ax-tree"].contains(&s.as_str())
                })
                .unwrap_or(false)
        {
            eprintln!("{message}");
            std::process::exit(64)
        }
        eprintln!("{message}");
        std::process::exit(code)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_metadata_context_has_native_binding_without_content_or_handles() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("clip.mov");
        std::fs::write(&path, b"owned clip").unwrap();
        let document = local_file_document(&format!("file://{}", path.display())).unwrap();
        let window = Window {
            id: 17,
            pid: 42,
            name: "QuickTime Player".into(),
            title: "clip.mov".into(),
            b: Rect {
                p: Point { x: 1., y: 2. },
                s: Size { w: 800., h: 600. },
            },
            bundle: "com.apple.QuickTimePlayerX".into(),
        };
        let envelope = file_context_envelope(&window, document);
        assert_eq!(envelope["source"], "window_document");
        assert_eq!(envelope["app"]["pid"].as_u64(), Some(42));
        assert_eq!(envelope["window"]["id"].as_u64(), Some(17));
        assert_eq!(envelope["document"]["pid"], envelope["app"]["pid"]);
        assert_eq!(envelope["document"]["window_id"], envelope["window"]["id"]);
        assert_eq!(envelope["document"]["path"], path.to_str().unwrap());
        assert_eq!(envelope["document"]["bytes"], 10);
        assert_eq!(envelope["document"]["content_read"], false);
        assert_eq!(envelope["read_only"], true);
        assert_eq!(envelope["content_read"], false);
        assert_eq!(envelope["actionable"], false);
        assert!(envelope.get("snapshot").is_none());
        assert!(envelope.get("elements").is_none());
        assert!(!envelope.to_string().contains("owned clip"));
        for mutation in ["pid", "id", "bundle", "bounds"] {
            let mut changed = window.clone();
            match mutation {
                "pid" => changed.pid += 1,
                "id" => changed.id += 1,
                "bundle" => changed.bundle = "other.app".into(),
                _ => changed.b.p.x += 20.,
            }
            assert!(!same_file_window(&window, &changed));
        }
        assert!(same_file_window(&window, &window));
    }
    #[test]
    fn document_reads_keep_long_static_paragraphs_with_bounded_unicode_text() {
        let paragraph = format!(
            "{} Activation is 62 percent; pilot 20 users by Friday.",
            "Context ".repeat(40)
        );
        assert!(super::static_text_value(&paragraph, 200).is_empty());
        assert_eq!(super::static_text_value(&paragraph, 6000), paragraph);
        let long = "界".repeat(7000);
        assert_eq!(super::static_text_value(&long, 6000).chars().count(), 6000);
    }
    #[test]
    fn draft_value_privacy_is_opt_in_and_narrowly_allowlisted() {
        assert!(!Budget::new().draft_values);
        assert!(!SAFE.contains(&"AXTextArea") && !SAFE.contains(&"AXTextField"));
        for (role, name) in [
            ("AXTextArea", ""),
            ("AXTextArea", "Message"),
            ("AXTextField", "Caption"),
            ("AXTextField", "Subject"),
            ("AXTextField", "To:"),
        ] {
            assert!(draft_field_allowed(role, name, ""));
        }
        for (role, name, subrole) in [
            ("AXTextArea", "Password", ""),
            ("AXTextArea", "Search messages", ""),
            ("AXTextField", "URL", ""),
            ("AXTextField", "To email address", ""),
            ("AXTextField", "Username", ""),
            ("AXTextField", "Message", "AXSecureTextField"),
            ("AXComboBox", "To:", ""),
            ("AXSecureTextField", "Message", ""),
        ] {
            assert!(!draft_field_allowed(role, name, subrole));
        }
    }
    #[test]
    fn draft_values_are_bounded_without_clipping_exact_content() {
        assert!(draft_value_fits("Line one\r\nLine two\t😀", 0, 0));
        assert!(draft_value_fits(&"😀".repeat(DRAFT_FIELD_CHARS), 0, 0));
        assert!(!draft_value_fits(&"x".repeat(DRAFT_FIELD_CHARS + 1), 0, 0));
        assert!(!draft_value_fits("value", DRAFT_FIELDS, 0));
        assert!(!draft_value_fits("value", 0, DRAFT_TOTAL_CHARS - 4));
        assert!(!draft_value_fits("hidden\u{0000}", 0, 0));
        assert!(!draft_value_fits("escape\u{001b}", 0, 0));
    }
    #[test]
    fn represented_local_file_is_decoded_and_never_read_or_guessed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("video clip.mov");
        std::fs::write(&path, b"owned fixture").unwrap();
        let url = format!("file://{}", path.to_str().unwrap().replace(' ', "%20"));
        let document = local_file_document(&url).unwrap();
        assert_eq!(document["path"], path.to_str().unwrap());
        assert_eq!(document["bytes"], 13);
        assert_eq!(document["content_read"], false);
        for input in [
            "file://remote.example/tmp/video.mov",
            "https://example.com/video.mov",
            "file:///does-not-exist.mov",
            "file://user:secret@localhost/tmp/video.mov",
        ] {
            assert!(local_file_document(input).is_none());
        }
        assert!(local_file_document(&format!("file://{}", dir.path().display())).is_none());
        assert_eq!(std::fs::read(&path).unwrap(), b"owned fixture");
    }
    #[test]
    fn url_rejects_credentials_and_non_documents() {
        assert!(valid_url("https://example.com/page"));
        assert!(!valid_url("https://user:pass@example.com/"));
        assert!(!valid_url("javascript:alert(1)"));
    }
    #[test]
    fn names_clean_and_unicode_canonical() {
        assert_eq!(clean("a\nb\tc"), "a b c");
        assert_eq!(canon("Microsoft Teams"), "microsoftteams");
    }
    #[test]
    fn geometry_guard() {
        let a = Rect {
            p: Point { x: 10., y: 20. },
            s: Size { w: 100., h: 200. },
        };
        assert!(near(a, a, 4.));
        assert!(!near(a, Rect::default(), 4.));
    }
    #[test]
    fn document_url_native_parser_validates_authority() {
        for good in [
            "HTTPS://example.com/",
            "https://example.com/path?email=a@b.com",
            "file:///tmp/test%20file",
            "https://[::1]/",
        ] {
            assert!(valid_url(good), "{good}");
        }
        for bad in [
            "https://",
            "https://user@example.com/",
            "https://user%40name@example.com/",
            "https://:secret@example.com/",
            "data:text/plain,hello",
            "https://example.com/\n",
        ] {
            assert!(!valid_url(bad), "{bad}");
        }
    }
    #[test]
    fn document_selector_matches_path_without_accepting_credentials_or_other_origins() {
        assert_eq!(
            document_key("https://example.com/a?token=private#section"),
            document_key("https://example.com/a")
        );
        assert_ne!(
            document_key("https://example.com/a"),
            document_key("https://other.example/a")
        );
        assert_ne!(
            document_key("https://example.com/a"),
            document_key("https://example.com/b")
        );
        for bad in [
            "file:///tmp/source",
            "https://user:secret@example.com/a",
            "https://example.com/a\n",
            "javascript:alert(1)",
        ] {
            assert!(document_key(bad).is_none());
        }
    }
    #[test]
    fn ax_path_rejects_malformed_or_unbounded_paths_before_access() {
        assert_eq!(parse_ax_path("w2.0.31"), Some(vec![2, 0, 31]));
        for bad in ["", "w", "w-1", "w0.", "w0.-1", "w0.foo", "w10000", "w0/1"] {
            assert!(parse_ax_path(bad).is_none(), "{bad}");
        }
        assert!(parse_ax_path(&format!("w0{}", ".0".repeat(65))).is_none());
    }
    #[test]
    fn document_window_selection_rejects_ambiguous_or_distant_windows() {
        let target = Rect {
            p: Point { x: 100., y: 100. },
            s: Size { w: 300., h: 200. },
        };
        assert_eq!(unique_closest_window(&[(3, target)], target), Some(3));
        assert_eq!(
            unique_closest_window(&[(3, target), (5, target)], target),
            None
        );
        let shifted = Rect {
            p: Point { x: 105., y: 105. },
            s: Size { w: 305., h: 205. },
        };
        assert!(near(shifted, target, 12.));
        assert_eq!(unique_closest_window(&[(3, shifted)], target), None);
        assert_eq!(
            unique_closest_window(&[(2, shifted), (4, target)], target),
            Some(4)
        );
    }
    #[test]
    fn notes_selected_range_preserves_surrounding_utf16_text() {
        let original = "A😀B and context";
        assert_eq!(
            notes_replacement(
                original,
                &Range {
                    location: 1,
                    length: 2
                },
                "replacement"
            ),
            Some("AreplacementB and context".into())
        );
        assert!(
            notes_replacement(
                original,
                &Range {
                    location: 2,
                    length: 0
                },
                "x"
            )
            .is_none()
        );
        assert!(
            notes_replacement(
                original,
                &Range {
                    location: -1,
                    length: 0
                },
                "x"
            )
            .is_none()
        );
        assert!(
            notes_replacement(
                original,
                &Range {
                    location: 1,
                    length: isize::MAX
                },
                "x"
            )
            .is_none()
        );
    }
    #[test]
    fn input_lease_covers_direct_mutations_and_delegated_reads_without_locking_plain_inspection() {
        for command in [
            "teams-draft",
            "teams-search",
            "select-file",
            "ax-perform",
            "activate",
            "paste",
            "scroll",
            "move-window",
        ] {
            assert!(requires_input_lease(&[command.into()]));
        }
        assert!(requires_input_lease(&[
            "focused-text-target".into(),
            "Notes".into(),
            "--paste-stdin".into()
        ]));
        for command in [
            "context",
            "ax-tree",
            "document-context",
            "document-location",
            "file-context",
            "windows",
            "app-info",
            "ax-status",
            "displays",
            "wait-ax-change",
            "focused-text-target",
        ] {
            assert!(!requires_input_lease(&[command.into()]));
        }
    }
    #[test]
    fn teams_draft_policy_never_replaces_an_existing_draft_or_accepts_injection() {
        let args = [
            "42",
            "7",
            "Vivek",
            "w0.1.2.3",
            "Type a message",
            "10",
            "20",
            "300",
            "40",
        ]
        .map(str::to_string)
        .to_vec();
        assert!(teams_draft_args(&args, "Literal message\nsecond line").is_some());
        assert_eq!(teams_draft_existing("", "Hello"), Some(false));
        assert_eq!(teams_draft_existing("Hello", "Hello"), Some(true));
        assert_eq!(teams_draft_existing("My unsent draft", "Hello"), None);
        assert_eq!(teams_draft_existing(" ", "Hello"), None);
        for (index, value) in [
            (0, "0"),
            (1, "0"),
            (2, "Vivek\nSend"),
            (3, "w0;send"),
            (4, "Search messages"),
            (5, "NaN"),
            (7, "0"),
        ] {
            let mut bad = args.clone();
            bad[index] = value.into();
            assert!(teams_draft_args(&bad, "Hello").is_none(), "{index}");
        }
        for body in [
            String::new(),
            "x".repeat(8001),
            "hello\0there".into(),
            "hello\rthere".into(),
        ] {
            assert!(teams_draft_args(&args, &body).is_none());
        }
    }
    #[test]
    fn teams_search_accepts_only_literal_global_search_and_bounded_query() {
        let args = [
            "42",
            "7",
            "w0.1.2.3",
            "AXComboBox",
            "Search (⌘ E)",
            "10",
            "20",
            "300",
            "40",
        ]
        .map(str::to_string)
        .to_vec();
        assert!(teams_search_args(&args, "Anushka Singh").is_some());
        for name in [
            "Search",
            "Search Microsoft Teams",
            "Search (⌘E)",
            "Search (Command+E)",
        ] {
            let mut named = args.clone();
            named[4] = name.into();
            assert!(teams_search_args(&named, "Anushka").is_some());
        }
        for (index, bad) in [
            (0, "0"),
            (1, "0"),
            (2, "w0;send"),
            (3, "AXTextArea"),
            (4, "Search messages"),
            (4, "Type a message"),
            (4, "Search password"),
            (5, "NaN"),
            (7, "0"),
        ] {
            let mut invalid = args.clone();
            invalid[index] = bad.into();
            assert!(teams_search_args(&invalid, "Anushka").is_none());
        }
        for query in [
            String::new(),
            " ".into(),
            " name".into(),
            "name\nSend".into(),
            "name\0".into(),
            "x".repeat(101),
        ] {
            assert!(teams_search_args(&args, &query).is_none());
        }
        assert_eq!(
            teams_draft_existing("unrelated user search", "Anushka"),
            None
        );
    }
    #[test]
    fn teams_search_rejects_ambiguous_stale_or_composer_targets() {
        let args = [
            "42",
            "7",
            "w0.1.2.3",
            "AXTextField",
            "Search",
            "10",
            "20",
            "300",
            "40",
        ]
        .map(str::to_string)
        .to_vec();
        let scope = teams_search_args(&args, "Anushka").unwrap();
        let row = |path: &str, role: &str, name: &str| Record {
            path: path.into(),
            role: role.into(),
            name: name.into(),
            value: String::new(),
            b: scope.bounds,
            actions: vec![],
            enabled: Some(true),
        };
        assert!(
            teams_search_target(&[row(&scope.path, "AXTextField", "Search")], &scope).is_some()
        );
        assert!(
            teams_search_target(
                &[
                    row(&scope.path, "AXTextField", "Search"),
                    row("w0.1.2.4", "AXComboBox", "Search")
                ],
                &scope
            )
            .is_none()
        );
        assert!(teams_search_target(&[row("w0.1.2.4", "AXTextField", "Search")], &scope).is_none());
        assert!(teams_search_target(&[row(&scope.path, "AXTextArea", "Search")], &scope).is_none());
        let mut moved = row(&scope.path, "AXTextField", "Search");
        moved.b.p.x += 20.;
        assert!(teams_search_target(&[moved], &scope).is_none());
        let mut disabled = row(&scope.path, "AXTextField", "Search");
        disabled.enabled = Some(false);
        assert!(teams_search_target(&[disabled], &scope).is_none());
    }
    #[test]
    fn unknown_flags_reject_before_inspecting_apps() {
        for args in [
            vec!["context", "app", "--unknown"],
            vec!["focused-text-target", "Notes", "--write-all"],
            vec!["ax-perform"],
        ] {
            let args = args.into_iter().map(String::from).collect::<Vec<_>>();
            assert_eq!(run(&args).unwrap_err().0, 64);
        }
    }
    #[test]
    fn auxiliary_browser_strip_does_not_displace_content_window() {
        let make = |id, title: &str, height| Window {
            id,
            pid: 12,
            name: "Google Chrome".into(),
            title: title.into(),
            b: Rect {
                p: Point { x: 0., y: 0. },
                s: Size {
                    w: 1000.,
                    h: height,
                },
            },
            bundle: "com.google.Chrome".into(),
        };
        let windows = vec![make(1, "", 80.), make(2, "Document", 800.)];
        assert_eq!(choose(&windows, "Chrome").unwrap().id, 2);
        let standalone = vec![make(1, "", 80.)];
        assert_eq!(choose(&standalone, "Chrome").unwrap().id, 1);
    }
    #[test]
    fn native_window_selector_preserves_dialogs_displays_and_exact_owner_priority() {
        let window = |id, title: &str, x, y, width, height, pid, app: &str| Window {
            id,
            pid,
            name: app.into(),
            title: title.into(),
            bundle: String::new(),
            b: Rect {
                p: Point { x, y },
                s: Size {
                    w: width,
                    h: height,
                },
            },
        };
        let strip = window(1, "", 0., 0., 1352., 41., 42, "Chrome");
        let content = window(2, "Google", 0., 0., 1352., 761., 42, "Chrome");
        let cases: Vec<(&str, Vec<Window>, &str, Option<u32>)> = vec![
            (
                "fullscreen toolbar",
                vec![strip.clone(), content.clone()],
                "Chrome",
                Some(2),
            ),
            ("standalone toolbar", vec![strip.clone()], "Chrome", Some(1)),
            (
                "named thin dialog",
                vec![
                    window(3, "Permission", 0., 0., 1352., 41., 42, "Chrome"),
                    content.clone(),
                ],
                "Chrome",
                Some(3),
            ),
            (
                "untitled dialog",
                vec![
                    window(4, "", 400., 200., 500., 220., 42, "Chrome"),
                    content.clone(),
                ],
                "Chrome",
                Some(4),
            ),
            (
                "normal untitled",
                vec![
                    window(5, "", 0., 0., 1352., 761., 42, "Chrome"),
                    content.clone(),
                ],
                "Chrome",
                Some(5),
            ),
            (
                "front content",
                vec![
                    window(6, "Second", 0., 0., 1352., 761., 42, "Chrome"),
                    content.clone(),
                ],
                "Chrome",
                Some(6),
            ),
            (
                "title query",
                vec![strip.clone(), content.clone()],
                "Google",
                Some(2),
            ),
            (
                "other process",
                vec![
                    strip.clone(),
                    window(7, "Google", 0., 0., 1352., 761., 43, "Chrome"),
                ],
                "Chrome",
                Some(1),
            ),
            (
                "other display",
                vec![
                    strip.clone(),
                    window(8, "Google", 1400., 0., 1352., 761., 42, "Chrome"),
                ],
                "Chrome",
                Some(1),
            ),
            (
                "owner before title",
                vec![
                    window(9, "Chrome", 0., 0., 1352., 761., 43, "Other"),
                    content.clone(),
                ],
                "Chrome",
                Some(2),
            ),
            ("missing query", vec![strip, content], "Missing", None),
        ];
        for (name, windows, query, expected) in cases {
            assert_eq!(choose(&windows, query).map(|w| w.id), expected, "{name}");
        }
    }
    #[test]
    fn native_traversal_budget_bounds_depth_nodes_and_deadline_without_losing_siblings() {
        let mut nodes = Budget::new();
        for _ in 0..3000 {
            assert!(nodes.consume(0));
        }
        assert!(!nodes.consume(0));
        assert!(nodes.truncated);
        let mut depth = Budget::new();
        assert!(depth.consume(64));
        assert!(!depth.consume(65));
        assert!(depth.truncated);
        assert!(depth.consume(2));
        let mut expired = Budget::new();
        expired.deadline = Instant::now();
        assert!(!expired.consume(0));
        assert!(expired.truncated);
        assert!(chromium("com.google.Chrome"));
        assert!(!chromium("com.google.Chrome.attacker"));
        assert!(embedded("com.microsoft.teams2"));
        assert!(!embedded("com.microsoft.teams2.attacker"));
    }
}
