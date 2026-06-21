#!/bin/bash
# Record demo GIF using VHS
# Installs VHS if not present

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TAPE_FILE="$SCRIPT_DIR/demo.tape"

# Check if VHS is installed
if ! command -v vhs &> /dev/null; then
    echo "📦 Installing VHS..."
    
    if [[ "$OSTYPE" == "darwin"* ]]; then
        brew install vhs
    elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
        go install github.com/charmbracelet/vhs@latest
    else
        echo "❌ Please install VHS manually: https://github.com/charmbracelet/vhs"
        exit 1
    fi
fi

echo "🎬 Recording demo GIF..."
vhs "$TAPE_FILE"

echo "✅ Demo GIF saved to docs/demo.gif"
