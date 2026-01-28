# NusaLaunchd

A launchd-style init system for Linux written in Rust.

## Status: Development (Week 1-2 - Foundation)

**Current Features:**
- ✅ TOML-based job configuration
- ✅ Job loading and validation
- ✅ Basic process spawning
- ✅ Simple supervision (auto-restart)

**Planned Features:**
- Socket activation
- Dependency resolution
- Cgroups integration
- Namespace sandboxing

## Quick Start

1. Build: `cargo build --release`
2. Run: `sudo ./target/release/nusalaunchd --config-dir ./configs/examples`
3. Control: Use `nusaload` tool (coming soon)

## Example Config

See `configs/examples/simple.toml` for a basic job configuration.
