# Contributing

CU is a macOS agent-facing CLI and MCP runtime. The production implementation
lives in `cu-runtime/` (Rust); `shared/` contains reusable native guards. Legacy
implementations in `tests/reference/` are test/benchmark oracles, not a runtime.

Use macOS 13 or newer, Xcode Command Line Tools and Rust 1.88 or newer.

Before submitting changes:

```sh
cargo fmt --manifest-path cu-runtime/Cargo.toml -- --check
cargo test --locked --manifest-path cu-runtime/Cargo.toml
cargo clippy --locked --manifest-path cu-runtime/Cargo.toml --all-targets -- -D warnings
cargo build --locked --release --manifest-path cu-runtime/Cargo.toml
./tests/test_cu.zsh
./tests/test_cu_rust_native.zsh
python3 -m unittest discover -s tests -p 'test_cu*.py'
```

Python and historical Swift sources are for development/reference tests only.
End users of the release binaries do not need those runtimes. Keep real desktop
mutations out of automated tests: use isolated temporary state and the documented
`CU_NATIVE`, `CU_OSASCRIPT`, `CU_SCREENCAPTURE`, `CU_SIPS`, and explicit `CLICLICK`
fixture overrides. Never test sending or deleting against real user data.

Preserve CLI arguments, JSON error/result contracts, snapshot scoping and MCP
stdio framing. Test malformed input, stale targets, disabled controls, partial
observations and bounded output. Input changes must retain focus checks,
cooperative leases and modifier/button cleanup. An accepted action is not proof
that the application completed it.

Do not commit personal screenshots, Accessibility dumps, application logs,
credentials, target directories or local release artifacts. Performance claims
must identify hardware, sample count, backend and whether actual GUI latency
was measured. See `scripts/benchmark-rust.py` and `docs/rust-performance.md`.
