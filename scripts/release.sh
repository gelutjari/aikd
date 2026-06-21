#!/bin/bash
# Release script for AIKD
# Creates tag and pushes to trigger release workflow

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$SCRIPT_DIR/.."

cd "$PROJECT_DIR"

# Get version from Cargo.toml
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

echo "🚀 Preparing release v${VERSION}"
echo ""

# Pre-flight checks
echo "Running pre-flight checks..."

echo "  Checking git status..."
if [ -n "$(git status --porcelain)" ]; then
    echo "❌ Working directory not clean. Commit changes first."
    exit 1
fi

echo "  Running tests..."
if ! cargo test --all --exclude aikd-benchmark; then
    echo "❌ Tests failed"
    exit 1
fi

echo "  Running clippy..."
if ! cargo clippy --all-targets -- -D warnings; then
    echo "❌ Clippy failed"
    exit 1
fi

echo ""
echo "✅ All pre-flight checks passed"
echo ""

# Confirm
read -p "Create release v${VERSION}? (y/N) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Cancelled"
    exit 0
fi

# Create tag
echo "Creating tag v${VERSION}..."
git tag -a "v${VERSION}" -m "Release v${VERSION}"
git push origin "v${VERSION}"

echo ""
echo "✅ Release v${VERSION} created!"
echo ""
echo "Monitor the release:"
echo "  gh run watch"
echo ""
echo "View release:"
echo "  https://github.com/gelutjari/aikd/releases/tag/v${VERSION}"
