//! Bounded AX notification wakeups. Events are hints to re-read native state,
//! never proof that an action or user task completed. No trees or pixels.
use super::*;
use std::cell::Cell;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXObserverCreate(
        pid: i32,
        callback: unsafe extern "C" fn(Ref, Ref, Ref, *mut c_void),
        observer: *mut Ref,
    ) -> i32;
    fn AXObserverAddNotification(
        observer: Ref,
        element: Ref,
        name: Ref,
        context: *mut c_void,
    ) -> i32;
    fn AXObserverRemoveNotification(observer: Ref, element: Ref, name: Ref) -> i32;
    fn AXObserverGetRunLoopSource(observer: Ref) -> Ref;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRunLoopGetCurrent() -> Ref;
    fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRemoveSource(run_loop: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRunInMode(mode: Ref, seconds: f64, return_after_source: bool) -> i32;
    fn CFRunLoopStop(run_loop: Ref);
    static kCFRunLoopDefaultMode: Ref;
}
#[derive(Debug, PartialEq, Eq)]
struct Request {
    app: String,
    pid: i32,
    window: u32,
    timeout_ms: u64,
}
fn arguments(args: &[String]) -> Option<Request> {
    if args.len() != 4
        || args[0].trim().is_empty()
        || args[0].len() > 200
        || args[0].chars().any(char::is_control)
    {
        return None;
    }
    Some(Request {
        app: args[0].clone(),
        pid: args[1].parse::<i32>().ok().filter(|p| *p > 1)?,
        window: args[2].parse::<u32>().ok().filter(|id| *id > 0)?,
        timeout_ms: args[3]
            .parse::<u64>()
            .ok()
            .filter(|ms| (1..=1000).contains(ms))?,
    })
}
fn source_matches(window: &Window, request: &Request) -> bool {
    window.pid == request.pid
        && window.id == request.window
        && (window.name.eq_ignore_ascii_case(&request.app)
            || window.bundle.eq_ignore_ascii_case(&request.app))
}
struct Wake {
    changed: Cell<bool>,
    run_loop: Ref,
}
unsafe extern "C" fn notification(_observer: Ref, _element: Ref, _name: Ref, context: *mut c_void) {
    if context.is_null() {
        return;
    }
    // The registration guard removes all notifications and the run-loop source
    // before this stack-owned Wake is dropped. Delivery runs on this thread.
    let state = unsafe { &*(context as *const Wake) };
    state.changed.set(true);
    unsafe { CFRunLoopStop(state.run_loop) };
}
struct Registration {
    observer: Owned,
    source: Ref,
    run_loop: Ref,
    notifications: Vec<(Ref, Owned)>,
}
impl Drop for Registration {
    fn drop(&mut self) {
        for (element, name) in &self.notifications {
            unsafe { AXObserverRemoveNotification(self.observer.0, *element, name.0) };
        }
        unsafe { CFRunLoopRemoveSource(self.run_loop, self.source, kCFRunLoopDefaultMode) };
    }
}
fn reply(
    request: Option<&Request>,
    started: Instant,
    available: bool,
    changed: bool,
    bound: bool,
    reason: &str,
) -> Value {
    json!({"ok":true,"source":"native_ax_notifications","available":available,"changed":changed,"binding_valid":bound,"completion_proof":false,"pid":request.map(|r|r.pid),"window_id":request.map(|r|r.window),"timeout_ms":request.map(|r|r.timeout_ms),"elapsed_ms":started.elapsed().as_millis().min(u64::MAX as u128) as u64,"reason":reason})
}
pub(super) fn wait(args: &[String]) -> Value {
    let started = Instant::now();
    let Some(request) = arguments(args) else {
        return reply(None, started, false, false, false, "invalid_arguments");
    };
    let deadline = started + Duration::from_millis(request.timeout_ms);
    if !unsafe { AXIsProcessTrusted() } {
        return reply(
            Some(&request),
            started,
            false,
            false,
            false,
            "accessibility_unavailable",
        );
    }
    let inventory = window_inventory(false);
    let Some(window) = inventory.iter().find(|w| source_matches(w, &request)) else {
        return reply(
            Some(&request),
            started,
            false,
            false,
            false,
            "source_changed",
        );
    };
    let application = app(request.pid);
    unsafe { AXUIElementSetMessagingTimeout(application.0, 0.04) };
    let windows = children(application.0, "AXWindows");
    let matches: Vec<_> = windows
        .iter()
        .filter(|e| frame(e.0).is_some_and(|b| near(b, window.b, 12.)))
        .collect();
    if matches.len() != 1 || Instant::now() >= deadline {
        return reply(
            Some(&request),
            started,
            false,
            false,
            false,
            "window_binding_unavailable",
        );
    }
    let root = matches[0];
    unsafe { AXUIElementSetMessagingTimeout(root.0, 0.04) };
    let run_loop = unsafe { CFRunLoopGetCurrent() };
    if run_loop.is_null() {
        return reply(
            Some(&request),
            started,
            false,
            false,
            true,
            "run_loop_unavailable",
        );
    }
    let state = Wake {
        changed: Cell::new(false),
        run_loop,
    };
    let context = (&state as *const Wake).cast_mut().cast::<c_void>();
    let mut raw = ptr::null();
    if unsafe { AXObserverCreate(request.pid, notification, &mut raw) } != 0 {
        return reply(
            Some(&request),
            started,
            false,
            false,
            true,
            "observer_unavailable",
        );
    }
    let Some(observer) = owned(raw) else {
        return reply(
            Some(&request),
            started,
            false,
            false,
            true,
            "observer_unavailable",
        );
    };
    let source = unsafe { AXObserverGetRunLoopSource(observer.0) };
    if source.is_null() {
        return reply(
            Some(&request),
            started,
            false,
            false,
            true,
            "observer_source_unavailable",
        );
    }
    let mut registration = Registration {
        observer,
        source,
        run_loop,
        notifications: Vec::new(),
    };
    for (element, name) in [
        (root.0, "AXLayoutChanged"),
        (root.0, "AXValueChanged"),
        (root.0, "AXUIElementDestroyed"),
        (application.0, "AXWindowCreated"),
        (application.0, "AXFocusedWindowChanged"),
    ] {
        if Instant::now() >= deadline {
            break;
        }
        let name = string(name);
        if unsafe { AXObserverAddNotification(registration.observer.0, element, name.0, context) }
            == 0
        {
            registration.notifications.push((element, name));
        }
    }
    if registration.notifications.is_empty() {
        return reply(
            Some(&request),
            started,
            false,
            false,
            true,
            "notifications_unsupported",
        );
    }
    unsafe { CFRunLoopAddSource(run_loop, source, kCFRunLoopDefaultMode) };
    while !state.changed.get() && Instant::now() < deadline {
        let duration = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(50));
        // This actually waits on the observer's run-loop source. No polling tree
        // traversal or sleep is substituted for notification availability.
        let status =
            unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, duration.as_secs_f64(), true) };
        if status == 1 {
            break;
        } // No live run-loop sources: do not spin.
    }
    let changed = state.changed.get();
    // Unregister before collecting more metadata or dropping callback state.
    drop(registration);
    let after = window_inventory(false);
    let fresh = after.iter().find(|w| source_matches(w, &request));
    let bound = fresh.is_some_and(|w| frame(root.0).is_some_and(|b| near(b, w.b, 12.)));
    reply(
        Some(&request),
        started,
        true,
        changed,
        bound,
        if !bound {
            "source_changed"
        } else if changed {
            "notification_hint"
        } else {
            "timeout"
        },
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(app: &str, pid: &str, window: &str, ms: &str) -> Vec<String> {
        [app, pid, window, ms].map(str::to_owned).to_vec()
    }
    #[test]
    fn notification_arguments_are_bounded_and_never_request_a_tree() {
        assert!(arguments(&args("Microsoft Teams", "42", "7", "500")).is_some());
        for a in [
            args("", "42", "7", "500"),
            args("Teams\nOther", "42", "7", "500"),
            args("Teams", "0", "7", "500"),
            args("Teams", "42", "0", "500"),
            args("Teams", "42", "7", "0"),
            args("Teams", "42", "7", "1001"),
            args("Teams", "42", "7", "NaN"),
        ] {
            assert!(arguments(&a).is_none());
        }
    }
    #[test]
    fn event_source_requires_exact_app_process_and_window_not_a_name_guess() {
        let request = arguments(&args("Microsoft Teams", "42", "7", "500")).unwrap();
        let make = |pid, id, name: &str| Window {
            id,
            pid,
            name: name.into(),
            title: "Private title must never be returned".into(),
            bundle: "com.microsoft.teams2".into(),
            b: Rect {
                p: Point { x: 0., y: 0. },
                s: Size { w: 500., h: 500. },
            },
        };
        assert!(source_matches(&make(42, 7, "Microsoft Teams"), &request));
        assert!(!source_matches(&make(43, 7, "Microsoft Teams"), &request));
        assert!(!source_matches(&make(42, 8, "Microsoft Teams"), &request));
        assert!(!source_matches(&make(42, 7, "Other App"), &request));
        let result = reply(
            Some(&request),
            Instant::now(),
            true,
            true,
            true,
            "notification_hint",
        );
        assert_eq!(result["completion_proof"], false);
        assert!(!result.to_string().contains("Private"));
    }
}
