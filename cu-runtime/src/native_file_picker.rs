//! A bounded, native-only file-panel transaction. No clipboard, pointer input,
//! screenshots, uploads, or message-send controls are used here.
use super::*;
use std::os::unix::fs::MetadataExt;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceCreate(state: i32) -> *mut c_void;
    fn CGEventCreateKeyboardEvent(source: *mut c_void, key: u16, down: bool) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut c_void);
    fn CGEventKeyboardSetUnicodeString(event: *mut c_void, length: usize, text: *const u16);
}
struct Diagnostic {
    phase: &'static str,
    focus: Option<Value>,
    shape: Option<Value>,
    started: Option<Instant>,
    phase_started: Option<Instant>,
    navigation_attempts: u8,
    navigation_route: Option<&'static str>,
}
thread_local! { static DIAGNOSTIC:std::cell::RefCell<Diagnostic>=const {std::cell::RefCell::new(Diagnostic{phase:"validate_arguments",focus:None,shape:None,started:None,phase_started:None,navigation_attempts:0,navigation_route:None})}; }
fn phase(name: &'static str) {
    DIAGNOSTIC.with(|value| {
        let now = Instant::now();
        let started = if name == "validate_arguments" {
            now
        } else {
            value.borrow().started.unwrap_or(now)
        };
        let (navigation_attempts, navigation_route) = if name == "validate_arguments" {
            (0, None)
        } else {
            let previous = value.borrow();
            (previous.navigation_attempts, previous.navigation_route)
        };
        let path_shape = if name == "validate_arguments" {
            None
        } else {
            value
                .borrow()
                .shape
                .as_ref()
                .filter(|shape| shape.get("path_write_ax_result").is_some())
                .cloned()
        };
        *value.borrow_mut() = Diagnostic {
            phase: name,
            focus: None,
            shape: path_shape,
            started: Some(started),
            phase_started: Some(now),
            navigation_attempts,
            navigation_route,
        }
    });
}
fn diagnostic() -> Value {
    DIAGNOSTIC.with(|value| {
        let value = value.borrow();
        json!({"phase":value.phase,"focus_guard":value.focus,"native_shape":value.shape,"elapsed_ms":value.started.map(|start|start.elapsed().as_millis() as u64),"phase_elapsed_ms":value.phase_started.map(|start|start.elapsed().as_millis() as u64),"navigation_attempts":value.navigation_attempts,"navigation_route":value.navigation_route})
    })
}
fn focus_guard(panel: &Panel, target: Ref, panel_scope: bool, require_active: bool) -> bool {
    let owner_bound = resolve(panel.pid, &panel.owner_path)
        .is_some_and(|e| unsafe { CFEqual(e.0, panel.owner.0) });
    let modal_bound =
        resolve(panel.pid, &panel.path).is_some_and(|e| unsafe { CFEqual(e.0, panel.element.0) });
    let owner_focused = focus_within(panel.owner.0);
    let modal_focused = focus_within(panel.element.0);
    let target_focused = focus_within(target);
    let mut target_pid = 0;
    let pid_available =
        unsafe { AXUIElementGetPid(target, &mut target_pid) } == 0 && target_pid > 0;
    let active = pid_of(frontmost());
    let client_active = active == panel.pid;
    let target_active = pid_available && active == target_pid;
    let accepted = owner_bound
        && modal_bound
        && target_focused
        && (if panel_scope {
            modal_focused
        } else {
            owner_focused
        })
        && (!require_active || client_active || target_active);
    DIAGNOSTIC.with(|value|value.borrow_mut().focus=Some(json!({"owner_bound":owner_bound,"modal_bound":modal_bound,"owner_focused":owner_focused,"modal_focused":modal_focused,"target_focused":target_focused,"target_pid_available":pid_available,"client_active":client_active,"target_active":target_active,"panel_scope":panel_scope,"active_required":require_active})));
    accepted
}
fn focus_set_result(accepted: bool) {
    DIAGNOSTIC.with(|value| {
        if let Some(focus) = value.borrow_mut().focus.as_mut() {
            focus["focus_set_accepted"] = json!(accepted);
        }
    });
}
#[derive(Debug, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
    bytes: u64,
    modified: (i64, i64),
    changed: (i64, i64),
    canonical: std::path::PathBuf,
}
fn file_identity(path: &str) -> Result<FileIdentity, &'static str> {
    if path.is_empty()
        || path.len() > 4096
        || path.chars().any(char::is_control)
        || !std::path::Path::new(path).is_absolute()
    {
        return Err("picker_invalid_path");
    }
    let metadata = std::fs::metadata(path).map_err(|_| "picker_file_unavailable")?;
    if !metadata.is_file() {
        return Err("picker_regular_file_required");
    }
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        bytes: metadata.len(),
        modified: (metadata.mtime(), metadata.mtime_nsec()),
        changed: (metadata.ctime(), metadata.ctime_nsec()),
        canonical: std::fs::canonicalize(path).map_err(|_| "picker_file_unavailable")?,
    })
}
fn source_identity(file: &FileIdentity) -> Value {
    json!({"device":file.device,"inode":file.inode,"modified_seconds":file.modified.0,"modified_nanoseconds":file.modified.1,"changed_seconds":file.changed.0,"changed_nanoseconds":file.changed.1})
}
fn expected_source_matches(raw: &str, file: &FileIdentity) -> Result<(), &'static str> {
    if raw.len() > 4096 {
        return Err("picker_invalid_expected_source");
    }
    let value: Value = serde_json::from_str(raw).map_err(|_| "picker_invalid_expected_source")?;
    let object = value.as_object().ok_or("picker_invalid_expected_source")?;
    let identity = value["identity"]
        .as_object()
        .ok_or("picker_invalid_expected_source")?;
    let keys = [
        "device",
        "inode",
        "modified_seconds",
        "modified_nanoseconds",
        "changed_seconds",
        "changed_nanoseconds",
    ];
    if object.len() != 2
        || !object.contains_key("bytes")
        || !object.contains_key("identity")
        || identity.len() != 6
        || !keys.iter().all(|key| identity.contains_key(*key))
        || value["bytes"].as_u64().is_none()
        || ["device", "inode"]
            .iter()
            .any(|key| value["identity"][*key].as_u64().is_none())
        || ["modified_seconds", "changed_seconds"]
            .iter()
            .any(|key| value["identity"][*key].as_i64().is_none())
        || ["modified_nanoseconds", "changed_nanoseconds"]
            .iter()
            .any(|key| {
                value["identity"][*key]
                    .as_u64()
                    .is_none_or(|n| n >= 1_000_000_000)
            })
    {
        return Err("picker_invalid_expected_source");
    }
    // This schema has numeric leaves only, so every JSON string token must be
    // a key. Count decoded strings before Value's duplicate-key replacement;
    // escaped spellings of a repeated key are rejected too.
    let bytes = raw.as_bytes();
    let mut index = 0;
    let mut names = std::collections::BTreeSet::new();
    while index < bytes.len() {
        if bytes[index] != b'"' {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < bytes.len() {
            if bytes[index] == b'\\' {
                index += 2;
                continue;
            }
            if bytes[index] == b'"' {
                break;
            }
            index += 1;
        }
        if index >= bytes.len() {
            return Err("picker_invalid_expected_source");
        }
        index += 1;
        let name: String = serde_json::from_str(&raw[start..index])
            .map_err(|_| "picker_invalid_expected_source")?;
        if !names.insert(name) {
            return Err("picker_invalid_expected_source");
        }
    }
    if names.len() != 8 {
        return Err("picker_invalid_expected_source");
    }
    if value != json!({"bytes":file.bytes,"identity":source_identity(file)}) {
        return Err("picker_source_changed");
    }
    Ok(())
}
fn app_matches(requested: &str, name: &str, bundle: &str) -> bool {
    let alias = |s: &str| match canon(s).as_str() {
        "teams" => "microsoftteams".to_owned(),
        other => other.to_owned(),
    };
    !requested.is_empty()
        && (alias(requested) == alias(name)
            || !bundle.is_empty() && requested.eq_ignore_ascii_case(bundle))
}
fn label(element: Ref, role: &str) -> String {
    ["AXTitle", "AXDescription", "AXHelp"]
        .iter()
        .map(|key| astr(element, key))
        .find(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let value = if SAFE.contains(&role) {
                astr(element, "AXValue")
            } else {
                String::new()
            };
            if value.chars().count() <= 200 {
                value
            } else {
                String::new()
            }
        })
}
fn tree_limit(kind: &str, role: &str, count: usize, depth: usize) {
    DIAGNOSTIC.with(|value| {
        let mut value = value.borrow_mut();
        let shape = value.shape.get_or_insert_with(|| json!({}));
        shape["tree_limit"] = json!({"kind":kind,"role":role,"count":count,"depth":depth});
    });
}
struct Node {
    element: Owned,
    path: String,
    role: String,
    name: String,
    selected: bool,
}
fn nodes(root: Ref, path: &str, deadline: Instant) -> Result<Vec<Node>, &'static str> {
    native_nodes(root, path, deadline, false)
}
// Selected-row relations may not have AXChildren indices. Their synthetic
// paths are read-only and must never be used for control resolution/actions.
fn native_nodes(
    root: Ref,
    path: &str,
    deadline: Instant,
    selection: bool,
) -> Result<Vec<Node>, &'static str> {
    let mut pending = std::collections::VecDeque::from([(
        Owned(unsafe { CFRetain(root) }),
        path.to_owned(),
        0usize,
        false,
    )]);
    let mut out = Vec::new();
    while let Some((element, path, depth, selected)) = pending.pop_front() {
        if Instant::now() >= deadline || out.len() >= 180 {
            tree_limit(
                if Instant::now() >= deadline {
                    "deadline"
                } else {
                    "nodes"
                },
                "",
                out.len(),
                depth,
            );
            return Err("picker_native_tree_incomplete");
        }
        unsafe {
            AXUIElementSetMessagingTimeout(element.0, 0.025);
        }
        let role = astr(element.0, "AXRole");
        let name = label(element.0, &role);
        let selected = selected
            || matches!(role.as_str(), "AXRow" | "AXCell")
                && boolean(element.0, "AXSelected") == Some(true);
        if depth < 8 {
            // Read selected native rows/cells and their descendants. Browser
            // columns are structural, not proof that every file in them is
            // selected; unselected rows and web bodies are never enumerated.
            let listing = matches!(
                role.as_str(),
                "AXBrowser" | "AXTable" | "AXOutline" | "AXColumn" | "AXList"
            );
            let mut selected_relation = false;
            let mut descendants = if role == "AXWebArea" || listing && !selection {
                vec![]
            } else if listing {
                let rows = children(element.0, "AXSelectedRows");
                if !rows.is_empty() {
                    selected_relation = true;
                    rows
                } else {
                    let chosen = children(element.0, "AXSelectedChildren");
                    if !chosen.is_empty() {
                        selected_relation = true;
                        chosen
                    } else {
                        let columns = children(element.0, "AXColumns");
                        let structural = if columns.is_empty() {
                            children(element.0, "AXChildren")
                        } else {
                            columns
                        };
                        structural
                            .into_iter()
                            .filter(|child| {
                                matches!(
                                    astr(child.0, "AXRole").as_str(),
                                    "AXColumn"
                                        | "AXList"
                                        | "AXScrollArea"
                                        | "AXTable"
                                        | "AXOutline"
                                        | "AXBrowser"
                                        | "AXSplitGroup"
                                )
                            })
                            .collect()
                    }
                }
            } else {
                children(element.0, "AXChildren")
            };
            if descendants.len() > 60 {
                tree_limit("children", &role, descendants.len(), depth);
                return Err("picker_native_tree_incomplete");
            }
            for (index, child) in descendants.drain(..).enumerate() {
                let structural = matches!(
                    astr(child.0, "AXRole").as_str(),
                    "AXColumn"
                        | "AXList"
                        | "AXScrollArea"
                        | "AXTable"
                        | "AXOutline"
                        | "AXBrowser"
                        | "AXSplitGroup"
                );
                pending.push_back((
                    child,
                    format!("{path}.{index}"),
                    depth + 1,
                    selected || selected_relation && !structural,
                ));
            }
        }
        out.push(Node {
            element,
            path,
            role,
            name,
            selected,
        });
    }
    Ok(out)
}
fn button(nodes: &[Node], name: &str) -> Result<usize, &'static str> {
    let candidates: Vec<_> = nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| {
            node.role == "AXButton"
                && node.name.eq_ignore_ascii_case(name)
                && boolean(node.element.0, "AXEnabled") != Some(false)
                && actions(node.element.0).iter().any(|a| a == "AXPress")
        })
        .map(|(i, _)| i)
        .collect();
    if candidates.len() == 1 {
        Ok(candidates[0])
    } else {
        Err("picker_native_button_unavailable")
    }
}
fn confirm_label_allowed(name: &str) -> bool {
    !name
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| {
            [
                "send", "save", "publish", "post", "delete", "purchase", "buy", "pay", "replace",
            ]
            .contains(&word)
        })
}
fn confirm_button(
    root: Ref,
    contents: &[Node],
    require_enabled: bool,
) -> Result<usize, &'static str> {
    // AppKit exposes AXDefaultButton on the native owning window on some
    // versions, rather than on its descendant NSOpenPanel AXSheet.
    let mut defaults = Vec::new();
    let mut ancestor = Some(Owned(unsafe { CFRetain(root) }));
    for _ in 0..8 {
        let Some(current) = ancestor.take() else {
            break;
        };
        if let Some(button) = attr(current.0, "AXDefaultButton") {
            defaults.push(button);
        }
        if matches!(
            astr(current.0, "AXRole").as_str(),
            "AXWindow" | "AXApplication"
        ) {
            break;
        }
        ancestor = attr(current.0, "AXParent");
    }
    DIAGNOSTIC.with(|value| {
        let mut value = value.borrow_mut();
        let shape = value.shape.get_or_insert_with(|| json!({}));
        shape["native_default_button_count"] = json!(defaults.len());
        shape["button_count"] = json!(
            contents
                .iter()
                .filter(|node| node.role == "AXButton")
                .count()
        );
        shape["default_subrole_count"] = json!(
            contents
                .iter()
                .filter(|node| node.role == "AXButton"
                    && astr(node.element.0, "AXSubrole") == "AXDefaultButton")
                .count()
        );
    });
    let candidates: Vec<_> = contents
        .iter()
        .enumerate()
        .filter(|(_, node)| {
            node.role == "AXButton"
                && confirm_label_allowed(&node.name)
                && (!require_enabled || boolean(node.element.0, "AXEnabled") != Some(false))
                && actions(node.element.0)
                    .iter()
                    .any(|action| action == "AXPress")
                && (defaults
                    .iter()
                    .any(|button| unsafe { CFEqual(button.0, node.element.0) })
                    || astr(node.element.0, "AXSubrole") == "AXDefaultButton"
                    // Standard AppKit NSOpenPanel identifier persists when an
                    // app customizes the prompt and default attributes vanish.
                    || astr(node.element.0, "AXIdentifier") == "OKButton"
                    || node.name.eq_ignore_ascii_case("Open"))
        })
        .map(|(index, _)| index)
        .collect();
    if candidates.len() == 1 {
        Ok(candidates[0])
    } else {
        Err("picker_native_confirm_button_unavailable")
    }
}
fn native_modal(role: &str, subrole: &str) -> bool {
    role == "AXSheet"
        || role == "AXDialog"
        || role == "AXWindow" && matches!(subrole, "AXDialog" | "AXSystemDialog")
}
struct Panel {
    element: Owned,
    path: String,
    pid: i32,
    window_id: u32,
    owner: Owned,
    owner_path: String,
    client_window_count: usize,
    known_modals: Vec<Owned>,
    go_requested: std::cell::Cell<bool>,
}

