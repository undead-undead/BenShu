#!/bin/bash
set -e

# BenShu Full Build Script (Native Windows/Linux Focus)
echo "🦀 Building BenShu Unified (Gateway + Panel)..."

# Build the unified binary (on Windows, this will include the Tray and GPU acceleration)
cargo build -p benshu-panel --release

echo "✨ Build Complete!"
echo "The binary is available at: ./target/release/benshu-panel"
echo "Run it directly to start both the engine and the dashboard."
