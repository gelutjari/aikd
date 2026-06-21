#!/usr/bin/env python3
"""Generate AIKD demo GIF frames - terminal animation"""

from PIL import Image, ImageDraw, ImageFont
import os

FRAMES_DIR = os.path.join(os.path.dirname(os.path.dirname(__file__)), 'assets', 'frames')
os.makedirs(FRAMES_DIR, exist_ok=True)

DARK_BG = '#1E1E1E'
TITLE_BAR = '#3C3C3C'
GREEN = '#3FB950'
BLUE = '#58A6FF'
ORANGE = '#CE412B'
GRAY = '#8B949E'
WHITE = '#E6EDF3'

def create_terminal_frame(lines, filename):
    """Create a terminal frame with given lines"""
    width, height = 900, 500
    img = Image.new('RGB', (width, height), DARK_BG)
    draw = ImageDraw.Draw(img)
    
    try:
        mono_font = ImageFont.truetype("consola.ttf", 14)
    except:
        mono_font = ImageFont.load_default()
    
    # Title bar
    draw.rectangle([0, 0, width, 30], fill=TITLE_BAR)
    draw.ellipse([10, 8, 26, 24], fill='#FF5F56')
    draw.ellipse([34, 8, 50, 24], fill='#FFBD2E')
    draw.ellipse([58, 8, 74, 24], fill='#27C93F')
    draw.text((width//2, 15), 'Terminal - AIKD Demo', fill='#CCCCCC', font=mono_font, anchor='mm')
    
    # Terminal content
    y = 50
    for text, color in lines:
        if text is None:
            y += 10
        else:
            draw.text((20, y), text, fill=color, font=mono_font)
            y += 22
    
    filepath = os.path.join(FRAMES_DIR, filename)
    img.save(filepath, 'PNG')
    return filepath

# Frame 1: Init
frame1_lines = [
    ('$ aikd init', GREEN),
    (None, None),
    ('  Scanning project structure...', GRAY),
    ('  Detected: Rust project (Cargo.toml)', GRAY),
    ('  Created: ~/.aikd/config.yaml', GRAY),
    (None, None),
    ('[+] AIKD initialized successfully', GREEN),
]

# Frame 2: Scan
frame2_lines = [
    ('$ aikd init', GREEN),
    ('[+] AIKD initialized successfully', GREEN),
    (None, None),
    ('$ aikd scan', GREEN),
    (None, None),
    ('[aikd] Discovering files... found 847 files', GRAY),
    ('[aikd] Checking for changes... 847 to index', GRAY),
    ('[aikd] Chunking files...', GRAY),
    ('[aikd] Storing 12,453 chunks...', GRAY),
    ('[aikd] Updating search index... done', GRAY),
    (None, None),
    ('Indexed 847 files, 12,453 chunks in 1.2s', BLUE),
]

# Frame 3: Query
frame3_lines = [
    ('$ aikd scan', GREEN),
    ('Indexed 847 files, 12,453 chunks in 1.2s', BLUE),
    (None, None),
    ('$ aikd query "authentication" --limit 3', GREEN),
    (None, None),
    ('1. src/auth/login.rs', WHITE),
    ('   Lines: 45-89 | Score: 0.923', GRAY),
    ('   Implements JWT-based authentication...', GRAY),
    (None, None),
    ('2. src/middleware/auth.rs', WHITE),
    ('   Lines: 12-34 | Score: 0.871', GRAY),
    ('   Validates Bearer tokens on routes...', GRAY),
    (None, None),
    ('3. docs/architecture/auth.md', WHITE),
    ('   Lines: 1-25 | Score: 0.845', GRAY),
    ('   Authentication flow diagram...', GRAY),
]

# Frame 4: Hybrid search
frame4_lines = [
    ('$ aikd query "authentication" --limit 3', GREEN),
    ('3 results found (0.21ms)', BLUE),
    (None, None),
    ('$ aikd query "how does login work" --hybrid', GREEN),
    (None, None),
    ('1. src/auth/login.rs', WHITE),
    ('   Lines: 45-89 | Score: 0.956', GRAY),
    ('   JWT token generation with refresh...', GRAY),
    (None, None),
    ('2. src/auth/oauth.rs', WHITE),
    ('   Lines: 1-45 | Score: 0.912', GRAY),
    ('   OAuth2 integration with Google...', GRAY),
    (None, None),
    ('Hybrid search: BM25 + Vector (0.35ms)', ORANGE),
]

# Frame 5: Stats
frame5_lines = [
    ('$ aikd query "how does login work" --hybrid', GREEN),
    ('2 results found (0.35ms)', BLUE),
    (None, None),
    ('$ aikd stats', GREEN),
    (None, None),
    ('AIKD v2.0.0', WHITE),
    ('Files: 847', GRAY),
    ('Chunks: 12,453', GRAY),
    ('Embeddings: 8,234 (384d)', GRAY),
    ('Sessions: 1', GRAY),
    ('Size: 23.4 KB', GRAY),
    (None, None),
    ('[+] Ready for AI agent integration', GREEN),
]

# Generate frames
frames = []
frames.append(create_terminal_frame(frame1_lines, 'frame1.png'))
frames.append(create_terminal_frame(frame2_lines, 'frame2.png'))
frames.append(create_terminal_frame(frame3_lines, 'frame3.png'))
frames.append(create_terminal_frame(frame4_lines, 'frame4.png'))
frames.append(create_terminal_frame(frame5_lines, 'frame5.png'))

print(f"[+] Generated {len(frames)} frames in {FRAMES_DIR}/")
for f in frames:
    print(f"  - {os.path.basename(f)}")
