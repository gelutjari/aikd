#!/bin/bash
# Interactive setup script for AIKD development environment

set -e

echo "═══════════════════════════════════════════════════════════"
echo "           🔧 AIKD ENVIRONMENT SETUP"
echo "═══════════════════════════════════════════════════════════"
echo ""

# Detect OS
if [[ "$OSTYPE" == "darwin"* ]]; then
    OS="macos"
elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
    OS="linux"
elif [[ "$OSTYPE" == "msys" ]] || [[ "$OSTYPE" == "cygwin" ]]; then
    OS="windows"
else
    OS="unknown"
fi

echo "Detected OS: $OS"
echo ""

# Install required tools
echo "📦 Installing required tools..."
echo ""

install_tool() {
    local tool=$1
    local install_cmd=$2
    
    if command -v "$tool" &> /dev/null; then
        echo "  ✅ $tool already installed"
    else
        echo "  📥 Installing $tool..."
        eval "$install_cmd"
        if command -v "$tool" &> /dev/null; then
            echo "  ✅ $tool installed"
        else
            echo "  ❌ Failed to install $tool"
        fi
    fi
}

# Rust
if ! command -v cargo &> /dev/null; then
    echo "  📥 Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    echo "  ✅ Rust installed"
else
    echo "  ✅ Rust already installed"
fi

# GitHub CLI
if ! command -v gh &> /dev/null; then
    echo "  📥 Installing GitHub CLI..."
    if [[ "$OS" == "macos" ]]; then
        brew install gh
    elif [[ "$OS" == "linux" ]]; then
        curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg | sudo dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg
        echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" | sudo tee /etc/apt/sources.list.d/github-cli.list > /dev/null
        sudo apt update && sudo apt install gh
    fi
    echo "  ✅ GitHub CLI installed"
else
    echo "  ✅ GitHub CLI already installed"
fi

# jq
if ! command -v jq &> /dev/null; then
    echo "  📥 Installing jq..."
    if [[ "$OS" == "macos" ]]; then
        brew install jq
    elif [[ "$OS" == "linux" ]]; then
        sudo apt-get install -y jq
    fi
    echo "  ✅ jq installed"
else
    echo "  ✅ jq already installed"
fi

# ImageMagick (optional)
if ! command -v convert &> /dev/null; then
    echo "  📥 Installing ImageMagick (optional)..."
    if [[ "$OS" == "macos" ]]; then
        brew install imagemagick
    elif [[ "$OS" == "linux" ]]; then
        sudo apt-get install -y imagemagick
    fi
    echo "  ✅ ImageMagick installed"
else
    echo "  ✅ ImageMagick already installed"
fi

# VHS (optional)
if ! command -v vhs &> /dev/null; then
    echo "  📥 Installing VHS (optional)..."
    if [[ "$OS" == "macos" ]]; then
        brew install vhs
    elif [[ "$OS" == "linux" ]]; then
        go install github.com/charmbracelet/vhs@latest
    fi
    echo "  ✅ VHS installed"
else
    echo "  ✅ VHS already installed"
fi

echo ""
echo "═══════════════════════════════════════════════════════════"
echo "           🔑 API KEYS SETUP"
echo "═══════════════════════════════════════════════════════════"
echo ""

# Create .env.example
cat > .env.example << 'EOF'
# AIKD Environment Variables
# Copy this file to .env and fill in your values

# FAL.ai API key (for logo generation)
# Get it at: https://fal.ai/dashboard/keys
FAL_KEY=

# crates.io API token (for publishing)
# Get it at: https://crates.io/settings/tokens
CARGO_REGISTRY_TOKEN=

# Discord bot token (for community server)
# Create at: https://discord.com/developers/applications
DISCORD_BOT_TOKEN=

# GitHub authentication
# Run: gh auth login
# GITHUB_TOKEN will be set automatically
EOF

echo "Created .env.example with required variables"
echo ""

echo "📝 Next steps:"
echo ""
echo "1. Get FAL.ai API key (for logo generation):"
echo "   https://fal.ai/dashboard/keys"
echo ""
echo "2. Get crates.io API token (for publishing):"
echo "   https://crates.io/settings/tokens"
echo ""
echo "3. Create Discord bot (for community server):"
echo "   https://discord.com/developers/applications"
echo ""
echo "4. Authenticate with GitHub:"
echo "   gh auth login"
echo ""
echo "5. Copy .env.example to .env and fill in values:"
echo "   cp .env.example .env"
echo "   # Edit .env with your values"
echo ""
echo "6. Run the auto-pilot:"
echo "   ./scripts/full-auto.sh"
echo ""
