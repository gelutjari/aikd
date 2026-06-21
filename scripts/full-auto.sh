#!/bin/bash
# Full auto-pilot orchestrator for AIKD
# Runs all automation scripts in correct order

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$SCRIPT_DIR/.."
LOG_FILE="$SCRIPT_DIR/auto-pilot.log"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log() {
    echo -e "${BLUE}[$(date +'%Y-%m-%d %H:%M:%S')]${NC} $1" | tee -a "$LOG_FILE"
}

success() {
    echo -e "${GREEN}✅ $1${NC}" | tee -a "$LOG_FILE"
}

warn() {
    echo -e "${YELLOW}⚠️  $1${NC}" | tee -a "$LOG_FILE"
}

error() {
    echo -e "${RED}❌ $1${NC}" | tee -a "$LOG_FILE"
}

# Clear log
> "$LOG_FILE"

echo "═══════════════════════════════════════════════════════════"
echo "           🚀 AIKD FULL AUTO-PILOT"
echo "═══════════════════════════════════════════════════════════"
echo ""

# Check required tools
log "Checking required tools..."

TOOLS=("git" "cargo" "curl" "jq")
MISSING=()

for tool in "${TOOLS[@]}"; do
    if ! command -v "$tool" &> /dev/null; then
        MISSING+=("$tool")
    fi
done

if [ ${#MISSING[@]} -gt 0 ]; then
    error "Missing tools: ${MISSING[*]}"
    echo "Install them and try again."
    exit 1
fi

success "All required tools installed"

# Check optional tools
OPTIONAL_TOOLS=("gh" "vhs" "convert")
OPTIONAL_MISSING=()

for tool in "${OPTIONAL_TOOLS[@]}"; do
    if ! command -v "$tool" &> /dev/null; then
        OPTIONAL_MISSING+=("$tool")
    fi
done

if [ ${#OPTIONAL_MISSING[@]} -gt 0 ]; then
    warn "Optional tools missing: ${OPTIONAL_MISSING[*]}"
    echo "Some features may be skipped."
fi

# Check environment variables
log "Checking environment variables..."

if [ -z "$FAL_KEY" ]; then
    warn "FAL_KEY not set — logo generation will be skipped"
fi

if [ -z "$DISCORD_BOT_TOKEN" ]; then
    warn "DISCORD_BOT_TOKEN not set — Discord creation will be skipped"
fi

if [ -z "$CARGO_REGISTRY_TOKEN" ]; then
    warn "CARGO_REGISTRY_TOKEN not set — crates.io publish will be skipped"
fi

# Phase 1: Code quality checks
echo ""
log "Phase 1: Running code quality checks..."
cd "$PROJECT_DIR"

log "Running cargo fmt..."
if cargo fmt --all -- --check; then
    success "cargo fmt passed"
else
    warn "cargo fmt found issues, fixing..."
    cargo fmt --all
    success "cargo fmt fixed"
fi

log "Running cargo clippy..."
if cargo clippy --all-targets -- -D warnings; then
    success "cargo clippy passed"
else
    error "cargo clippy failed"
    exit 1
fi

log "Running cargo test..."
if cargo test --all --exclude aikd-benchmark; then
    success "cargo test passed"
else
    error "cargo test failed"
    exit 1
fi

# Phase 2: Generate assets
echo ""
log "Phase 2: Generating assets..."

if [ -n "$FAL_KEY" ] && command -v curl &> /dev/null; then
    log "Generating logo..."
    if bash "$SCRIPT_DIR/generate-logo.sh"; then
        success "Logo generated"
    else
        warn "Logo generation failed"
    fi
else
    warn "Skipping logo generation (FAL_KEY not set)"
fi

if command -v vhs &> /dev/null; then
    log "Recording demo GIF..."
    if bash "$SCRIPT_DIR/record-demo.sh"; then
        success "Demo GIF recorded"
    else
        warn "Demo GIF recording failed"
    fi
else
    warn "Skipping demo GIF (VHS not installed)"
fi

# Phase 3: Git operations
echo ""
log "Phase 3: Git operations..."

cd "$PROJECT_DIR"

log "Committing changes..."
git add -A
git diff --cached --quiet || {
    git commit -m "chore: automated updates via auto-pilot"
    success "Changes committed"
}

log "Pushing to remote..."
git push origin master
success "Pushed to remote"

# Phase 4: Create release
echo ""
log "Phase 4: Creating release..."

if command -v gh &> /dev/null; then
    read -p "Create release tag v2.0.0? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        git tag v2.0.0
        git push origin v2.0.0
        success "Release tag v2.0.0 created"
    fi
else
    warn "Skipping release (gh not installed)"
fi

# Phase 5: Discord server
echo ""
log "Phase 5: Discord server..."

if [ -n "$DISCORD_BOT_TOKEN" ]; then
    read -p "Create Discord server? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        if bash "$SCRIPT_DIR/create-discord.sh"; then
            success "Discord server created"
        else
            warn "Discord creation failed"
        fi
    fi
else
    warn "Skipping Discord (DISCORD_BOT_TOKEN not set)"
fi

# Phase 6: GitHub configuration
echo ""
log "Phase 6: GitHub configuration..."

if command -v gh &> /dev/null; then
    log "Setting repository description..."
    gh repo edit gelutjari/aikd \
        --description "The Ultra-Fast Local Memory Layer for AI Coding Agents" \
        --homepage "https://github.com/gelutjari/aikd" \
        --topics "rust,ai,mcp,search,codebase,memory" 2>/dev/null || true
    success "Repository configured"
else
    warn "Skipping GitHub config (gh not installed)"
fi

# Summary
echo ""
echo "═══════════════════════════════════════════════════════════"
echo "           ✅ AUTO-PILOT COMPLETE"
echo "═══════════════════════════════════════════════════════════"
echo ""
echo "📊 Summary:"
echo "  • Code quality: ✓"
echo "  • Tests: ✓"
echo "  • Git: ✓"
echo ""
echo "🔗 Links:"
echo "  • Repository: https://github.com/gelutjari/aikd"
echo "  • Actions: https://github.com/gelutjari/aikd/actions"

if [ -f "$DOCS_DIR/discord-invite.txt" ]; then
    echo "  • Discord: $(cat "$DOCS_DIR/discord-invite.txt")"
fi

echo ""
echo "📝 Next steps:"
echo "  1. Monitor CI: gh run watch"
echo "  2. Check release: https://github.com/gelutjari/aikd/releases"
echo "  3. Verify crates.io: https://crates.io/crates/aikd"
echo ""
