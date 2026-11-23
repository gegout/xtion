# Repository Guidelines

## Project Structure & Module Organization

`xtion` is a native Rust PS1080 USB library and CLI. `src/usb.rs` owns USB access, `protocol.rs` handles wire formats, `depth.rs` reconstructs frames, `frame.rs` defines public data, and `device.rs` manages acquisition. Keep higher-level applications outside these layers.

Offline integration tests live in `tests/`, runnable hardware examples in `examples/`, and research, architecture and validation evidence in `docs/`. See `README.md` for setup and usage. Do not store application code in environment metadata directories.

## Build, Test, and Development Commands

- `cargo build --release --locked`: build the library and CLI with locked dependencies.
- `cargo fmt --all --check`: check Rust formatting.
- `cargo clippy --all-targets -- -D warnings`: lint library, CLI, tests and examples.
- `cargo test`: run offline tests.
- `cargo run -- probe`: discover candidate sensors without activating depth.

Rust and a C compiler are required for vendored libusb. Keep `Cargo.lock` current.

## Coding Style & Naming Conventions

Use Rust 2024, rustfmt with four-space indentation, and Clippy. Formatting configuration is in `rustfmt.toml`; lints are in `Cargo.toml`. Unsafe Rust is forbidden in this crate.

Choose descriptive module and function names. Keep modules focused, use consistent filename casing, and avoid introducing unrelated formatting changes in functional patches.

## Testing Guidelines

Use standard Rust `#[test]` unit tests and integration tests. Name tests after verified behavior. No coverage percentage is mandated. Keep automated tests independent of connected hardware; use synthetic fixtures and clearly label them.

Include tests for new behavior and regression tests for bug fixes. Cover relevant failure cases, and keep tests deterministic and independent of local credentials.

## Commit & Pull Request Guidelines

Git history is unavailable in this workspace, so no existing commit convention can be verified. Use concise, imperative commit subjects, such as `Add initial project scaffold`.

Pull requests should explain the change, link related issues when available, and report validation commands and results. Include screenshots for visible interface changes. State explicitly when checks cannot run.

## Security & Configuration

Never commit secrets or local credentials. Do not install vendor camera runtimes or flash firmware. USB port or target-device resets require explicit user authorization; the current troubleshooting session has that authorization. Protect unrelated devices and never reset the shared host controller as a routine recovery step. Only send source-confirmed commands to candidate sensors. Record hardware observations separately from protocol assumptions; never claim synthetic frames as sensor measurements.
