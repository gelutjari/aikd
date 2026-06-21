#!/bin/bash
# Generate AIKD logo using FAL.ai API
# Required env: FAL_KEY

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ASSETS_DIR="$SCRIPT_DIR/../assets"

mkdir -p "$ASSETS_DIR"

if [ -z "$FAL_KEY" ]; then
    echo "❌ FAL_KEY not set. Get your key at https://fal.ai/dashboard/keys"
    echo "   export FAL_KEY=your-key-here"
    exit 1
fi

echo "🎨 Generating AIKD logo with FAL.ai..."

RESPONSE=$(curl -s -X POST "https://fal.run/fal-ai/flux-pro/v1.1" \
    -H "Authorization: Key $FAL_KEY" \
    -H "Content-Type: application/json" \
    -d '{
        "prompt": "Minimalist logo for AIKD (AI Knowledge Daemon), abstract brain combined with terminal cursor and code brackets, Rust orange #CE412B and deep navy blue, flat geometric modern, icon only no text, vector style, clean white background --ar 1:1",
        "image_size": "square_hd",
        "num_images": 4,
        "output_format": "png"
    }')

# Extract first image URL
IMAGE_URL=$(echo "$RESPONSE" | jq -r '.images[0].url')

if [ "$IMAGE_URL" = "null" ] || [ -z "$IMAGE_URL" ]; then
    echo "❌ Failed to generate logo. Response:"
    echo "$RESPONSE"
    exit 1
fi

echo "📥 Downloading logo..."
curl -s -o "$ASSETS_DIR/logo.png" "$IMAGE_URL"

# Resize for GitHub
if command -v convert &> /dev/null; then
    echo "📐 Resizing for GitHub..."
    convert "$ASSETS_DIR/logo.png" -resize 400x400 "$ASSETS_DIR/logo-github.png"
fi

echo "✅ Logo saved to $ASSETS_DIR/logo.png"
echo "   GitHub version: $ASSETS_DIR/logo-github.png"
