//! Read saved Excel workbook identity when AXDocument is absent. No contents,
//! activation, shell commands supplied by a model, edits, or file discovery.
use super::*;
use std::{
    io::Read,
    process::{Command, Stdio},
};

// Excel's activeWorkbook accessor is unavailable on some releases. Bind a
// workbook through its own windows instead; never infer a path from its title.
const SCRIPT: &str = "function run(){const a=Application('com.microsoft.Excel');function read(){const bs=a.workbooks();if(bs.length>12)throw Error('Workbook limit');return bs.map(b=>{const ws=b.windows();if(ws.length>12)throw Error('Window limit');return {full_name:String(b.fullName()),directory:String(b.path()),windows:ws.map(w=>({name:String(w.name()),bounds:w.bounds()}))};});}const before=read(),after=read();if(JSON.stringify(before)!==JSON.stringify(after))throw Error('Workbook changed');return JSON.stringify(before);}";

fn scripted_workbooks() -> Result<Value, &'static str> {
    let mut child = Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e", SCRIPT])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "file_context_document_unavailable")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("file_context_document_unavailable")?;
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
        .filter(|data| data.len() <= 32_768)
        .ok_or("file_context_document_unavailable")?;
    if !status.is_some_and(|status| status.success()) {
        return Err("file_context_document_unavailable");
    }
    serde_json::from_slice(&output).map_err(|_| "file_context_document_unavailable")
}

fn document(workbooks: &Value, w: &Window) -> Result<Value, &'static str> {
    if w.bundle != "com.microsoft.Excel" {
        return Err("file_context_document_unavailable");
    }
    let books = workbooks
        .as_array()
        .filter(|books| books.len() <= 12)
        .ok_or("file_context_document_unavailable")?;
    let mut matched = Vec::new();
    for book in books {
        let windows = book["windows"]
            .as_array()
            .filter(|windows| windows.len() <= 12)
            .ok_or("file_context_document_unavailable")?;
        for window in windows {
            let b = &window["bounds"];
            let numbers: Option<Vec<f64>> = ["x", "y", "width", "height"]
                .iter()
                .map(|key| b[*key].as_f64().filter(|number| number.is_finite()))
                .collect();
            let Some(n) = numbers else {
                return Err("file_context_document_unavailable");
            };
            let bounds = Rect {
                p: Point { x: n[0], y: n[1] },
                s: Size { w: n[2], h: n[3] },
            };
            if window["name"].as_str() == Some(w.title.as_str()) && near(bounds, w.b, 2.) {
                matched.push(book);
            }
        }
    }
    if matched.len() != 1 {
        return Err("file_context_window_ambiguous");
    }
    let book = matched[0];
    let path = book["full_name"]
        .as_str()
        .filter(|path| path.len() <= 4096 && !path.chars().any(char::is_control))
        .ok_or("file_context_document_unavailable")?;
    let directory = book["directory"]
        .as_str()
        .filter(|directory| !directory.is_empty())
        .ok_or("file_context_document_unavailable")?;
    if !std::path::Path::new(path).is_absolute()
        || std::path::Path::new(path).parent() != Some(std::path::Path::new(directory))
    {
        return Err("file_context_document_unavailable");
    }
    let mut url = String::from("file://");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            url.push(byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(&mut url, "%{byte:02X}");
        }
    }
    let mut document = local_file_document(&url).ok_or("file_context_document_unavailable")?;
    document["method"] = json!("excel_scripting_workbook_metadata");
    Ok(document)
}

fn verify_unchanged(
    before: &Value,
    after: &Value,
    original: &Value,
    w: &Window,
) -> Result<(), &'static str> {
    if before != after || document(after, w)? != *original {
        return Err("file_context_source_changed");
    }
    Ok(())
}
pub(super) fn resolve(w: &Window, element: Ref) -> Result<Value, &'static str> {
    if w.bundle != "com.microsoft.Excel" {
        return Err("file_context_document_unavailable");
    }
    let before = scripted_workbooks()?;
    let document = document(&before, w)?;
    let after = scripted_workbooks()?;
    verify_unchanged(&before, &after, &document, w)?;
    if !frame(element).is_some_and(|bounds| near(bounds, w.b, 12.))
        || !window_inventory(false)
            .iter()
            .any(|next| same_file_window(w, next))
    {
        return Err("file_context_source_changed");
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn window() -> Window {
        Window {
            id: 7,
            pid: 42,
            name: "Microsoft Excel".into(),
            title: "Owned workbook".into(),
            bundle: "com.microsoft.Excel".into(),
            b: Rect {
                p: Point { x: 0., y: 33. },
                s: Size { w: 800., h: 600. },
            },
        }
    }
    fn book(path: &std::path::Path) -> Value {
        json!([{"full_name":path,"directory":path.parent(),"windows":[{"name":"Owned workbook","bounds":{"x":0,"y":33,"width":800,"height":600}}]}])
    }
    #[test]
    fn saved_workbook_path_comes_from_exact_owned_window_not_title_guess() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("literal #;$ 😀.xlsx");
        std::fs::write(&path, b"owned").unwrap();
        let books = book(&path);
        let doc = document(&books, &window()).unwrap();
        assert_eq!(doc["path"], path.to_str().unwrap());
        assert_eq!(doc["source"], "window_document");
        assert_eq!(doc["method"], "excel_scripting_workbook_metadata");
        assert_eq!(doc["read_only"], true);
        assert_eq!(doc["content_read"], false);
        assert!(verify_unchanged(&books, &books, &doc, &window()).is_ok());
        for (pointer, value) in [
            ("/0/windows/0/name", json!("Another workbook")),
            ("/0/windows/0/bounds/x", json!(100)),
            ("/0/full_name", json!("https://example.com/book.xlsx")),
            ("/0/directory", json!("")),
            ("/0/full_name", json!("Book1")),
        ] {
            let mut bad = books.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(document(&bad, &window()).is_err(), "{pointer}");
        }
        let mut other = window();
        other.bundle = "com.example.app".into();
        assert!(document(&books, &other).is_err());
        let mut ambiguous = books.clone();
        ambiguous.as_array_mut().unwrap().push(books[0].clone());
        assert!(document(&ambiguous, &window()).is_err());
        let folder = book(root.path());
        assert!(document(&folder, &window()).is_err());
    }
    #[test]
    fn source_swap_file_changes_and_duplicate_windows_never_reuse_workbook_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.xlsx");
        let other = root.path().join("b.xlsx");
        std::fs::write(&path, b"same").unwrap();
        std::fs::write(&other, b"same").unwrap();
        let books = book(&path);
        let doc = document(&books, &window()).unwrap();
        assert!(verify_unchanged(&books, &book(&other), &doc, &window()).is_err());
        std::fs::rename(&other, &path).unwrap();
        assert!(verify_unchanged(&books, &books, &doc, &window()).is_err());
        let mut duplicate = books.clone();
        duplicate[0]["windows"]
            .as_array_mut()
            .unwrap()
            .push(books[0]["windows"][0].clone());
        assert!(document(&duplicate, &window()).is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(document(&books, &window()).is_err());
    }
}
