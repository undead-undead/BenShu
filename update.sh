#!/bin/bash
set -e

# BenShu Update Script (Linux/Native)
# Runs git pull and triggers a rebuild in the background.

GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}[BenShu]${NC} Starting system update..."

# 1. Pull latest code
if [ -d ".git" ]; then
    echo -e "${BLUE}[BenShu]${NC} Fetching latest changes from git..."
    git fetch origin
    git reset --hard origin/main
else
    echo -e "${YELLOW}[WARN]${NC} Not a git repository. Skipping git pull."
fi

# 2. Rebuild
echo -e "${BLUE}[BenShu]${NC} Rebuilding BenShu (this may take a while)..."
# We run cargo build in the background or just return and let the gateway handle it?
# The gateway will call this and wait for it to finish.
cargo build -p benshu-gateway --release

echo -e "${GREEN}[OK]${NC} Update complete! Please restart the gateway to apply changes."
