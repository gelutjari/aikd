#!/usr/bin/env python3
"""
AIKD Social Media Uploader

NOTE: Direct API upload requires:
- Instagram: Facebook Business account + Graph API token
- TikTok: TikTok Developer account + API access
- Threads: Instagram account (same as Instagram)

This script helps prepare content for manual upload.
For direct API upload, you need to set up API access first.

Usage:
    python scripts/social-upload.py --platform all
    python scripts/social-upload.py --platform instagram
    python scripts/social-upload.py --platform tiktok
    python scripts/social-upload.py --platform threads
"""

import os
import sys
import json
import subprocess
from pathlib import Path

# Project root
PROJECT_ROOT = Path(__file__).parent.parent
ASSETS_DIR = PROJECT_ROOT / 'assets'
DOCS_DIR = PROJECT_ROOT / 'docs' / 'social'

# Platform configurations
PLATFORMS = {
    'instagram': {
        'name': 'Instagram Reels',
        'video': 'demo-vertical.mp4',
        'aspect': '9:16',
        'resolution': '1080x1920',
        'caption_file': 'instagram-caption.txt',
        'hashtags_file': 'instagram-hashtags.txt',
    },
    'tiktok': {
        'name': 'TikTok',
        'video': 'demo-vertical.mp4',
        'aspect': '9:16',
        'resolution': '1080x1920',
        'caption_file': 'tiktok-caption.txt',
        'hashtags_file': 'tiktok-hashtags.txt',
    },
    'threads': {
        'name': 'Threads',
        'video': 'demo-square.mp4',
        'aspect': '1:1',
        'resolution': '1080x1080',
        'caption_file': 'threads-caption.txt',
        'hashtags_file': 'threads-hashtags.txt',
    },
    'twitter': {
        'name': 'Twitter/X',
        'video': 'demo-square.mp4',
        'aspect': '1:1',
        'resolution': '1080x1080',
        'caption_file': 'twitter-caption.txt',
        'hashtags_file': 'twitter-hashtags.txt',
    },
    'youtube': {
        'name': 'YouTube Shorts',
        'video': 'demo-vertical.mp4',
        'aspect': '9:16',
        'resolution': '1080x1920',
        'caption_file': 'youtube-caption.txt',
        'hashtags_file': 'youtube-hashtags.txt',
    },
    'linkedin': {
        'name': 'LinkedIn',
        'video': 'demo.mp4',
        'aspect': '16:9',
        'resolution': '900x500',
        'caption_file': 'linkedin-caption.txt',
        'hashtags_file': 'linkedin-hashtags.txt',
    },
}

# Captions for each platform
CAPTIONS = {
    'instagram': """[BRAIN] AIKD - Give your AI coding agents instant memory!

Tired of explaining your codebase to Claude/Cursor every single conversation?

AIKD indexes your code locally and provides instant search in 0.21ms [FAST]

[OK] 100% local - your code never leaves your machine
[OK] Single binary - no Python, no dependencies
[OK] Works with Claude, Cursor, Cline out of the box

Try it now:
curl -sSf https://raw.githubusercontent.com/gelutjari/aikd/main/install.sh | bash

GitHub: github.com/gelutjari/aikd""",

    'tiktok': """POV: Your AI agent finally remembers your codebase [BRAIN]

AIKD gives Claude/Cursor instant memory of your entire project.

0.21ms search. 100% local. Single binary.

Link in bio 👆""",

    'threads': """[BRAIN] Just open-sourced AIKD - a Rust tool that gives AI coding agents instant memory of your codebase.

The problem: Every time you start a new conversation with Claude/Cursor, it forgets everything about your project.

The solution: AIKD indexes your code locally and provides instant search via MCP protocol.

Key features:
• 0.21ms search queries (yes, milliseconds)
• 100% local - zero cloud dependency
• Single binary - no Python/Node required
• Works with Claude, Cursor, Cline out of the box

Try it: github.com/gelutjari/aikd""",

    'twitter': """[ROCKET] Just open-sourced AIKD — a Rust tool that gives AI coding agents instant memory of your codebase.

0.21ms search queries. 100% local. Single binary.

GitHub: github.com/gelutjari/aikd

#Rust #AI #OpenSource #DeveloperTools""",

    'youtube': """[BRAIN] AIKD - The Ultra-Fast Local Memory Layer for AI Coding Agents

Give Claude, Cursor, and Cline instant memory of your entire codebase.

[OK] 0.21ms search queries
[OK] 100% local - your code never leaves your machine
[OK] Single binary - no Python, no dependencies
[OK] Works with Claude, Cursor, Cline out of the box

[LINK] GitHub: https://github.com/gelutjari/aikd
[PACKAGE] Install: curl -sSf https://raw.githubusercontent.com/gelutjari/aikd/main/install.sh | bash""",

    'linkedin': """[BRAIN] Excited to share AIKD - a Rust-based tool that gives AI coding agents instant memory of your codebase.

The Problem:
Every time you start a new conversation with Claude, Cursor, or Cline, it forgets everything about your project.

The Solution:
AIKD indexes your code locally and provides instant search via MCP protocol.

Key Features:
• 0.21ms search queries (yes, milliseconds)
• 100% local - zero cloud dependency
• Single binary - no Python/Node required
• Works with Claude, Cursor, Cline out of the box

Try it: github.com/gelutjari/aikd

#Rust #AI #OpenSource #DeveloperTools #SoftwareEngineering""",
}

