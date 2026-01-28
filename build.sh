#!/bin/bash
set -e

echo "Building NusaLaunchd..."
cargo build --release

echo "Creating example configs directory..."
mkdir -p /etc/nusalaunchd/jobs
cp configs/examples/*.toml /etc/nusalaunchd/jobs/ 2>/dev/null || true

echo "Build complete!"
echo ""
echo "To run:"
echo "  sudo ./target/release/nusalaunchd --foreground"
echo ""
echo "To run with custom config:"
echo "  sudo ./target/release/nusalaunchd --config-dir ./configs/examples --foreground"