fn panel(requested: &str, pid: i32, id: u32, deadline: Instant) -> Result<Panel, &'static str> {
    // AppKit can publish a remote sheet before its focused native subtree is
    // mounted. Re-read only that exact owner/window; never refocus or replay
    // input to repair an incomplete initial binding.
    let bind_deadline = deadline.min(Instant::now() + Duration::from_millis(350));
    loop {
        match panel_once(requested, pid, id, deadline) {
            Err("picker_native_open_panel_required") if Instant::now() < bind_deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            result => return result,
        }
    }
}
fn panel_once(
    requested: &str,
    pid: i32,
    id: u32,
    deadline: Instant,
) -> Result<Panel, &'static str> {
    let inventory = windows();
    let window = inventory
        .iter()
        .find(|w| w.id == id && w.pid == pid && app_matches(requested, &w.name, &w.bundle))
        .ok_or("picker_target_mismatch")?;
    let windows = axwindows(pid);
    if windows.len() > 12 {
        return Err("picker_target_ambiguous");
    }
    let mut candidates = Vec::new();
    let mut modal_nodes = 0usize;
    let mut geometry_bound = 0usize;
    let mut focus_bound = 0usize;
    let mut file_panels = 0usize;
    for (i, root) in windows.iter().enumerate() {
        let owned_parent = frame(root.0).is_some_and(|b| near(b, window.b, 12.));
        let root_tree = nodes(root.0, &format!("w{i}"), deadline)?;
        let known_modals: Vec<_> = root_tree
            .iter()
            .filter(|node| native_modal(&node.role, &astr(node.element.0, "AXSubrole")))
            .map(|node| Owned(unsafe { CFRetain(node.element.0) }))
            .collect();
        for node in root_tree {
            if !native_modal(&node.role, &astr(node.element.0, "AXSubrole")) {
                continue;
            }
            modal_nodes += 1;
            if !(owned_parent || frame(node.element.0).is_some_and(|b| near(b, window.b, 12.))) {
                continue;
            }
            geometry_bound += 1;
            if !focus_within(node.element.0) {
                // Report only bounded focus facts, never the focused app or text.
                DIAGNOSTIC.with(|value| {
                    value.borrow_mut().focus = Some(json!({
                    "client_active":pid_of(frontmost()) == pid,
                    "owner_focused":focus_within(root.0),"modal_focused":false}))
                });
                continue;
            }
            focus_bound += 1;
            let contents = nodes(node.element.0, &node.path, deadline)?;
            if contents
                .iter()
                .any(|node| matches!(node.role.as_str(), "AXBrowser" | "AXOutline"))
                && !contents.iter().any(|field| {
                    matches!(field.role.as_str(), "AXTextField" | "AXComboBox")
                        && field.name.to_lowercase().contains("save")
                })
                && button(&contents, "Cancel").is_ok()
            {
                file_panels += 1;
                candidates.push(Panel {
                    element: node.element,
                    path: node.path,
                    pid,
                    window_id: id,
                    owner: Owned(unsafe { CFRetain(root.0) }),
                    owner_path: format!("w{i}"),
                    known_modals: known_modals
                        .iter()
                        .map(|element| Owned(unsafe { CFRetain(element.0) }))
                        .collect(),
                    go_requested: std::cell::Cell::new(false),
                    client_window_count: inventory
                        .iter()
                        .filter(|window| window.pid == pid)
                        .count(),
                });
            }
        }
    }
    if candidates.len() != 1 {
        DIAGNOSTIC.with(|value| value.borrow_mut().shape=Some(json!({"modal_nodes":modal_nodes,"geometry_bound":geometry_bound,"focus_bound":focus_bound,"file_panels":file_panels})));
        return Err("picker_native_open_panel_required");
    }
    Ok(candidates.remove(0))
}
fn guarded_key(panel: &Panel, focus: Ref, key: u16, flags: u64) -> Result<(), &'static str> {
    if !((key == 5 && flags == ((1 << 17) | (1 << 20))) || key == 36 && flags == 0) {
        return Err("picker_keyboard_action_unavailable");
    }
    phase(if key == 5 {
        "go_to_shortcut_focus"
    } else {
        "go_to_return_focus"
    });
    if !focus_guard(panel, focus, key == 5, false) {
        return Err("picker_focus_changed");
    }
    let mut pid = 0;
    if unsafe { AXUIElementGetPid(focus, &mut pid) } != 0 || pid <= 0 {
        return Err("picker_focus_changed");
    }
    let source = owned(unsafe { CGEventSourceCreate(1) }).ok_or("picker_keyboard_unavailable")?;
    let down = owned(unsafe { CGEventCreateKeyboardEvent(source.0.cast_mut(), key, true) })
        .ok_or("picker_keyboard_unavailable")?;
    let up = owned(unsafe { CGEventCreateKeyboardEvent(source.0.cast_mut(), key, false) })
        .ok_or("picker_keyboard_unavailable")?;
    unsafe {
        CGEventSetFlags(down.0.cast_mut(), flags);
        CGEventSetFlags(up.0.cast_mut(), flags);
    }
    if !focus_guard(panel, focus, key == 5, true) {
        return Err("picker_focus_changed");
    }
    // AppKit's XPC file panels ignore events posted to their service PID.
    // Route these two fixed shortcuts through the system only after proving
    // the same exact focused modal/field and its foreground owning app.
    unsafe {
        CGEventPost(0, down.0.cast_mut());
        CGEventPost(0, up.0.cast_mut());
    }
    Ok(())
}
fn go_dialog(panel: &Panel, deadline: Instant) -> Result<Option<(Owned, String)>, &'static str> {
    let mut matches = Vec::new();
    // AX paths can briefly be unavailable while AppKit mounts a nested GoTo
    // sheet. Wait for fresh positive original bindings; never input while they
    // are absent, and never replace the retained native handles.
    if !focus_guard(panel, panel.owner.0, false, false) {
        return Ok(None);
    }
    let tree = nodes(panel.owner.0, &panel.owner_path, deadline)?;
    let marker_seen = tree.iter().any(|node| {
        ["go to the folder", "go to folder"]
            .iter()
            .any(|label| node.name.to_lowercase().contains(label))
    });
    let modal_count = tree
        .iter()
        .filter(|node| native_modal(&node.role, &astr(node.element.0, "AXSubrole")))
        .count();
    let client_windows = windows()
        .iter()
        .filter(|window| window.pid == panel.pid)
        .count();
    DIAGNOSTIC.with(|value|value.borrow_mut().shape=Some(json!({"client_window_added":client_windows>panel.client_window_count,"ax_root_count":axwindows(panel.pid).len().min(100),"owner_modal_count":modal_count,"go_to_marker_seen":marker_seen,"go_to_candidate_count":0})));
    for node in tree {
        if unsafe { CFEqual(node.element.0, panel.element.0) }
            || !focus_within(node.element.0)
            || !native_modal(&node.role, &astr(node.element.0, "AXSubrole"))
        {
            continue;
        }
        let contents = nodes(node.element.0, &node.path, deadline)?;
        let explicit_label = contents.iter().any(|n| {
            ["go to the folder", "go to folder"]
                .iter()
                .any(|label| n.name.to_lowercase().contains(label))
        });
        // Some AppKit versions expose no textual GoTo label. The fresh native
        // modal created by our fixed GoTo action can instead be identified by
        // its retained native identity, original owner ancestry, and one
        // provably settable nonsecure path field. No pre-existing arbitrary
        // dialog can enter this route, and exact path readback remains required.
        let fresh_path_modal = panel.go_requested.get()
            && !panel
                .known_modals
                .iter()
                .any(|known| unsafe { CFEqual(known.0, node.element.0) })
            && path_field(&contents).is_ok();
        if (explicit_label || fresh_path_modal) && button(&contents, "Open").is_err() {
            matches.push((node.element, node.path));
        }
    }
    DIAGNOSTIC.with(|value| {
        if let Some(shape) = value.borrow_mut().shape.as_mut() {
            shape["go_to_candidate_count"] = json!(matches.len());
        }
    });
    if matches.len() > 1 {
        return Err("picker_go_to_dialog_ambiguous");
    }
    Ok(matches.pop())
}
fn path_field(contents: &[Node]) -> Result<usize, &'static str> {
    let candidates: Vec<_> = contents
        .iter()
        .enumerate()
        .filter(|(_, n)| {
            matches!(n.role.as_str(), "AXTextField" | "AXComboBox")
                && !astr(n.element.0, "AXSubrole")
                    .to_lowercase()
                    .contains("secure")
                && !["search", "password", "secret", "url"]
                    .iter()
                    .any(|word| n.name.to_lowercase().contains(word))
                && settable(n.element.0, "AXValue")
        })
        .map(|(i, _)| i)
        .collect();
    let candidates: Vec<_> = candidates
        .iter()
        .copied()
        .filter(|index| {
            !candidates.iter().any(|other| {
                other != index
                    && contents[*other]
                        .path
                        .strip_prefix(&contents[*index].path)
                        .is_some_and(|suffix| suffix.starts_with('.'))
            })
        })
        .collect();
    if candidates.len() != 1 {
        return Err("picker_path_field_unavailable");
    }
    Ok(candidates[0])
}
fn selection_proof(
    panel: &Panel,
    expected: &FileIdentity,
    deadline: Instant,
) -> Result<bool, &'static str> {
    let mut files = std::collections::BTreeSet::new();
    let mut expected_seen = false;
    for node in native_nodes(panel.element.0, &panel.path, deadline, true)? {
        if !node.selected && !unsafe { CFEqual(node.element.0, panel.element.0) } {
            continue;
        }
        for key in if node.selected {
            vec!["AXURL", "AXFilename"]
        } else {
            vec!["AXFilename"]
        } {
            let raw = astr(node.element.0, key);
            let path = if raw.starts_with("file:") {
                local_file_document(&raw).and_then(|v| v["path"].as_str().map(str::to_owned))
            } else if std::path::Path::new(&raw).is_absolute() {
                Some(raw)
            } else {
                None
            };
            if let Some(path) = path {
                let Ok(canonical) = std::fs::canonicalize(&path) else {
                    return Ok(false);
                };
                expected_seen |= file_identity(&path).ok().as_ref() == Some(expected);
                // Include selected directories/other native URLs too: they
                // must not silently disappear from the exact-one selection.
                files.insert(canonical);
            }
        }
    }
    DIAGNOSTIC.with(|value| {
        let mut value = value.borrow_mut();
        let shape = value.shape.get_or_insert_with(|| json!({}));
        shape["selected_file_url_count"] = json!(files.len());
        shape["expected_file_url_seen"] = json!(expected_seen);
    });
    Ok(expected_seen && files.len() == 1)
}
fn same_go_to(panel: &Panel, dialog: Ref, path: &str) -> bool {
    resolve(panel.pid, &panel.owner_path).is_some_and(|e| unsafe { CFEqual(e.0, panel.owner.0) })
        && resolve(panel.pid, &panel.path).is_some_and(|e| unsafe { CFEqual(e.0, panel.element.0) })
        && resolve(panel.pid, path).is_some_and(|e| unsafe { CFEqual(e.0, dialog) })
}
// AppKit can acknowledge AXValue before its remote text field publishes the
// updated value. Poll only reads after the single write; never replay input or
// confirm Go until the exact value has been observed in the same bound field.
fn wait_path_readback(
    deadline: Instant,
    mut sample: impl FnMut() -> Result<bool, &'static str>,
    mut now: impl FnMut() -> Instant,
    mut pause: impl FnMut(Duration),
) -> Result<(), &'static str> {
    loop {
        if now() >= deadline {
            return Err("picker_path_write_unverified");
        }
        let matched = sample()?;
        if now() >= deadline {
            return Err("picker_path_write_unverified");
        }
        if matched {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(now());
        if remaining.is_zero() {
            return Err("picker_path_write_unverified");
        }
        pause(remaining.min(Duration::from_millis(15)));
    }
}
fn path_input_chunks(path: &str) -> Result<Vec<Vec<u16>>, &'static str> {
    if path.is_empty()
        || path.len() > 4096
        || path.chars().any(char::is_control)
        || !std::path::Path::new(path).is_absolute()
    {
        return Err("picker_invalid_path");
    }
    let mut chunks = vec![Vec::new()];
    for character in path.chars() {
        let mut buffer = [0u16; 2];
        let units = character.encode_utf16(&mut buffer);
        if chunks.last().unwrap().len() + units.len() > 20 {
            chunks.push(Vec::new());
        }
        chunks.last_mut().unwrap().extend_from_slice(units);
    }
    Ok(chunks)
}
fn path_keyboard_fallback_allowed(error: &str, remaining: Duration) -> bool {
    error == "picker_path_write_unverified" && !remaining.is_zero()
}
fn keyboard_path_replacement(
    panel: &Panel,
    dialog: Ref,
    dialog_path: &str,
    field: &Node,
    path: &str,
    deadline: Instant,
) -> Result<(), &'static str> {
    let chunks = path_input_chunks(path)?;
    let guard = || {
        Instant::now() < deadline
            && same_go_to(panel, dialog, dialog_path)
            && resolve(panel.pid, &field.path)
                .is_some_and(|element| unsafe { CFEqual(element.0, field.element.0) })
            && focus_guard(panel, field.element.0, false, true)
            && focus_within(dialog)
    };
    let current = typed_text(field.element.0, "AXValue").ok_or("picker_path_write_unverified")?;
    if current == path {
        return Ok(());
    }
    if !guard() {
        return Err("picker_focus_changed");
    }
    let source = owned(unsafe { CGEventSourceCreate(1) }).ok_or("picker_keyboard_unavailable")?;
    let select_down = owned(unsafe { CGEventCreateKeyboardEvent(source.0.cast_mut(), 0, true) })
        .ok_or("picker_keyboard_unavailable")?;
    let select_up = owned(unsafe { CGEventCreateKeyboardEvent(source.0.cast_mut(), 0, false) })
        .ok_or("picker_keyboard_unavailable")?;
    unsafe {
        CGEventSetFlags(select_down.0.cast_mut(), 1 << 20);
        CGEventSetFlags(select_up.0.cast_mut(), 1 << 20);
    }
    phase("select_go_to_path_text");
    if !guard() {
        return Err("picker_focus_changed");
    }
    unsafe {
        CGEventPost(0, select_down.0.cast_mut());
        CGEventPost(0, select_up.0.cast_mut());
    }
    let range = Range {
        location: 0,
        length: current.encode_utf16().count() as isize,
    };
    wait_path_readback(
        deadline.min(Instant::now() + Duration::from_millis(350)),
        || {
            if !guard() {
                return Err("picker_focus_changed");
            }
            if typed_text(field.element.0, "AXValue").as_deref() != Some(current.as_str()) {
                return Err("picker_path_readback_changed");
            }
            Ok(selection(field.element.0).is_some_and(|selected| selected == range))
        },
        Instant::now,
        std::thread::sleep,
    )?;
    phase("type_go_to_path_text");
    for units in chunks {
        let down = owned(unsafe { CGEventCreateKeyboardEvent(source.0.cast_mut(), 0, true) })
            .ok_or("picker_keyboard_unavailable")?;
        let up = owned(unsafe { CGEventCreateKeyboardEvent(source.0.cast_mut(), 0, false) })
            .ok_or("picker_keyboard_unavailable")?;
        unsafe {
            CGEventSetFlags(down.0.cast_mut(), 0);
            CGEventSetFlags(up.0.cast_mut(), 0);
            CGEventKeyboardSetUnicodeString(down.0.cast_mut(), units.len(), units.as_ptr());
            CGEventKeyboardSetUnicodeString(up.0.cast_mut(), units.len(), units.as_ptr());
        }
        if !guard() {
            return Err("picker_focus_changed");
        }
        unsafe {
            CGEventPost(0, down.0.cast_mut());
            CGEventPost(0, up.0.cast_mut());
        }
    }
    Ok(())
}
fn confirm_go_to(
    panel: &Panel,
    dialog: Ref,
    dialog_path: &str,
    contents: &[Node],
    field: &Node,
    path: &str,
    expected: &FileIdentity,
) -> Result<(), &'static str> {
    phase("confirm_same_go_to");
    if !same_go_to(panel, dialog, dialog_path)
        || !focus_guard(panel, field.element.0, false, false)
        || !focus_within(dialog)
    {
        return Err("picker_focus_changed");
    }
    if typed_text(field.element.0, "AXValue").as_deref() != Some(path) {
        return Err("picker_path_readback_changed");
    }
    if file_identity(path).ok().as_ref() != Some(expected) {
        return Err("picker_file_changed");
    }
    let button = button(contents, "Go").or_else(|_| confirm_button(dialog, contents, true));
    DIAGNOSTIC.with(|value| {
        let mut value = value.borrow_mut();
        value.navigation_attempts = value.navigation_attempts.saturating_add(1);
        value.navigation_route = Some(if button.is_ok() {
            "native_go_button"
        } else {
            "guarded_go_return"
        });
    });
    if let Ok(index) = button {
        if unsafe { AXUIElementPerformAction(contents[index].element.0, string("AXPress").0) } != 0
        {
            return Err("picker_go_to_dispatch_uncertain");
        }
    } else {
        guarded_key(panel, field.element.0, 36, 0)?;
    }
    Ok(())
}
fn prepare(
    panel: &Panel,
    path: &str,
    expected: &FileIdentity,
    deadline: Instant,
) -> Result<(), &'static str> {
    phase("prepare_panel_focus");
    if !focus_guard(panel, panel.element.0, true, false) {
        return Err("picker_focus_changed");
    }
    phase("discover_go_to_dialog");
    let mut go = go_dialog(panel, deadline)?;
    if go.is_none() {
        let contents = nodes(panel.element.0, &panel.path, deadline)?;
        let go_button = contents.iter().enumerate().find(|(_, n)| {
            n.role == "AXButton"
                && matches!(
                    n.name.to_lowercase().as_str(),
                    "go to folder" | "go to folder…" | "go to the folder…"
                )
        });
        if let Some((_, target)) = go_button {
            phase("open_go_to_button");
            if !focus_guard(panel, panel.element.0, true, false)
                || unsafe { AXUIElementPerformAction(target.element.0, string("AXPress").0) } != 0
            {
                return Err("picker_go_to_unavailable");
            }
        } else {
            guarded_key(panel, panel.element.0, 5, (1 << 17) | (1 << 20))?;
        }
        panel.go_requested.set(true);
        phase("wait_go_to_dialog");
        while Instant::now() < deadline {
            go = go_dialog(panel, deadline)?;
            if go.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(35));
        }
    }
    let (dialog, path_prefix) = go.ok_or("picker_go_to_unavailable")?;
    phase("go_to_dialog_focus");
    if !focus_guard(panel, dialog.0, false, false) {
        return Err("picker_focus_changed");
    }
    let contents = nodes(dialog.0, &path_prefix, deadline)?;
    phase("resolve_go_to_field");
    let field = &contents[path_field(&contents)?];
    // A remote AppKit field may advertise AXValue before becoming the editor.
    // Establish the exact field's focus before the one-shot value setter.
    phase("focus_go_to_field");
    let focus_set = unsafe {
        AXUIElementSetAttributeValue(field.element.0, string("AXFocused").0, kCFBooleanTrue)
    } == 0;
    let focus_ok = focus_guard(panel, field.element.0, false, false);
    focus_set_result(focus_set);
    if !focus_set || !focus_ok {
        return Err("picker_focus_changed");
    }
    phase("write_go_to_path");
    if !same_go_to(panel, dialog.0, &path_prefix)
        || !resolve(panel.pid, &field.path)
            .is_some_and(|element| unsafe { CFEqual(element.0, field.element.0) })
        || !focus_guard(panel, field.element.0, false, false)
    {
        return Err("picker_focus_changed");
    }
    let write_result = unsafe {
        AXUIElementSetAttributeValue(field.element.0, string("AXValue").0, string(path).0)
    };
    DIAGNOSTIC.with(|value| {
        value.borrow_mut().shape = Some(json!({"path_write_ax_result":write_result}));
    });
    let keyboard_fallback = std::cell::Cell::new(false);
    let mut attempts = 0u32;
    let mut readback = || {
        phase("wait_go_to_path_readback");
        wait_path_readback(
            deadline.min(Instant::now() + Duration::from_millis(350)),
            || {
                attempts += 1;
                let bound = same_go_to(panel, dialog.0, &path_prefix)
                    && resolve(panel.pid, &field.path)
                        .is_some_and(|element| unsafe { CFEqual(element.0, field.element.0) });
                let focused = bound && focus_guard(panel, dialog.0, false, false);
                let matches =
                    focused && typed_text(field.element.0, "AXValue").as_deref() == Some(path);
                DIAGNOSTIC.with(|value| {
                    value.borrow_mut().shape = Some(json!({
                        "path_write_ax_result":write_result,
                        "path_keyboard_fallback":keyboard_fallback.get(),
                        "path_readback_attempts":attempts,
                        "path_readback_bound":bound,
                        "path_readback_matches":matches,
                    }));
                });
                if !focused {
                    return Err("picker_focus_changed");
                }
                Ok(matches)
            },
            Instant::now,
            std::thread::sleep,
        )
    };
    let first_read = if write_result == 0 {
        readback()
    } else {
        Err("picker_path_write_unverified")
    };
    if let Err(error) = first_read {
        if !path_keyboard_fallback_allowed(
            error,
            deadline.saturating_duration_since(Instant::now()),
        ) {
            return Err(error);
        }
        keyboard_fallback.set(true);
        DIAGNOSTIC.with(|value| {
            if let Some(shape) = value.borrow_mut().shape.as_mut() {
                shape["path_keyboard_fallback"] = json!(true);
            }
        });
        keyboard_path_replacement(panel, dialog.0, &path_prefix, field, path, deadline)?;
        readback()?;
    }
    phase("validate_source_before_go");
    if file_identity(path).ok().as_ref() != Some(expected) {
        return Err("picker_file_changed");
    }
    confirm_go_to(
        panel,
        dialog.0,
        &path_prefix,
        &contents,
        field,
        path,
        expected,
    )?;
    let mut confirmations = 1u8;
    let mut next_confirmation = Instant::now() + Duration::from_millis(180);
    phase("verify_native_file_selection");
    while Instant::now() < deadline {
        if same_go_to(panel, dialog.0, &path_prefix) {
            // A first Return can accept AppKit autocomplete while retaining the
            // exact same native GoTo sheet. At most two further confirmations
            // are allowed there, never in the original Open panel/composer.
            if confirmations < 3 && Instant::now() >= next_confirmation {
                let refreshed = nodes(dialog.0, &path_prefix, deadline)?;
                if !refreshed.iter().any(|node| {
                    !unsafe { CFEqual(node.element.0, dialog.0) }
                        && native_modal(&node.role, &astr(node.element.0, "AXSubrole"))
                }) && let Ok(index) = path_field(&refreshed)
                {
                    let field = &refreshed[index];
                    if typed_text(field.element.0, "AXValue").as_deref() != Some(path) {
                        return Err("picker_path_readback_changed");
                    }
                    phase("refocus_same_go_to");
                    let mut field_pid = 0;
                    if unsafe { AXUIElementGetPid(field.element.0, &mut field_pid) } != 0
                        || ![panel.pid, field_pid].contains(&pid_of(frontmost()))
                    {
                        focus_guard(panel, field.element.0, false, true);
                        return Err("picker_focus_changed");
                    }
                    if !same_go_to(panel, dialog.0, &path_prefix) {
                        continue;
                    }
                    let accepted = unsafe {
                        AXUIElementSetAttributeValue(
                            field.element.0,
                            string("AXFocused").0,
                            kCFBooleanTrue,
                        )
                    } == 0;
                    let focused = focus_guard(panel, field.element.0, false, false);
                    focus_set_result(accepted);
                    if focused && focus_within(dialog.0) {
                        confirm_go_to(
                            panel,
                            dialog.0,
                            &path_prefix,
                            &refreshed,
                            field,
                            path,
                            expected,
                        )?;
                        confirmations += 1;
                        next_confirmation = Instant::now() + Duration::from_millis(180);
                        phase("verify_native_file_selection");
                    }
                }
            }
        } else if focus_guard(panel, panel.element.0, true, false)
            && selection_proof(panel, expected, deadline)?
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(35));
    }
    Err("picker_selection_path_unverified")
}
pub(super) fn run(args: &[String]) -> Value {
    phase("validate_arguments");
    let fail = |code: &str| {
        let uncertain = DIAGNOSTIC.with(|value| value.borrow().navigation_attempts > 0);
        let mut value = json!({"ok":false,"error_code":code,"selection_dispatch":if uncertain{Value::Null}else{json!(false)},"selection_uncertain":uncertain,"open_attempted":false,"instruction":if uncertain{"GoTo navigation was attempted and may have implicitly accepted the picker. Inspect destination state; never repeat selection because verification is unclear. No upload/delivery proof."}else{"No final Open was dispatched. Inspect the existing picker before continuing; this is not upload or delivery proof."}});
        value
            .as_object_mut()
            .unwrap()
            .extend(diagnostic().as_object().unwrap().clone());
        value
    };
    if !matches!(args.len(), 4 | 5) {
        return fail("picker_invalid_arguments");
    }
    let Ok(pid) = args[1].parse::<i32>() else {
        return fail("picker_invalid_arguments");
    };
    let Ok(id) = args[2].parse::<u32>() else {
        return fail("picker_invalid_arguments");
    };
    if pid <= 0 || id == 0 {
        return fail("picker_invalid_arguments");
    }
    phase("validate_source");
    let expected = match file_identity(&args[3]) {
        Ok(value) => value,
        Err(code) => return fail(code),
    };
    phase("validate_expected_source");
    if let Some(raw) = args.get(4)
        && let Err(code) = expected_source_matches(raw, &expected)
    {
        return fail(code);
    }
    if !unsafe { AXIsProcessTrusted() } {
        return fail("picker_accessibility_unavailable");
    }
    let deadline = Instant::now() + Duration::from_millis(4200);
    phase("bind_native_panel");
    let panel = match panel(&args[0], pid, id, deadline) {
        Ok(panel) => panel,
        Err(code) => return fail(code),
    };
    if let Err(code) = prepare(&panel, &args[3], &expected, deadline) {
        return fail(code);
    }
    phase("revalidate_source_before_open");
    if !focus_guard(&panel, panel.element.0, true, false)
        || file_identity(&args[3]).ok().as_ref() != Some(&expected)
    {
        return fail("picker_target_or_file_changed");
    }
    phase("resolve_final_open");
    let contents = match nodes(panel.element.0, &panel.path, deadline) {
        Ok(nodes) => nodes,
        Err(code) => return fail(code),
    };
    let open = match confirm_button(panel.element.0, &contents, true) {
        Ok(index) => &contents[index],
        Err(code) => return fail(code),
    };
    phase("verify_selection_before_open");
    if !selection_proof(&panel, &expected, deadline).unwrap_or(false)
        || !focus_guard(&panel, panel.element.0, true, false)
    {
        return fail("picker_selection_path_unverified");
    }
    // One-shot boundary. Even AX returning an error may have dispatched Open.
    // Never retry, fall back to coordinates, or claim upload/delivery success.
    phase("dispatch_open_once");
    // Discovery uses short read timeouts. The XPC panel can close during its
    // handler, so allow a bounded acknowledgement window for this one action.
    unsafe {
        AXUIElementSetMessagingTimeout(open.element.0, 0.75);
    }
    let ax_status = unsafe { AXUIElementPerformAction(open.element.0, string("AXPress").0) };
    let accepted = ax_status == 0;
    json!({"ok":accepted,"phase":"dispatch_open_once","source":"native_file_picker","app":{"pid":pid},"window":{"id":panel.window_id},"path":args[3],"bytes":expected.bytes,"source_identity":source_identity(&expected),"selection_dispatch":if accepted{json!(true)}else{Value::Null},"selection_uncertain":!accepted,"open_attempted":true,"native_action":{"name":"AXPress","attempts":1,"ax_status":ax_status},"error_code":if accepted{Value::Null}else{json!("picker_open_dispatch_uncertain")},"instruction":"Final Open was attempted once. Inspect the destination attachment state; this receipt proves neither upload nor message delivery. Never repeat Open because verification is unclear."})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyboard_path_chunks_preserve_literal_unicode_and_never_split_surrogates() {
        let path = "/tmp/123456789012345😀;/literal $name.json";
        let chunks = path_input_chunks(path).unwrap();
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(!chunk.is_empty() && chunk.len() <= 20);
            assert!(String::from_utf16(chunk).is_ok());
        }
        assert_eq!(String::from_utf16(&chunks.concat()).unwrap(), path);
        for bad in ["relative", "", "/tmp/new\nline", "/tmp/key\u{1b}"] {
            assert_eq!(path_input_chunks(bad), Err("picker_invalid_path"));
        }
    }
    #[test]
    fn path_write_diagnostics_survive_fallback_phases_without_private_path_text() {
        phase("validate_arguments");
        DIAGNOSTIC.with(|value| {
            value.borrow_mut().shape =
                Some(json!({"path_write_ax_result":-25204,"path_keyboard_fallback":true}))
        });
        for name in [
            "select_go_to_path_text",
            "type_go_to_path_text",
            "wait_go_to_path_readback",
        ] {
            phase(name);
            assert_eq!(diagnostic()["native_shape"]["path_write_ax_result"], -25204);
            assert_eq!(diagnostic()["native_shape"]["path_keyboard_fallback"], true);
        }
        phase("validate_arguments");
        assert!(diagnostic()["native_shape"].is_null());
    }
    #[test]
    fn path_readback_waits_for_async_publication_without_repeating_the_write() {
        let start = Instant::now();
        let clock = std::cell::Cell::new(start);
        let samples = std::cell::Cell::new(0usize);
        let waits = std::cell::Cell::new(0usize);
        let writes = std::cell::Cell::new(0usize);
        // The caller performs exactly one write, followed only by reads.
        writes.set(writes.get() + 1);
        let result = wait_path_readback(
            start + Duration::from_millis(350),
            || {
                samples.set(samples.get() + 1);
                Ok(samples.get() == 3)
            },
            || clock.get(),
            |duration| {
                waits.set(waits.get() + 1);
                clock.set(clock.get() + duration);
            },
        );
        assert_eq!(result, Ok(()));
        assert_eq!(samples.get(), 3);
        assert_eq!(waits.get(), 2);
        assert_eq!(writes.get(), 1);
        assert_eq!(clock.get() - start, Duration::from_millis(30));
    }
    #[test]
    fn path_readback_has_no_delay_when_ready_and_stops_on_lost_binding() {
        let start = Instant::now();
        assert_eq!(
            wait_path_readback(
                start + Duration::from_millis(350),
                || Ok(true),
                || start,
                |_| panic!("an exact immediate readback must not wait"),
            ),
            Ok(())
        );
        assert_eq!(
            wait_path_readback(
                start + Duration::from_millis(350),
                || Err("picker_focus_changed"),
                || start,
                |_| panic!("an identity change must stop immediately"),
            ),
            Err("picker_focus_changed")
        );
    }
    #[test]
    fn path_readback_deadline_is_bounded_even_when_a_native_getter_is_slow() {
        let start = Instant::now();
        let deadline = start + Duration::from_millis(32);
        let clock = std::cell::Cell::new(start);
        let samples = std::cell::Cell::new(0usize);
        assert_eq!(
            wait_path_readback(
                deadline,
                || {
                    samples.set(samples.get() + 1);
                    Ok(false)
                },
                || clock.get(),
                |duration| clock.set(clock.get() + duration),
            ),
            Err("picker_path_write_unverified")
        );
        assert_eq!(clock.get(), deadline);
        assert_eq!(samples.get(), 3);
        assert_eq!(
            wait_path_readback(
                deadline,
                || panic!("expired transaction must not read another UI field"),
                || clock.get(),
                |_| panic!("expired transaction must not wait"),
            ),
            Err("picker_path_write_unverified")
        );
        clock.set(start);
        assert_eq!(
            wait_path_readback(
                deadline,
                || {
                    clock.set(deadline);
                    Ok(true)
                },
                || clock.get(),
                |_| panic!("late readback must not wait or confirm"),
            ),
            Err("picker_path_write_unverified")
        );
    }
    #[test]
    fn expected_source_rejects_changed_same_name_before_any_native_action() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("same.mov");
        std::fs::write(&file, b"old").unwrap();
        let before = file_identity(file.to_str().unwrap()).unwrap();
        let raw = json!({"bytes":before.bytes,"identity":source_identity(&before)}).to_string();
        assert!(expected_source_matches(&raw, &before).is_ok());
        std::fs::write(&file, b"different bytes").unwrap();
        let value = run(&[
            "App".into(),
            "1".into(),
            "1".into(),
            file.to_str().unwrap().into(),
            raw,
        ]);
        assert_eq!(value["error_code"], "picker_source_changed");
        assert_eq!(value["selection_dispatch"], false);
        assert_eq!(value["open_attempted"], false);
    }
    #[test]
    fn expected_source_schema_rejects_duplicates_wrong_types_and_invalid_nanos() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("clip.mov");
        std::fs::write(&file, b"file").unwrap();
        let current = file_identity(file.to_str().unwrap()).unwrap();
        let valid = json!({"bytes":current.bytes,"identity":source_identity(&current)});
        for key in [
            "device",
            "inode",
            "modified_seconds",
            "modified_nanoseconds",
            "changed_seconds",
            "changed_nanoseconds",
        ] {
            let mut invalid = valid.clone();
            invalid["identity"][key] = json!("1");
            assert!(expected_source_matches(&invalid.to_string(), &current).is_err());
        }
        let mut invalid = valid.clone();
        invalid["identity"]["modified_nanoseconds"] = json!(1_000_000_000u64);
        assert!(expected_source_matches(&invalid.to_string(), &current).is_err());
        let raw = valid
            .to_string()
            .replacen("\"bytes\":", "\"bytes\":0,\"bytes\":", 1);
        assert_eq!(
            expected_source_matches(&raw, &current),
            Err("picker_invalid_expected_source")
        );
        let mut invalid = valid.clone();
        invalid["extra"] = json!(1);
        assert!(expected_source_matches(&invalid.to_string(), &current).is_err());
    }
    #[test]
    fn native_picker_confirm_labels_allow_custom_attach_but_never_send_or_save() {
        assert!(confirm_label_allowed("Attach owned file"));
        assert!(confirm_label_allowed("Open"));
        for label in ["Send", "Send file", "Save", "Replace", "Publish", "Buy now"] {
            assert!(!confirm_label_allowed(label));
        }
    }
    #[test]
    fn path_and_file_identity_are_literal_and_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("clip ; $literal😀.mov");
        std::fs::write(&file, b"one").unwrap();
        let path = file.to_str().unwrap();
        let before = file_identity(path).unwrap();
        assert_eq!(before, file_identity(path).unwrap());
        std::fs::write(&file, b"changed size").unwrap();
        assert_ne!(before, file_identity(path).unwrap());
        for bad in [
            "relative.mov",
            "/tmp/line\nfile",
            dir.path().to_str().unwrap(),
        ] {
            assert!(file_identity(bad).is_err());
        }
    }
    #[test]
    fn application_binding_is_exact_not_substring() {
        assert!(app_matches(
            "Teams",
            "Microsoft Teams",
            "com.microsoft.teams2"
        ));
        assert!(app_matches(
            "com.example.fixture",
            "Fixture",
            "com.example.fixture"
        ));
        assert!(!app_matches(
            "Team",
            "Microsoft Teams",
            "com.microsoft.teams2"
        ));
        assert!(!app_matches(
            "Fixture",
            "Other Fixture",
            "com.example.other"
        ));
    }
    #[test]
    fn native_modal_shape_excludes_web_and_standard_windows() {
        assert!(native_modal("AXSheet", ""));
        assert!(native_modal("AXWindow", "AXDialog"));
        assert!(!native_modal("AXWindow", "AXStandardWindow"));
        assert!(!native_modal("AXWebArea", "AXDialog"));
    }
    #[test]
    fn malformed_requests_never_reach_accessibility_or_input() {
        for args in [
            vec![],
            vec!["App", "0", "1", "/tmp/file"],
            vec!["App", "1", "0", "/tmp/file"],
            vec!["App", "1", "2", "relative"],
        ] {
            let value = run(&args.into_iter().map(str::to_owned).collect::<Vec<_>>());
            assert_eq!(value["ok"], false);
            assert_eq!(value["selection_dispatch"], false);
            assert_eq!(value["open_attempted"], false);
        }
    }
}
#[test]
fn path_keyboard_fallback_only_recovers_pre_submit_write_or_readback_failure() {
    assert!(path_keyboard_fallback_allowed(
        "picker_path_write_unverified",
        Duration::from_millis(1)
    ));
    assert!(!path_keyboard_fallback_allowed(
        "picker_path_write_unverified",
        Duration::ZERO
    ));
    for error in [
        "picker_focus_changed",
        "picker_file_changed",
        "picker_go_to_dispatch_uncertain",
        "picker_path_readback_changed",
    ] {
        assert!(!path_keyboard_fallback_allowed(
            error,
            Duration::from_secs(1)
        ));
    }
}