# Hashtags for each platform
HASHTAGS = {
    'instagram': '#Rust #AI #Coding #Developer #Programming #OpenSource #TechStartup #CodeNewbie #100DaysOfCode #WebDev #SoftwareEngineering #MachineLearning #AIAssistant #Codebase #DeveloperTools #ClaudeAI #Cursor #Cline #MCP #SearchEngine',
    'tiktok': '#Rust #AI #Coding #Developer #Programming #OpenSource #TechTok #CodeTok #DevTok #AIAssistant #ClaudeAI #Cursor #100DaysOfCode #WebDev #SoftwareEngineering #MachineLearning',
    'threads': '#Rust #AI #OpenSource #Developer #Programming #Coding',
    'twitter': '#Rust #AI #OpenSource #DeveloperTools',
    'youtube': '#Rust #AI #Coding #Developer #OpenSource #Programming',
    'linkedin': '#Rust #AI #OpenSource #DeveloperTools #SoftwareEngineering #MachineLearning #Coding #Programming',
}


def create_content_files():
    """Create caption and hashtag files for each platform"""
    output_dir = DOCS_DIR / 'upload-content'
    output_dir.mkdir(parents=True, exist_ok=True)
    
    for platform, config in PLATFORMS.items():
        # Create caption file
        caption_file = output_dir / config['caption_file']
        with open(caption_file, 'w', encoding='utf-8') as f:
            f.write(CAPTIONS[platform])
        
        # Create hashtag file
        hashtags_file = output_dir / config['hashtags_file']
        with open(hashtags_file, 'w', encoding='utf-8') as f:
            f.write(HASHTAGS[platform])
    
    print(f"[+] Created content files in {output_dir}/")
    return output_dir


def check_videos():
    """Check if all required videos exist"""
    missing = []
    for platform, config in PLATFORMS.items():
        video_path = ASSETS_DIR / config['video']
        if not video_path.exists():
            missing.append(f"{platform}: {config['video']}")
    
    if missing:
        print("[!] Missing videos:")
        for m in missing:
            print(f"    - {m}")
        return False
    
    print("[+] All videos present")
    return True


def print_upload_instructions():
    """Print upload instructions for each platform"""
    print("\n" + "=" * 60)
    print("SOCIAL MEDIA UPLOAD INSTRUCTIONS")
    print("=" * 60)
    
    for platform, config in PLATFORMS.items():
        video_path = ASSETS_DIR / config['video']
        caption_path = DOCS_DIR / 'upload-content' / config['caption_file']
        hashtags_path = DOCS_DIR / 'upload-content' / config['hashtags_file']
        
        print(f"\n[MOBILE] {config['name']}")
        print(f"   Video: {video_path}")
        print(f"   Aspect: {config['aspect']} ({config['resolution']})")
        print(f"   Caption: {caption_path}")
        print(f"   Hashtags: {hashtags_path}")
        print(f"   Status: {'[OK]' if video_path.exists() else '[MISSING]'}")


def main():
    """Main function"""
    print("=" * 60)
    print("AIKD SOCIAL MEDIA UPLOADER")
    print("=" * 60)
    
    # Check videos
    print("\n[1] Checking videos...")
    videos_ok = check_videos()
    
    # Create content files
    print("\n[2] Creating content files...")
    content_dir = create_content_files()
    
    # Print instructions
    print("\n[3] Upload instructions:")
    print_upload_instructions()
    
    print("\n" + "=" * 60)
    print("NEXT STEPS")
    print("=" * 60)
    print("""
1. Open each platform's app/website
2. Upload the video from assets/ folder
3. Copy caption from docs/social/upload-content/
4. Add hashtags from docs/social/upload-content/
5. Post!

For direct API upload (requires business accounts):
- Instagram: https://developers.facebook.com/docs/instagram-api
- TikTok: https://developers.tiktok.com/
- Threads: Same as Instagram API

Files are ready in:
  Videos: {assets}/
  Captions: {content}/
""".format(assets=ASSETS_DIR, content=content_dir))


if __name__ == '__main__':
    main()
