# 🚀 NusaLaunchd - A Launchd-style Init System for Linux

NusaLaunchd is a modern, lightweight init system for Linux inspired by macOS launchd, built in Rust. It provides socket activation, dependency management, and process supervision with a clean TOML-based configuration format.


## Quick Start

1. Build: `cargo build --release`
2. Run: `sudo ./target/release/nusalaunchd --config-dir ./configs/examples`
3. Control: Use `nusaload` tool (coming soon)

## Example Config

See `configs/examples/simple.toml` for a basic job configuration.
