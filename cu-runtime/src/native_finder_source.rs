//! Exact Finder selection metadata. No activation, content reads, or pixels.
use super::*;
use std::{
    io::Read,
    process::{Command, Stdio},
};

// Finder's scripting selection belongs to its front Finder window, not to
// whichever app happens to be active. Bind that window on every read.
const SCRIPT: &str = "function run(){const a=Application('com.apple.finder'); function read(){const ws=a.finderWindows();if(!ws.length)throw Error('No Finder window');const w=ws[0],b=w.bounds(),s=a.selection();if(s.length!==1)throw Error('Select exactly one item');return {window_id:Number(w.id()),bounds:b,urls:s.map(x=>String(x.url()))};} const before=read(),after=read();if(JSON.stringify(before)!==JSON.stringify(after))throw Error('Selection changed');return JSON.stringify(before);}";

fn scripted_selection() -> Result<Value, &'static str> {
    let mut child = Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e", SCRIPT])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "file_context_selection_unavailable")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("file_context_selection_unavailable")?;
    let reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        stdout.take(32_769).read_to_end(&mut data).map(|_| data)
    });
    let until = Instant::now() + Duration::from_millis(900);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let output = reader
        .join()
        .ok()
        .and_then(Result::ok)
        .filter(|b| b.len() <= 32_768)
        .ok_or("file_context_selection_unavailable")?;
    if !status.is_some_and(|s| s.success()) {
        return Err("file_context_selection_unavailable");
    }
    serde_json::from_slice(&output).map_err(|_| "file_context_selection_unavailable")
}

fn selection_document(selection: &Value, w: &Window) -> Result<Value, &'static str> {
    let b = &selection["bounds"];
    let rect = Rect {
        p: Point {
            x: b["x"]
                .as_f64()
                .ok_or("file_context_selection_window_mismatch")?,
            y: b["y"]
                .as_f64()
                .ok_or("file_context_selection_window_mismatch")?,
        },
        s: Size {
            w: b["width"]
                .as_f64()
                .ok_or("file_context_selection_window_mismatch")?,
            h: b["height"]
                .as_f64()
                .ok_or("file_context_selection_window_mismatch")?,
        },
    };
    if !near(rect, w.b, 2.) || selection["window_id"].as_u64().filter(|n| *n > 0).is_none() {
        return Err("file_context_selection_window_mismatch");
    }
    let urls = selection["urls"]
        .as_array()
        .filter(|a| a.len() == 1)
        .ok_or("file_context_selection_ambiguous")?;
    let raw = urls[0]
        .as_str()
        .filter(|s| s.len() <= 16_384)
        .ok_or("file_context_selection_unavailable")?;
    let mut document =
        local_file_document(raw).ok_or("file_context_selected_regular_file_unavailable")?;
    document["source"] = json!("finder_selection");
    document["bundle_id"] = json!("com.apple.finder");
    document["selection_count"] = json!(1);
    document["selection_verified"] = json!(true);
    document["selection_window_verified"] = json!(true);
    document["method"] = json!("finder_scripting_selection");
    document["instruction"] = json!(
        "One selected regular file from the exact Finder window. Contents were not read. Revalidate selection and file identity before attaching; obtain Send approval before sending."
    );
    Ok(document)
}

fn verify_unchanged(
    before: &Value,
    after: &Value,
    document: &Value,
    w: &Window,
) -> Result<(), &'static str> {
    if before != after || selection_document(after, w)? != *document {
        return Err("file_context_source_changed");
    }
    Ok(())
}

pub(super) fn resolve(w: &Window, element: Ref) -> Result<Value, &'static str> {
    // AXSelectedChildren/AXURL is faster when Finder exposes it directly. The
    // script also verifies selection/window ownership rather than trusting a
    // path inferred from an AX label or a folder's AXDocument attribute.
    let ax_selection = || {
        let selected = children(element, "AXSelectedChildren");
        if selected.is_empty() {
            return None;
        }
        let urls: Option<Vec<String>> = selected
            .iter()
            .map(|e| {
                attr(e.0, "AXURL")
                    .map(|v| text(v.0))
                    .filter(|s| !s.is_empty())
            })
            .collect();
        urls.map(|urls| json!({"window_id":w.id,"bounds":{"x":w.b.p.x,"y":w.b.p.y,"width":w.b.s.w,"height":w.b.s.h},"urls":urls}))
    };
    let native = ax_selection();
    let use_ax = native.is_some();
    let before = match native {
        Some(value) => value,
        None => scripted_selection()?,
    };
    let mut document = selection_document(&before, w)?;
    let after = if use_ax {
        ax_selection().ok_or("file_context_source_changed")?
    } else {
        scripted_selection()?
    };
    verify_unchanged(&before, &after, &document, w)?;
    if !frame(element).is_some_and(|b| near(b, w.b, 12.))
        || !window_inventory(false)
            .iter()
            .any(|next| same_file_window(w, next))
    {
        return Err("file_context_source_changed");
    }
    if use_ax {
        document["method"] = json!("finder_ax_selection");
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_regular_file_requires_exact_window_and_single_selection() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("owned.mov");
        std::fs::write(&file, b"test").unwrap();
        let w = Window {
            id: 4,
            pid: 42,
            name: "Finder".into(),
            title: "Owned".into(),
            bundle: "com.apple.finder".into(),
            b: Rect {
                p: Point { x: 2., y: 3. },
                s: Size { w: 800., h: 600. },
            },
        };
        let good = json!({"window_id":9,"bounds":{"x":2,"y":3,"width":800,"height":600},"urls":[format!("file://{}",file.display())]});
        let doc = selection_document(&good, &w).unwrap();
        assert_eq!(doc["source"], "finder_selection");
        assert_eq!(doc["content_read"], false);
        assert!(verify_unchanged(&good, &good, &doc, &w).is_ok());
        let mut changed = good.clone();
        changed["window_id"] = json!(10);
        assert!(verify_unchanged(&good, &changed, &doc, &w).is_err());
        changed = good.clone();
        changed["urls"] = json!(["file:///changed.mov"]);
        assert!(verify_unchanged(&good, &changed, &doc, &w).is_err());
        let replacement = root.path().join("replacement.mov");
        std::fs::write(&replacement, b"same").unwrap();
        std::fs::rename(replacement, &file).unwrap();
        assert!(verify_unchanged(&good, &good, &doc, &w).is_err());
        for invalid in [
            json!([]),
            json!(["file:///a", "file:///b"]),
            json!([format!("file://{}", root.path().display())]),
        ] {
            let mut v = good.clone();
            v["urls"] = invalid;
            assert!(selection_document(&v, &w).is_err());
        }
        let mut v = good;
        v["bounds"]["x"] = json!(400);
        assert!(selection_document(&v, &w).is_err());
    }
}
