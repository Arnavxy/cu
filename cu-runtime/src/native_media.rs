//! Native media and input bridges. No shell, subprocess, or Swift runtime.
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr, thread,
    time::Duration,
};
type Id = *mut c_void;
type Sel = *const c_void;
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Point {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Size {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    origin: Point,
    size: Size,
}
#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> *const c_void;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
    #[cfg(target_arch = "x86_64")]
    fn objc_msgSend_stret();
    fn objc_autoreleasePoolPush() -> Id;
    fn objc_autoreleasePoolPop(pool: Id);
}
#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {}
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}
#[link(name = "Vision", kind = "framework")]
unsafe extern "C" {}
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: *const c_void);
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGImageGetWidth(image: Id) -> usize;
    fn CGImageGetHeight(image: Id) -> usize;
    fn CGColorSpaceCreateDeviceRGB() -> Id;
    fn CGBitmapContextCreate(
        data: Id,
        w: usize,
        h: usize,
        bits: usize,
        row: usize,
        space: Id,
        info: u32,
    ) -> Id;
    fn CGContextSetInterpolationQuality(context: Id, quality: i32);
    fn CGContextDrawImage(context: Id, rect: Rect, image: Id);
    fn CGEventSourceCreate(state: i32) -> Id;
    fn CGEventCreateKeyboardEvent(source: Id, key: u16, down: bool) -> Id;
    fn CGEventSetFlags(event: Id, flags: u64);
    fn CGEventPost(tap: u32, event: Id);
    fn CGEventCreateMouseEvent(source: Id, kind: u32, point: Point, button: u32) -> Id;
    fn CGEventCreateScrollWheelEvent2(
        source: Id,
        units: u32,
        wheels: u32,
        one: i32,
        two: i32,
        three: i32,
    ) -> Id;
    fn CGEventSetLocation(event: Id, point: Point);
}
fn class(name: &str) -> Id {
    let name = CString::new(name).unwrap();
    unsafe { objc_getClass(name.as_ptr()).cast_mut() }
}
fn sel(name: &str) -> Sel {
    let name = CString::new(name).unwrap();
    unsafe { sel_registerName(name.as_ptr()) }
}
macro_rules! msg {
 ($obj:expr,$selector:expr => $ret:ty $(,$arg:expr => $typ:ty)*) => {{
  let f:unsafe extern "C" fn(Id,Sel $(,$typ)*)->$ret=unsafe{std::mem::transmute(objc_msgSend as *const ())};
  unsafe{f($obj,sel($selector) $(,$arg)*)}
 }};
}
struct Pool(Id);
impl Pool {
    fn new() -> Self {
        Self(unsafe { objc_autoreleasePoolPush() })
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        unsafe { objc_autoreleasePoolPop(self.0) }
    }
}
struct Object(Id);
impl Object {
    fn checked(id: Id, message: &str) -> Result<Self, String> {
        if id.is_null() {
            Err(message.into())
        } else {
            Ok(Self(id))
        }
    }
}
impl Drop for Object {
    fn drop(&mut self) {
        msg!(self.0,"release"=>());
    }
}
struct Cf(Id);
impl Cf {
    fn checked(id: Id, message: &str) -> Result<Self, String> {
        if id.is_null() {
            Err(message.into())
        } else {
            Ok(Self(id))
        }
    }
}
impl Drop for Cf {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}
fn string(text: &str) -> Result<Object, String> {
    let id = msg!(class("NSString"),"alloc"=>Id);
    Object::checked(
        msg!(id,"initWithBytes:length:encoding:"=>Id,text.as_ptr()=>*const u8,text.len()=>usize,4usize=>usize),
        "could not create UTF-8 text",
    )
}
fn text(id: Id) -> String {
    if id.is_null() {
        return String::new();
    }
    let p = msg!(id,"UTF8String"=>*const c_char);
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}
fn count(id: Id) -> usize {
    if id.is_null() {
        0
    } else {
        msg!(id,"count"=>usize)
    }
}
fn at(id: Id, index: usize) -> Id {
    msg!(id,"objectAtIndex:"=>Id,index=>usize)
}
fn clean(value: &str) -> String {
    value.replace(['\t', '\n', '\r'], " ")
}
fn image(path: &str) -> Result<(Object, Id), String> {
    let path = string(path)?;
    let id = msg!(class("NSImage"),"alloc"=>Id);
    let owner = Object::checked(
        msg!(id,"initWithContentsOfFile:"=>Id,path.0=>Id),
        "could not read image",
    )?;
    let cg = msg!(owner.0,"CGImageForProposedRect:context:hints:"=>Id,ptr::null_mut::<Rect>()=>*mut Rect,ptr::null_mut::<c_void>()=>Id,ptr::null_mut::<c_void>()=>Id);
    if cg.is_null() {
        Err("could not read image".into())
    } else {
        Ok((owner, cg))
    }
}
fn rect_message(id: Id, name: &str) -> Rect {
    #[cfg(target_arch = "aarch64")]
    {
        msg!(id,name=>Rect)
    }
    #[cfg(target_arch = "x86_64")]
    {
        let mut rect = Rect::default();
        let f: unsafe extern "C" fn(*mut Rect, Id, Sel) =
            unsafe { std::mem::transmute(objc_msgSend_stret as *const ()) };
        unsafe { f(&mut rect, id, sel(name)) };
        rect
    }
}
pub fn ocr(path: &str, origin_x: f64, origin_y: f64) -> Result<String, String> {
    if !origin_x.is_finite() || !origin_y.is_finite() {
        return Err("invalid image origin".into());
    }
    let _pool = Pool::new();
    let (_image, cg) = image(path)?;
    let request = Object::checked(
        msg!(class("VNRecognizeTextRequest"),"new"=>Id),
        "could not create OCR request",
    )?;
    msg!(request.0,"setRecognitionLevel:"=>(),0isize=>isize);
    msg!(request.0,"setUsesLanguageCorrection:"=>(),false=>bool);
    let language = string("en-US")?;
    let languages = msg!(class("NSArray"),"arrayWithObject:"=>Id,language.0=>Id);
    msg!(request.0,"setRecognitionLanguages:"=>(),languages=>Id);
    let options = msg!(class("NSDictionary"),"dictionary"=>Id);
    let handler = msg!(class("VNImageRequestHandler"),"alloc"=>Id);
    let handler = Object::checked(
        msg!(handler,"initWithCGImage:options:"=>Id,cg=>Id,options=>Id),
        "could not create OCR handler",
    )?;
    let requests = msg!(class("NSArray"),"arrayWithObject:"=>Id,request.0=>Id);
    let mut error: Id = ptr::null_mut();
    if !msg!(handler.0,"performRequests:error:"=>bool,requests=>Id,&mut error=>*mut Id) {
        return Err(if error.is_null() {
            "OCR request failed".into()
        } else {
            text(msg!(error,"localizedDescription"=>Id))
        });
    }
    let results = msg!(request.0,"results"=>Id);
    let width = unsafe { CGImageGetWidth(cg) } as f64;
    let height = unsafe { CGImageGetHeight(cg) } as f64;
    let mut rows: Vec<(Rect, String, f32)> = Vec::new();
    for i in 0..count(results) {
        let result = at(results, i);
        let candidates = msg!(result,"topCandidates:"=>Id,1usize=>usize);
        if count(candidates) == 0 {
            continue;
        }
        let candidate = at(candidates, 0);
        rows.push((
            rect_message(result, "boundingBox"),
            text(msg!(candidate,"string"=>Id)),
            msg!(candidate,"confidence"=>f32),
        ));
    }
    // Match the legacy row grouping, without an inconsistent comparator in sort.
    for i in 1..rows.len() {
        let mut j = i;
        while j > 0 {
            let a = rows[j].0;
            let b = rows[j - 1].0;
            let ay = 1. - a.origin.y - a.size.height;
            let by = 1. - b.origin.y - b.size.height;
            let before = if (ay - by).abs() > 0.01 {
                ay < by
            } else {
                a.origin.x < b.origin.x
            };
            if !before {
                break;
            }
            rows.swap(j, j - 1);
            j -= 1;
        }
    }
    let mut output = String::new();
    for (b, s, confidence) in rows {
        let x = origin_x + (b.origin.x + b.size.width / 2.) * width;
        let y = origin_y + (1. - b.origin.y - b.size.height / 2.) * height;
        output.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{:.3}\n",
            clean(&s),
            x.round() as i64,
            y.round() as i64,
            (b.size.width * width).round() as i64,
            (b.size.height * height).round() as i64,
            confidence
        ));
    }
    Ok(output)
}
pub fn fingerprint(path: &str) -> Result<String, String> {
    let _pool = Pool::new();
    let (_image, cg) = image(path)?;
    let mut pixels = vec![0u8; 64 * 64 * 4];
    let space = Cf::checked(
        unsafe { CGColorSpaceCreateDeviceRGB() },
        "could not create image color space",
    )?;
    let context = Cf::checked(
        unsafe { CGBitmapContextCreate(pixels.as_mut_ptr().cast(), 64, 64, 8, 64 * 4, space.0, 1) },
        "could not create image context",
    )?;
    unsafe {
        CGContextSetInterpolationQuality(context.0, 4);
        CGContextDrawImage(
            context.0,
            Rect {
                origin: Point { x: 0., y: 0. },
                size: Size {
                    width: 64.,
                    height: 64.,
                },
            },
            cg,
        )
    };
    Ok(format!("{:016x}", fingerprint_bytes(&pixels)))
}
fn fingerprint_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(1469598103934665603u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(1099511628211)
    })
}
fn delay(key: &str, default: f64) -> Duration {
    let seconds = std::env::var(key)
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n >= 0. && *n <= 60.)
        .unwrap_or(default);
    Duration::from_secs_f64(seconds)
}
type ClipboardItem = Vec<(String, Vec<u8>)>;
fn backup_clipboard(pasteboard: Id) -> Result<Vec<ClipboardItem>, String> {
    let mut backup = Vec::new();
    let mut bytes = 0usize;
    let items = msg!(pasteboard,"pasteboardItems"=>Id);
    for i in 0..count(items) {
        let item = at(items, i);
        let types = msg!(item,"types"=>Id);
        let mut values = Vec::new();
        for t in 0..count(types) {
            let typ = at(types, t);
            let data = msg!(item,"dataForType:"=>Id,typ=>Id);
            if data.is_null() {
                continue;
            }
            let len = msg!(data,"length"=>usize);
            bytes = bytes.saturating_add(len);
            if bytes > 64 * 1024 * 1024 {
                return Err("clipboard is too large to preserve safely".into());
            }
            let p = msg!(data,"bytes"=>*const u8);
            if len > 0 && p.is_null() {
                return Err("could not preserve clipboard data".into());
            }
            let contents = if len == 0 {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(p, len) }.to_vec()
            };
            values.push((text(typ), contents));
        }
        backup.push(values);
    }
    Ok(backup)
}
struct ClipboardRestore {
    board: Id,
    change: isize,
    backup: Vec<ClipboardItem>,
}
impl Drop for ClipboardRestore {
    fn drop(&mut self) {
        // A user's newer clipboard always wins over our restoration.
        if msg!(self.board,"changeCount"=>isize) != self.change {
            return;
        }
        msg!(self.board,"clearContents"=>isize);
        let items = msg!(class("NSMutableArray"),"array"=>Id);
        for saved in &self.backup {
            let Ok(item) =
                Object::checked(msg!(class("NSPasteboardItem"),"new"=>Id), "clipboard item")
            else {
                continue;
            };
            for (kind, bytes) in saved {
                let Ok(kind) = string(kind) else {
                    continue;
                };
                let data = msg!(class("NSData"),"dataWithBytes:length:"=>Id,bytes.as_ptr()=>*const u8,bytes.len()=>usize);
                msg!(item.0,"setData:forType:"=>bool,data=>Id,kind.0=>Id);
            }
            msg!(items,"addObject:"=>(),item.0=>Id);
        }
        if !self.backup.is_empty() {
            msg!(self.board,"writeObjects:"=>bool,items=>Id);
        }
    }
}
pub fn paste(path: &str) -> Result<(), String> {
    if !unsafe { AXIsProcessTrusted() } {
        return Err("Accessibility permission is unavailable".into());
    }
    let _pool = Pool::new();
    let contents = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let source = Cf::checked(
        unsafe { CGEventSourceCreate(1) },
        "could not create paste events",
    )?;
    let down = Cf::checked(
        unsafe { CGEventCreateKeyboardEvent(source.0, 9, true) },
        "could not create paste events",
    )?;
    let up = Cf::checked(
        unsafe { CGEventCreateKeyboardEvent(source.0, 9, false) },
        "could not create paste events",
    )?;
    let board = msg!(class("NSPasteboard"),"generalPasteboard"=>Id);
    if board.is_null() {
        return Err("clipboard is unavailable".into());
    }
    let backup = backup_clipboard(board)?;
    let value = string(&contents)?;
    let kind = string("public.utf8-plain-text")?;
    msg!(board,"clearContents"=>isize);
    let mut restore = ClipboardRestore {
        board,
        change: msg!(board,"changeCount"=>isize),
        backup,
    };
    let written = msg!(board,"setString:forType:"=>bool,value.0=>Id,kind.0=>Id);
    restore.change = msg!(board,"changeCount"=>isize);
    if !written {
        return Err("could not write text to the clipboard".into());
    }
    thread::sleep(delay("CU_PASTE_READY_DELAY", 0.02));
    if msg!(board,"changeCount"=>isize) != restore.change {
        return Err("clipboard changed before paste; no paste was sent".into());
    }
    unsafe {
        CGEventSetFlags(down.0, 1 << 20);
        CGEventSetFlags(up.0, 1 << 20);
        CGEventPost(0, down.0);
        CGEventPost(0, up.0)
    };
    thread::sleep(delay("CU_PASTE_RESTORE_DELAY", 0.35));
    drop(restore);
    Ok(())
}
pub fn scroll(x: f64, y: f64, delta: i32, count: usize) -> Result<(), String> {
    if !x.is_finite()
        || !y.is_finite()
        || x.abs() > 100000.
        || y.abs() > 100000.
        || count == 0
        || count > 4096
    {
        return Err("invalid scroll arguments".into());
    }
    if !unsafe { AXIsProcessTrusted() } {
        return Err("Accessibility permission is unavailable".into());
    }
    let point = Point { x, y };
    let move_event = Cf::checked(
        unsafe { CGEventCreateMouseEvent(ptr::null_mut(), 5, point, 0) },
        "could not create pointer event",
    )?;
    unsafe { CGEventPost(0, move_event.0) };
    thread::sleep(Duration::from_millis(25));
    for i in 0..count {
        let event = Cf::checked(
            unsafe { CGEventCreateScrollWheelEvent2(ptr::null_mut(), 1, 1, delta, 0, 0) },
            "could not create scroll event",
        )?;
        unsafe {
            CGEventSetLocation(event.0, point);
            CGEventPost(0, event.0)
        };
        if i + 1 < count {
            thread::sleep(Duration::from_millis(35));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleaning_preserves_text() {
        assert_eq!(clean("a\tb\nc\r雪"), "a b c 雪");
    }
    #[test]
    fn stable_fingerprint() {
        assert_eq!(fingerprint_bytes(&[]), 1469598103934665603);
        assert_eq!(fingerprint_bytes(&[0]), 4953163356653287321);
    }
    #[test]
    fn invalid_scroll_rejected_before_permission_or_input() {
        assert!(scroll(f64::NAN, 0., 1, 1).is_err());
        assert!(scroll(0., 0., 1, 0).is_err());
        assert!(scroll(0., 0., 1, 4097).is_err());
    }
    #[test]
    fn image_error_is_recoverable() {
        assert!(fingerprint("/nonexistent-cu-test-image.png").is_err());
        assert!(ocr("/nonexistent-cu-test-image.png", 0., 0.).is_err());
    }
    #[test]
    fn private_clipboard_restores_binary_types_and_preserves_newer_copy() {
        let _pool = Pool::new();
        let board = msg!(class("NSPasteboard"), "pasteboardWithUniqueName" => Id);
        assert!(!board.is_null());
        let data = vec![0, 255, 17, 0];
        let original = vec![vec![("dev.cu.binary-fixture".to_string(), data)]];
        let restore = ClipboardRestore {
            board,
            change: msg!(board, "changeCount" => isize),
            backup: original.clone(),
        };
        drop(restore);
        assert_eq!(backup_clipboard(board).unwrap(), original);
        let restore = ClipboardRestore {
            board,
            change: msg!(board, "changeCount" => isize),
            backup: original,
        };
        msg!(board, "clearContents" => isize);
        let value = string("newer user copy").unwrap();
        let kind = string("public.utf8-plain-text").unwrap();
        assert!(msg!(board, "setString:forType:" => bool, value.0 => Id, kind.0 => Id));
        let newer = backup_clipboard(board).unwrap();
        drop(restore);
        assert_eq!(backup_clipboard(board).unwrap(), newer);
        msg!(board, "releaseGlobally" => ());
    }
}
