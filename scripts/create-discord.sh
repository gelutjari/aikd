#!/bin/bash
# Create Discord server for AIKD community
# Required env: DISCORD_BOT_TOKEN

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOCS_DIR="$SCRIPT_DIR/../docs"

if [ -z "$DISCORD_BOT_TOKEN" ]; then
    echo "❌ DISCORD_BOT_TOKEN not set."
    echo "   1. Create Discord app at https://discord.com/developers/applications"
    echo "   2. Create bot and copy token"
    echo "   3. export DISCORD_BOT_TOKEN=your-token"
    exit 1
fi

echo "🤖 Creating Discord server..."

# Create server
SERVER_RESPONSE=$(curl -s -X POST "https://discord.com/api/v10/guilds" \
    -H "Authorization: Bot $DISCORD_BOT_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{
        "name": "AIKD Community",
        "icon": null,
        "verification_level": 1,
        "default_message_notifications": 0,
        "explicit_content_filter": 2
    }')

SERVER_ID=$(echo "$SERVER_RESPONSE" | jq -r '.id')

if [ "$SERVER_ID" = "null" ] || [ -z "$SERVER_ID" ]; then
    echo "❌ Failed to create server. Response:"
    echo "$SERVER_RESPONSE"
    exit 1
fi

echo "✅ Server created: $SERVER_ID"

# Create channels
CHANNELS=("general" "showcase" "help" "feature-requests" "announcements")

for channel in "${CHANNELS[@]}"; do
    echo "📝 Creating #$channel..."
    curl -s -X POST "https://discord.com/api/v10/guilds/$SERVER_ID/channels" \
        -H "Authorization: Bot $DISCORD_BOT_TOKEN" \
        -H "Content-Type: application/json" \
        -d "{\"name\": \"$channel\", \"type\": 0}" > /dev/null
done

# Create voice channel
echo "🎤 Creating voice-chat..."
curl -s -X POST "https://discord.com/api/v10/guilds/$SERVER_ID/channels" \
    -H "Authorization: Bot $DISCORD_BOT_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"name": "voice-chat", "type": 2}' > /dev/null

# Create invite link
echo "🔗 Creating invite link..."
CHANNELS_RESPONSE=$(curl -s "https://discord.com/api/v10/guilds/$SERVER_ID/channels" \
    -H "Authorization: Bot $DISCORD_BOT_TOKEN")

GENERAL_ID=$(echo "$CHANNELS_RESPONSE" | jq -r '.[] | select(.name == "general") | .id')

INVITE_RESPONSE=$(curl -s -X POST "https://discord.com/api/v10/channels/$GENERAL_ID/invites" \
    -H "Authorization: Bot $DISCORD_BOT_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"max_age": 0, "max_uses": 0}')

INVITE_CODE=$(echo "$INVITE_RESPONSE" | jq -r '.code')

# Save invite URL
mkdir -p "$DOCS_DIR"
echo "https://discord.gg/$INVITE_CODE" > "$DOCS_DIR/discord-invite.txt"

echo ""
echo "✅ Discord server created!"
echo "   Server ID: $SERVER_ID"
echo "   Invite URL: https://discord.gg/$INVITE_CODE"
echo "   Saved to: $DOCS_DIR/discord-invite.txt"
