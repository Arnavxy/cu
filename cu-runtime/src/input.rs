//! Bounded native macOS input. Parsing completes before any event is posted.
#[path = "../../shared/input_lease.rs"]
#[allow(dead_code)]
mod input_lease;
use std::{collections::BTreeMap, ffi::c_void, thread, time::Duration};
use unicode_segmentation::UnicodeSegmentation;
type Result<T> = std::result::Result<T, &'static str>;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Point {
    x: f64,
    y: f64,
}
#[repr(C)]
struct Size {
    width: f64,
    height: f64,
}
#[repr(C)]
struct Rect {
    origin: Point,
    size: Size,
}
#[derive(Debug, PartialEq)]
enum Action {
    Move(Point),
    Click(Point, u8, bool),
    Drag(Point, u8),
    Text(String),
    Key(u16),
    Modifier(u16, u64, bool),
    Wait(u32),
    Position,
    Color(Point),
}
fn key_code(s: &str) -> Option<u16> {
    let values: &[(&str, u16)] = &[
        ("a", 0),
        ("s", 1),
        ("d", 2),
        ("f", 3),
        ("h", 4),
        ("g", 5),
        ("z", 6),
        ("x", 7),
        ("c", 8),
        ("v", 9),
        ("b", 11),
        ("q", 12),
        ("w", 13),
        ("e", 14),
        ("r", 15),
        ("y", 16),
        ("t", 17),
        ("1", 18),
        ("2", 19),
        ("3", 20),
        ("4", 21),
        ("6", 22),
        ("5", 23),
        ("=", 24),
        ("9", 25),
        ("7", 26),
        ("-", 27),
        ("8", 28),
        ("0", 29),
        ("]", 30),
        ("o", 31),
        ("u", 32),
        ("[", 33),
        ("i", 34),
        ("p", 35),
        ("return", 36),
        ("enter", 36),
        ("l", 37),
        ("j", 38),
        ("'", 39),
        ("k", 40),
        (";", 41),
        ("\\", 42),
        (",", 43),
        ("/", 44),
        ("n", 45),
        ("m", 46),
        (".", 47),
        ("tab", 48),
        ("space", 49),
        ("`", 50),
        ("delete", 51),
        ("backspace", 51),
        ("esc", 53),
        ("escape", 53),
        ("f1", 122),
        ("f2", 120),
        ("f3", 99),
        ("f4", 118),
        ("f5", 96),
        ("f6", 97),
        ("f7", 98),
        ("f8", 100),
        ("f9", 101),
        ("f10", 109),
        ("f11", 103),
        ("f12", 111),
        ("home", 115),
        ("end", 119),
        ("page-up", 116),
        ("page-down", 121),
        ("forward-delete", 117),
        ("arrow-left", 123),
        ("arrow-right", 124),
        ("arrow-down", 125),
        ("arrow-up", 126),
    ];
    values
        .iter()
        .find(|(name, _)| *name == s)
        .map(|(_, code)| *code)
}
fn modifier(s: &str) -> Option<(u16, u64)> {
    Some(match s {
        "cmd" | "command" => (55, 1 << 20),
        "shift" => (56, 1 << 17),
        "alt" | "option" => (58, 1 << 19),
        "ctrl" | "control" => (59, 1 << 18),
        "fn" => (63, 1 << 23),
        _ => return None,
    })
}
fn point(s: &str) -> Result<Point> {
    let (x, y) = s.split_once(',').ok_or("Invalid logical point")?;
    let x = x.parse::<f64>().map_err(|_| "Invalid logical point")?;
    let y = y.parse::<f64>().map_err(|_| "Invalid logical point")?;
    if !x.is_finite() || !y.is_finite() || x.abs() > 100000. || y.abs() > 100000. {
        return Err("Invalid logical point");
    }
    Ok(Point { x, y })
}
fn parse(args: &[String]) -> Result<Vec<Action>> {
    if args.is_empty() || args.len() > 4096 {
        return Err("Use 1 to 4096 input operations");
    }
    let mut actions = Vec::with_capacity(args.len());
    let mut waits = 0u64;
    for arg in args {
        if arg == "p" {
            actions.push(Action::Position);
            continue;
        }
        let (op, v) = arg.split_once(':').ok_or("Malformed input operation")?;
        actions.push(match op {
            "m" => Action::Move(point(v)?),
            "c" | "dc" | "tc" | "rc" => Action::Click(
                point(v)?,
                if op == "dc" {
                    2
                } else if op == "tc" {
                    3
                } else {
                    1
                },
                op == "rc",
            ),
            "dd" | "dm" | "du" => Action::Drag(
                point(v)?,
                if op == "dd" {
                    0
                } else if op == "dm" {
                    1
                } else {
                    2
                },
            ),
            "t" => {
                if v.encode_utf16().count() > 16384 {
                    return Err("Text is too long");
                }
                Action::Text(v.into())
            }
            "kp" => Action::Key(key_code(&v.to_lowercase()).ok_or("Unsupported key")?),
            "kd" | "ku" => {
                let (code, flag) = modifier(&v.to_lowercase()).ok_or("Unsupported modifier")?;
                Action::Modifier(code, flag, op == "kd")
            }
            "w" => {
                let ms = v.parse::<u32>().map_err(|_| "Wait exceeds 10000ms")?;
                if ms > 10000 {
                    return Err("Wait exceeds 10000ms");
                }
                waits += u64::from(ms);
                if waits > 60000 {
                    return Err("Total waits exceed 60 seconds");
                }
                Action::Wait(ms)
            }
            "cp" => Action::Color(point(v)?),
            _ => return Err("Unsupported input operation"),
        });
    }
    Ok(actions)
}
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
    fn CGEventSourceCreate(state: i32) -> *mut c_void;
    fn CGEventCreate(source: *const c_void) -> *mut c_void;
    fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> *mut c_void;
    fn CGEventKeyboardSetUnicodeString(event: *mut c_void, len: usize, text: *const u16);
    fn CGEventCreateMouseEvent(
        source: *const c_void,
        kind: u32,
        point: Point,
        button: u32,
    ) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventSetIntegerValueField(event: *mut c_void, field: u32, value: i64);
    fn CGEventPost(tap: u32, event: *const c_void);
    fn CGEventGetLocation(event: *const c_void) -> Point;
    fn CGWindowListCreateImage(
        rect: Rect,
        options: u32,
        window: u32,
        image_options: u32,
    ) -> *mut c_void;
    fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits: usize,
        row: usize,
        space: *const c_void,
        info: u32,
    ) -> *mut c_void;
    fn CGContextDrawImage(context: *mut c_void, rect: Rect, image: *const c_void);
    fn CGImageGetWidth(image: *const c_void) -> usize;
    fn CGImageGetHeight(image: *const c_void) -> usize;
}
struct Owned(*mut c_void);
impl Owned {
    fn checked(p: *mut c_void, error: &'static str) -> Result<Self> {
        if p.is_null() { Err(error) } else { Ok(Self(p)) }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) }
        }
    }
}
struct Input {
    source: Owned,
    flags: u64,
    held: BTreeMap<u16, u64>,
}
impl Input {
    fn key(&self, code: u16, down: bool, text: Option<&str>) -> Result<()> {
        let event = Owned::checked(
            unsafe { CGEventCreateKeyboardEvent(self.source.0, code, down) },
            "Cannot create keyboard event",
        )?;
        unsafe {
            CGEventSetFlags(event.0, self.flags);
        }
        if let Some(text) = text {
            let chars: Vec<u16> = text.encode_utf16().collect();
            if !chars.is_empty() {
                unsafe { CGEventKeyboardSetUnicodeString(event.0, chars.len(), chars.as_ptr()) }
            }
        }
        unsafe { CGEventPost(0, event.0) };
        Ok(())
    }
    fn mouse(&self, kind: u32, point: Point, button: u32, clicks: i64) -> Result<()> {
        let event = Owned::checked(
            unsafe { CGEventCreateMouseEvent(self.source.0, kind, point, button) },
            "Cannot create mouse event",
        )?;
        unsafe {
            CGEventSetFlags(event.0, self.flags);
            CGEventSetIntegerValueField(event.0, 1, clicks);
            CGEventPost(0, event.0)
        };
        Ok(())
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        let held = std::mem::take(&mut self.held);
        for (code, flag) in held {
            self.flags &= !flag;
            let _ = self.key(code, false, None);
        }
    }
}
fn color(point: Point) -> Result<[u8; 3]> {
    let error = "Screen Recording permission is required for pixel reads";
    let image = Owned::checked(
        unsafe {
            CGWindowListCreateImage(
                Rect {
                    origin: point,
                    size: Size {
                        width: 1.,
                        height: 1.,
                    },
                },
                1,
                0,
                8,
            )
        },
        error,
    )?;
    // Preserve the first physical pixel when Retina returns a larger image.
    let width = unsafe { CGImageGetWidth(image.0) };
    let height = unsafe { CGImageGetHeight(image.0) };
    if width == 0 || height == 0 || width > 64 || height > 64 {
        return Err(error);
    }
    let mut bytes = vec![0u8; width * height * 4];
    let space = Owned::checked(unsafe { CGColorSpaceCreateDeviceRGB() }, error)?;
    let bitmap = Owned::checked(
        unsafe {
            CGBitmapContextCreate(
                bytes.as_mut_ptr().cast(),
                width,
                height,
                8,
                width * 4,
                space.0,
                0x4001,
            )
        },
        error,
    )?;
    unsafe {
        CGContextDrawImage(
            bitmap.0,
            Rect {
                origin: Point { x: 0., y: 0. },
                size: Size {
                    width: width as f64,
                    height: height as f64,
                },
            },
            image.0,
        )
    };
    // RGBA premultiplied-last, explicit big-endian byte order.
    let alpha = u32::from(bytes[3]);
    Ok(std::array::from_fn(|i| {
        (u32::from(bytes[i]) * 255)
            .checked_div(alpha)
            .unwrap_or(0)
            .min(255) as u8
    }))
}
fn run(actions: &[Action]) -> Result<()> {
    let mutating = actions
        .iter()
        .any(|a| !matches!(a, Action::Position | Action::Color(_) | Action::Wait(_)));
    // Retain one lease across every event and held modifier. Declared before
    // Input so its Drop releases held keys before this lease closes.
    let _lease = if mutating {
        input_lease::acquire().map(Some)
    } else {
        input_lease::acquire_if_delegated()
    }
    .map_err(|e| e.code())?;
    if mutating && !unsafe { AXIsProcessTrusted() } {
        return Err("Accessibility permission is required for CU input");
    }
    let mut input = Input {
        source: Owned(unsafe { CGEventSourceCreate(1) }),
        flags: 0,
        held: BTreeMap::new(),
    };
    for action in actions {
        match action {
            Action::Move(p) => input.mouse(5, *p, 0, 1)?,
            Action::Click(p, count, right) => {
                for click in 1..=*count {
                    input.mouse(
                        if *right { 3 } else { 1 },
                        *p,
                        u32::from(*right),
                        i64::from(click),
                    )?;
                    // Give the receiving AppKit control a chance to enter its
                    // mouse-tracking loop before delivering the release.
                    thread::sleep(Duration::from_millis(10));
                    input.mouse(
                        if *right { 4 } else { 2 },
                        *p,
                        u32::from(*right),
                        i64::from(click),
                    )?;
                    if click < *count {
                        thread::sleep(Duration::from_millis(40));
                    }
                }
            }
            Action::Drag(p, state) => input.mouse(
                match state {
                    0 => 1,
                    1 => 6,
                    _ => 2,
                },
                *p,
                0,
                1,
            )?,
            Action::Text(text) => {
                for character in text.graphemes(true) {
                    let shortcut = if input.flags & ((1 << 20) | (1 << 18)) != 0 {
                        key_code(&character.to_lowercase())
                    } else {
                        None
                    };
                    let code = shortcut.unwrap_or(0);
                    let unicode = if shortcut.is_some() {
                        None
                    } else {
                        Some(character)
                    };
                    input.key(code, true, unicode)?;
                    input.key(code, false, unicode)?;
                }
            }
            Action::Key(code) => {
                input.key(*code, true, None)?;
                input.key(*code, false, None)?;
            }
            Action::Modifier(code, flag, down) => {
                if *down {
                    input.flags |= flag;
                    input.held.insert(*code, *flag);
                } else {
                    input.flags &= !flag;
                    input.held.remove(code);
                }
                input.key(*code, *down, None)?;
            }
            Action::Wait(ms) => thread::sleep(Duration::from_millis(u64::from(*ms))),
            Action::Position => {
                let event = Owned::checked(
                    unsafe { CGEventCreate(std::ptr::null()) },
                    "Cannot read pointer",
                )?;
                let p = unsafe { CGEventGetLocation(event.0) };
                println!("{},{}", p.x as i64, p.y as i64);
            }
            Action::Color(p) => {
                let rgb = color(*p)?;
                println!("{} {} {}", rgb[0], rgb[1], rgb[2]);
            }
        }
    }
    Ok(())
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["-V"] || args == ["--version"] {
        println!("cu-input {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let validation = args.first().is_some_and(|s| s == "--validate");
    let result = parse(if validation { &args[1..] } else { &args }).and_then(|a| {
        if validation {
            println!("{{\"ok\":true,\"operations\":{}}}", a.len());
            Ok(())
        } else {
            run(&a)
        }
    });
    if let Err(error) = result {
        eprintln!("cu-input: {error}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn p(args: &[&str]) -> Result<Vec<Action>> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }
    #[test]
    fn grammar() {
        assert_eq!(
            p(&[
                "p",
                "m:-1.5,2",
                "c:0,0",
                "dc:1,2",
                "tc:3,4",
                "rc:5,6",
                "dd:1,2",
                "dm:2,3",
                "du:3,4",
                "t:a:b",
                "kp:RETURN",
                "kd:CMD",
                "ku:cmd",
                "w:0",
                "cp:2,3"
            ])
            .unwrap()
            .len(),
            15
        );
    }
    #[test]
    fn bounded_points() {
        for v in [
            "NaN,0",
            "inf,0",
            "100001,0",
            "0,-100001",
            "1,2,3",
            ",1",
            "0,",
        ] {
            assert!(point(v).is_err(), "{v}");
        }
        assert!(point("-100000,100000").is_ok());
    }
    #[test]
    fn waits_and_limits() {
        assert!(p(&[]).is_err());
        assert!(p(&["w:10001"]).is_err());
        assert!(p(&["w:10000"; 7]).is_err());
        assert!(p(&["w:10000"; 6]).is_ok());
        assert!(parse(&vec!["p".into(); 4097]).is_err());
        assert!(p(&["kp:unknown"]).is_err());
        assert!(p(&["kd:a"]).is_err());
        assert!(p(&["unknown:x"]).is_err());
    }
    #[test]
    fn utf16_limit_and_graphemes() {
        assert!(parse(&[format!("t:{}", "😀".repeat(8192))]).is_ok());
        assert!(parse(&[format!("t:{}", "😀".repeat(8193))]).is_err());
        assert_eq!("👨‍👩‍👧‍👦e\u{301}".graphemes(true).count(), 2);
    }
    #[test]
    fn all_validated_before_execution() {
        assert!(p(&["c:0,0", "bad"]).is_err());
    }
}
