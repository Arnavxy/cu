mod cli_input;
mod cli_media;
mod cli_misc;
mod cli_pointer;
mod commands;
// All command dispatch and execution is implemented in Rust.
fn requires_input_lease(args: &[String]) -> bool {
    let args = &args[args.iter().take_while(|s| s.as_str() == "--json").count()..];
    let Some(command) = args.first().map(String::as_str) else {
        return false;
    };
    match command {
        "type" | "paste" | "key" | "combo" | "move" | "drag" | "click" | "dclick" | "rclick"
        | "clickel" | "clicktext" | "scroll" | "act" | "open" | "select-file" => true,
        "batch" => !args.iter().any(|s| s == "--help" || s == "-h"),
        "observe" => !args.iter().any(|s| s == "--background"),
        "tree" | "find" => !args.iter().any(|s| s == "--native-only"),
        "shot" => args.iter().any(|s| s == "--window" || s == "-w"),
        _ => false,
    }
}
fn dispatch(args: &[String]) -> (i32, String) {
    match cli_input::dispatch(args)
        .or_else(|| cli_media::dispatch(args))
        .or_else(|| cli_pointer::dispatch(args))
        .or_else(|| cli_misc::dispatch(args))
        .or_else(|| commands::dispatch(args))
    {
        Some(Ok(reply)) => reply,
        Some(Err(error)) => (1, serde_json::json!({"ok":false,"error":error}).to_string()),
        None => (
            2,
            serde_json::json!({"ok":false,"error":"Unknown command; run cu help"}).to_string(),
        ),
    }
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let lease = if requires_input_lease(&args) {
        cu_tools::input_lease::acquire().map(Some)
    } else {
        cu_tools::input_lease::acquire_if_delegated()
    };
    let result = lease.and_then(|lease| match lease {
        Some(lease) => cu_tools::input_lease::with_current_lease(lease, || dispatch(&args)),
        None => Ok(dispatch(&args)),
    });
    let (code,output)=result.unwrap_or_else(|error|(1,serde_json::json!({"ok":false,"error":{"code":error.code(),"message":"Another native operation is active, or its private input lease is unavailable. No input was dispatched."}}).to_string()));
    // All leases and helper-delegation scopes have dropped before process exit.
    println!("{output}");
    std::process::exit(code);
}
#[cfg(test)]
mod lease_classification_tests {
    use super::*;
    fn needs(args: &[&str]) -> bool {
        requires_input_lease(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }
    #[test]
    fn leases_mutations_and_foreground_fallbacks_but_not_pure_reads() {
        for command in [
            "type",
            "paste",
            "key",
            "combo",
            "move",
            "drag",
            "click",
            "dclick",
            "rclick",
            "clickel",
            "clicktext",
            "scroll",
            "act",
            "open",
            "select-file",
            "batch",
        ] {
            assert!(needs(&["--json", command]), "{command}");
        }
        for command in [
            "apps", "document", "read", "color", "pos", "windows", "bounds", "audit", "help",
            "version", "unknown",
        ] {
            assert!(!needs(&[command]), "{command}");
        }
        assert!(needs(&["observe", "Notes"]));
        assert!(!needs(&["observe", "Notes", "--background"]));
        assert!(needs(&["find", "Notes", "hello"]));
        assert!(!needs(&["find", "Notes", "hello", "--native-only"]));
        assert!(needs(&["shot", "--window", "Notes"]));
        assert!(!needs(&["shot"]));
        assert!(!needs(&["batch", "--help"]));
    }
}
