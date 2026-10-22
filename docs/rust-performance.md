# Rust CLI and MCP performance

CU's Rust CLI and MCP server remove the shell orchestration and Python MCP
interpreter from the runtime path. This reduces command/protocol overhead;
it does not make applications, Accessibility providers, OCR, or network services
instant. Measurements below use mock backends and must not be marketed as real
computer-use completion times.

## Reproduce

```sh
cargo build --locked --release --manifest-path cu-runtime/Cargo.toml
python3 scripts/benchmark-rust.py --samples 30 --warmup 5 \
  --output artifacts/rust-performance.json
```

Run after all builds finish. The script checks that the measured binary hashes
remain unchanged. It makes no desktop changes and requires no GUI permissions.
It does not install packages, contact servers, or run an AI model. It needs Git,
Python 3, and zsh on macOS. `artifacts/` is ignored by Git; retain the JSON report
alongside release evidence when publishing measurements.

The baseline is extracted directly from `git show v0.4.1:bin/cu` and
`git show v0.4.1:scripts/cu-mcp.py`. The newer `tests/reference/` copies differ from
that release and are deliberately not used. The report records baseline source
hashes, release executable hashes and sizes, Python version, OS, architecture,
helper source hashes, individual timings, and response sizes.

## Measurement contract

- Each comparable operation runs five warmups and thirty measured iterations.
  Legacy/Rust order alternates using a fixed pseudorandom seed. Median and
  nearest-rank p95 include subprocess launch, output transport, JSON parsing,
  and result assertions. Fresh MCP initialization also includes server shutdown.
- Each iteration starts with an empty snapshot store and a reset mock button.
  Both implementations use the identical native, input, image, and AppleScript
  helpers. Helpers are intentionally small zsh processes, so their shared
  process-launch overhead remains in both measurements.
- The action benchmark actually transitions the mock button from value `0` to
  `1`. It asserts an `ok` receipt and changed semantic/visual verification; an
  exit code alone does not pass. Coordinate fallback is disabled on both sides.
- Verification delay and configurable click settling are zero on both sides.
  Internal fixed activation/stability waits remain. Foreground verification
  still invokes mocked capture/fingerprint helpers; those do not capture real
  images. The report lists backend helper invocations, **not total OS process
  counts**: it does not trace internal legacy utilities or forks.
- Persistent MCP scenarios reuse one initialized server. Published v0.4.1 does
  not implement `ping`, so ping is reported for Rust alone. `tools/list` returns
  a larger catalog in Rust; response byte counts are included and it is not an
  equal-byte serialization benchmark.
- Native-only background action is another Rust-only scenario. It verifies a
  semantic change and asserts that no mock capture, fingerprint, or pointer
  helper ran. It is a different operation from foreground visual verification,
  so no cross-mode speedup ratio is claimed.
- These are warm-filesystem-cache measurements on a development machine, not
  cold-cache or isolated CPU tests. Background system activity can affect p95.
  RAM, energy, real app latency, and model latency are not measured.

## Results

Measured 22 October 2026 on macOS 27.0, arm64; Python 3.9.6. Thirty samples
per cell after five warmups. All times are milliseconds.

| Operation | v0.4.1 median / p95 | Rust median / p95 |
| --- | ---: | ---: |
| CLI version, fresh process | 13.147 / 17.353 | 6.010 / 6.661 |
| CLI native observation | 44.414 / 51.350 | 21.655 / 27.632 |
| CLI observe + verified foreground action | 285.437 / 324.496 | 244.877 / 270.536 |
| MCP initialize, fresh process | 35.968 / 38.325 | 5.194 / 5.857 |
| MCP tools/list, persistent server | 0.064 / 0.110 | 0.105 / 0.134 |
| MCP observe + verified foreground action | 297.650 / 384.337 | 250.275 / 279.160 |
| MCP ping, persistent server | Not available | 0.021 / 0.032 |
| CLI native-only background observe + verified action | Not available | 51.615 / 53.512 |

The largest measured reduction is fresh MCP process initialization. Native
observation overhead is also lower. Foreground action gains are smaller because
both implementations retain the same verification work and fixed settling waits.
The native-only background path avoids capture and pointer work while checking
the semantic result; use it when the target exposes adequate Accessibility.

Rust `tools/list` is slightly slower here: its response is 5,752 bytes versus
3,007 bytes for v0.4.1. This result is retained rather than hidden or presented
as a speedup. Neither sub-millisecond catalog result predicts desktop latency.

| Local release-profile executable | Bytes |
| --- | ---: |
| `cu` | 896,784 |
| `cu-mcp` | 527,568 |
| `cu-native` | 884,624 |
| `cu-input` | 425,680 |

Sizes are standalone executable file sizes, excluding dynamically linked macOS
frameworks, operating-system caches, and installation assets. They do not
represent RAM usage.

Measured local CLI/MCP SHA-256 (release downloads are built separately):

- `cu`: `face64416cfe84c065236b7cfc4a6ceae7f9d7dae48f52dc8a031c34c246e54d`
- `cu-mcp`: `6d6fc3f9c72c622851d87334dd4d401292b6d9044157e6ca24079880c34a8092`

No assembly rewrite is justified by these measurements. Keep optimizing
measured process boundaries and unnecessary observation/capture calls before
considering instruction-level changes. For production latency claims, separately
measure real applications, permissions, stale-state recovery, and end-to-end
task success; these mock benchmarks establish none of those guarantees.
