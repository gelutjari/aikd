#!/usr/bin/env python3
"""Generate AIKD visual assets - logo, banner, demo images"""

from PIL import Image, ImageDraw, ImageFont
import os

ASSETS_DIR = os.path.join(os.path.dirname(os.path.dirname(__file__)), 'assets')
os.makedirs(ASSETS_DIR, exist_ok=True)

# Colors
RUST_ORANGE = '#CE412B'
NAVY_BLUE = '#1A1A2E'
DARK_BG = '#0D1117'
LIGHT_TEXT = '#E6EDF3'
ACCENT_GREEN = '#3FB950'
ACCENT_BLUE = '#58A6FF'
ACCENT_PURPLE = '#BC8CFF'

def create_logo():
    """Create AIKD logo"""
    size = 400
    img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    
    # Background circle
    margin = 20
    draw.ellipse([margin, margin, size-margin, size-margin], fill=NAVY_BLUE)
    
    # Brain shape (simplified)
    cx, cy = size // 2, size // 2 - 20
    
    # Left brain half
    draw.arc([cx-80, cy-60, cx, cy+60], 180, 0, fill=RUST_ORANGE, width=8)
    draw.arc([cx-70, cy-50, cx-10, cy+50], 180, 0, fill=RUST_ORANGE, width=6)
    
    # Right brain half
    draw.arc([cx, cy-60, cx+80, cy+60], 0, 180, fill=RUST_ORANGE, width=8)
    draw.arc([cx+10, cy-50, cx+70, cy+50], 0, 180, fill=RUST_ORANGE, width=6)
    
    # Terminal cursor
    cursor_x, cursor_y = cx - 15, cy + 40
    draw.rectangle([cursor_x, cursor_y, cursor_x+30, cursor_y+40], fill=ACCENT_GREEN)
    draw.polygon([(cursor_x+30, cursor_y), (cursor_x+30, cursor_y+15), (cursor_x+45, cursor_y+7)], fill=ACCENT_GREEN)
    
    # Code brackets
    draw.text((cx-60, cy+50), '<', fill=ACCENT_BLUE, font=ImageFont.load_default())
    draw.text((cx+50, cy+50), '>', fill=ACCENT_BLUE, font=ImageFont.load_default())
    
    # Neural connections
    for i in range(5):
        x1 = cx - 40 + i * 20
        y1 = cy - 30 + (i % 2) * 20
        draw.line([(x1, y1), (x1 + 15, y1 + 15)], fill=ACCENT_PURPLE, width=2)
        draw.ellipse([x1+12, y1+12, x1+18, y1+18], fill=ACCENT_PURPLE)
    
    logo_path = os.path.join(ASSETS_DIR, 'logo.png')
    img.save(logo_path, 'PNG')
    print(f"[+] Logo saved: {logo_path}")
    return logo_path

def create_banner():
    """Create GitHub banner"""
    width, height = 1200, 630
    img = Image.new('RGB', (width, height), DARK_BG)
    draw = ImageDraw.Draw(img)
    
    # Gradient background
    for y in range(height):
        r = int(13 + (26 - 13) * y / height)
        g = int(17 + (26 - 17) * y / height)
        b = int(23 + (46 - 23) * y / height)
        draw.line([(0, y), (width, y)], fill=(r, g, b))
    
    # Title
    try:
        title_font = ImageFont.truetype("arial.ttf", 72)
        subtitle_font = ImageFont.truetype("arial.ttf", 32)
        tagline_font = ImageFont.truetype("arial.ttf", 24)
    except:
        title_font = ImageFont.load_default()
        subtitle_font = ImageFont.load_default()
        tagline_font = ImageFont.load_default()
    
    # AIKD title
    draw.text((width//2, 150), ' AIKD', fill=RUST_ORANGE, font=title_font, anchor='mm')
    
    # Subtitle
    draw.text((width//2, 250), 'The Ultra-Fast Local Memory Layer', fill=LIGHT_TEXT, font=subtitle_font, anchor='mm')
    draw.text((width//2, 300), 'for AI Coding Agents', fill=LIGHT_TEXT, font=subtitle_font, anchor='mm')
    
    # Stats boxes
    stats = [
        ('0.21ms', 'Search Latency'),
        ('100%', 'Local & Private'),
        ('33MB', 'Single Binary'),
        ('Rust', 'Powered By')
    ]
    
    box_width = 200
    box_height = 80
    start_x = (width - len(stats) * box_width - (len(stats)-1) * 30) // 2
    
    for i, (value, label) in enumerate(stats):
        x = start_x + i * (box_width + 30)
        y = 400
        
        # Box background
        draw.rounded_rectangle([x, y, x+box_width, y+box_height], radius=10, fill=NAVY_BLUE)
        
        # Value
        draw.text((x + box_width//2, y + 25), value, fill=ACCENT_GREEN, font=tagline_font, anchor='mm')
        
        # Label
        small_font = tagline_font
        draw.text((x + box_width//2, y + 55), label, fill=LIGHT_TEXT, font=small_font, anchor='mm')
    
    # Bottom tagline
    draw.text((width//2, 550), 'Written in Rust. Zero cloud dependency. Search 10,000 chunks in 0.21ms.', 
              fill='#8B949E', font=tagline_font, anchor='mm')
    
    banner_path = os.path.join(ASSETS_DIR, 'banner.png')
    img.save(banner_path, 'PNG')
    print(f"[+] Banner saved: {banner_path}")
    return banner_path

def create_demo_screenshot():
    """Create demo terminal screenshot"""
    width, height = 900, 600
    img = Image.new('RGB', (width, height), '#1E1E1E')
    draw = ImageDraw.Draw(img)
    
    try:
        mono_font = ImageFont.truetype("consola.ttf", 14)
        bold_font = ImageFont.truetype("consolab.ttf", 14)
    except:
        mono_font = ImageFont.load_default()
        bold_font = ImageFont.load_default()
    
    # Terminal title bar
    draw.rectangle([0, 0, width, 30], fill='#3C3C3C')
    draw.ellipse([10, 8, 26, 24], fill='#FF5F56')
    draw.ellipse([34, 8, 50, 24], fill='#FFBD2E')
    draw.ellipse([58, 8, 74, 24], fill='#27C93F')
    draw.text((width//2, 15), 'Terminal', fill='#CCCCCC', font=mono_font, anchor='mm')
    
    # Terminal content
    y = 50
    lines = [
        ('$ aikd init', ACCENT_GREEN),
        ('[+] AIKD initialized for project', '#8B949E'),
        ('', None),
        ('$ aikd scan', ACCENT_GREEN),
        ('[aikd] Discovering files... found 847 files', '#8B949E'),
        ('[aikd] Checking for changes... 847 to index', '#8B949E'),
        ('[aikd] ████████████████████████████████ 847/847', ACCENT_BLUE),
        ('[aikd] Storing 12,453 chunks from 847 files...', '#8B949E'),
        ('[aikd] Updating search index... done', '#8B949E'),
        ('', None),
        ('$ aikd query "authentication" --limit 3', ACCENT_GREEN),
        ('', None),
        ('1. src/auth/login.rs', LIGHT_TEXT),
        ('   Lines: 45-89 | Score: 0.923', '#8B949E'),
        ('   Implements JWT-based authentication with...', '#8B949E'),
        ('', None),
        ('2. src/middleware/auth.rs', LIGHT_TEXT),
        ('   Lines: 12-34 | Score: 0.871', '#8B949E'),
        ('   Validates Bearer tokens on protected routes...', '#8B949E'),
        ('', None),
        ('3. docs/architecture/auth.md', LIGHT_TEXT),
        ('   Lines: 1-25 | Score: 0.845', '#8B949E'),
        ('   Authentication flow diagram and security...', '#8B949E'),
    ]
    
    for text, color in lines:
        if text:
            draw.text((20, y), text, fill=color, font=mono_font)
        y += 20
    
    demo_path = os.path.join(ASSETS_DIR, 'demo-screenshot.png')
    img.save(demo_path, 'PNG')
    print(f"[+] Demo screenshot saved: {demo_path}")
    return demo_path

def create_architecture_diagram():
    """Create architecture diagram"""
    width, height = 1000, 700
    img = Image.new('RGB', (width, height), DARK_BG)
    draw = ImageDraw.Draw(img)
    
    try:
        title_font = ImageFont.truetype("arial.ttf", 28)
        label_font = ImageFont.truetype("arial.ttf", 16)
        small_font = ImageFont.truetype("arial.ttf", 12)
    except:
        title_font = ImageFont.load_default()
        label_font = ImageFont.load_default()
        small_font = ImageFont.load_default()
    
    # Title
    draw.text((width//2, 30), 'AIKD Architecture', fill=LIGHT_TEXT, font=title_font, anchor='mm')
    
    # Interface layer
    interfaces = [
        ('CLI', 150), ('MCP Server', 400), ('REST API', 650)
    ]
    
    for name, x in interfaces:
        draw.rounded_rectangle([x-60, 80, x+60, 120], radius=8, fill=ACCENT_BLUE)
        draw.text((x, 100), name, fill='white', font=label_font, anchor='mm')
    
    # Arrow down
    for x in [150, 400, 650]:
        draw.line([(x, 120), (x, 180)], fill='#8B949E', width=2)
        draw.polygon([(x-5, 175), (x+5, 175), (x, 185)], fill='#8B949E')
    
    # Core Engine
    draw.rounded_rectangle([100, 180, 750, 260], radius=10, fill=NAVY_BLUE, outline=ACCENT_PURPLE)
    draw.text((425, 200), 'Core Engine', fill=ACCENT_PURPLE, font=label_font, anchor='mm')
    
    core_components = [
        ('Scanner', 180), ('Chunker', 320), ('Session', 460), ('Watcher', 600)
    ]
    
    for name, x in core_components:
        draw.rounded_rectangle([x-45, 220, x+45, 250], radius=5, fill=DARK_BG, outline=ACCENT_PURPLE)
        draw.text((x, 235), name, fill=LIGHT_TEXT, font=small_font, anchor='mm')
    
    # Arrow down
    for x in [250, 425, 600]:
        draw.line([(x, 260), (x, 310)], fill='#8B949E', width=2)
        draw.polygon([(x-5, 305), (x+5, 305), (x, 315)], fill='#8B949E')
    
    # Storage layer
    storage = [
        ('Tantivy\nBM25', 180, ACCENT_GREEN),
        ('ONNX\nEmbeddings', 425, RUST_ORANGE),
        ('SQLite\n+WAL', 670, ACCENT_BLUE)
    ]
    
    for name, x, color in storage:
        draw.rounded_rectangle([x-70, 310, x+70, 380], radius=8, fill=NAVY_BLUE, outline=color)
        draw.text((x, 345), name, fill=color, font=label_font, anchor='mm')
    
    # HNSW Index
    draw.rounded_rectangle([350, 420, 500, 470], radius=8, fill=NAVY_BLUE, outline=ACCENT_PURPLE)
    draw.text((425, 445), 'HNSW Index', fill=ACCENT_PURPLE, font=label_font, anchor='mm')
    
    # Data flow labels
    draw.text((200, 290), 'Index', fill='#8B949E', font=small_font, anchor='mm')
    draw.text((425, 290), 'Embed', fill='#8B949E', font=small_font, anchor='mm')
    draw.text((650, 290), 'Store', fill='#8B949E', font=small_font, anchor='mm')
    
    # Query flow
    draw.rounded_rectangle([350, 500, 500, 540], radius=8, fill=DARK_BG, outline=ACCENT_GREEN)
    draw.text((425, 520), 'Hybrid Search', fill=ACCENT_GREEN, font=label_font, anchor='mm')
    
    # RRF label
    draw.text((425, 560), 'Reciprocal Rank Fusion', fill='#8B949E', font=small_font, anchor='mm')
    
    # Bottom features
    features = [
        (' 0.21ms queries', 150),
        (' 100% local', 350),
        (' Resource adaptive', 550),
        (' Auto-reindex', 750)
    ]
    
    for text, x in features:
        draw.rounded_rectangle([x-70, 600, x+70, 640], radius=5, fill=NAVY_BLUE)
        draw.text((x, 620), text, fill=LIGHT_TEXT, font=small_font, anchor='mm')
    
    arch_path = os.path.join(ASSETS_DIR, 'architecture.png')
    img.save(arch_path, 'PNG')
    print(f"[+] Architecture diagram saved: {arch_path}")
    return arch_path

def create_comparison_chart():
    """Create comparison chart"""
    width, height = 800, 500
    img = Image.new('RGB', (width, height), DARK_BG)
    draw = ImageDraw.Draw(img)
    
    try:
        title_font = ImageFont.truetype("arial.ttf", 28)
        header_font = ImageFont.truetype("arial.ttf", 18)
        cell_font = ImageFont.truetype("arial.ttf", 14)
    except:
        title_font = ImageFont.load_default()
        header_font = ImageFont.load_default()
        cell_font = ImageFont.load_default()
    
    # Title
    draw.text((width//2, 30), 'Performance Comparison', fill=LIGHT_TEXT, font=title_font, anchor='mm')
    
    # Table
    headers = ['Metric', 'grep', 'LlamaIndex', 'AIKD']
    col_widths = [180, 140, 140, 140]
    row_height = 50
    start_x = 60
    start_y = 80
    
    # Header row
    x = start_x
    for i, (header, w) in enumerate(zip(headers, col_widths)):
        color = ACCENT_BLUE if i == 3 else '#8B949E'
        draw.rounded_rectangle([x, start_y, x+w, start_y+row_height], radius=5, fill=NAVY_BLUE)
        draw.text((x + w//2, start_y + row_height//2), header, fill=color, font=header_font, anchor='mm')
        x += w + 10
    
    # Data rows
    data = [
        ('Search Speed', '<1ms', '500ms+', '0.21ms ✓'),
        ('Semantic Search', '[-]', '[+]', '[+] ✓'),
        ('Local Only', '[+]', '[-]', '[+] ✓'),
        ('Memory Usage', 'Low', 'High', '27% RAM ✓'),
        ('Setup Time', '0s', '~10min', '30s ✓'),
        ('MCP Native', '[-]', '[-]', '[+] ✓'),
    ]
    
    for row_idx, (metric, *values) in enumerate(data):
        y = start_y + (row_idx + 1) * (row_height + 10)
        x = start_x
        
        # Metric name
        draw.rounded_rectangle([x, y, x+col_widths[0], y+row_height], radius=5, fill=NAVY_BLUE)
        draw.text((x + col_widths[0]//2, y + row_height//2), metric, fill=LIGHT_TEXT, font=cell_font, anchor='mm')
        x += col_widths[0] + 10
        
        # Values
        for i, (value, w) in enumerate(zip(values, col_widths[1:])):
            is_best = '✓' in value
            color = ACCENT_GREEN if is_best else '#8B949E'
            bg = '#1A2332' if is_best else NAVY_BLUE
            
            draw.rounded_rectangle([x, y, x+w, y+row_height], radius=5, fill=bg)
            draw.text((x + w//2, y + row_height//2), value.replace(' ✓', ''), fill=color, font=cell_font, anchor='mm')
            x += w + 10
    
    chart_path = os.path.join(ASSETS_DIR, 'comparison.png')
    img.save(chart_path, 'PNG')
    print(f"[+] Comparison chart saved: {chart_path}")
    return chart_path

def create_benchmark_chart():
    """Create benchmark visualization"""
    width, height = 800, 400
    img = Image.new('RGB', (width, height), DARK_BG)
    draw = ImageDraw.Draw(img)
    
    try:
        title_font = ImageFont.truetype("arial.ttf", 28)
        label_font = ImageFont.truetype("arial.ttf", 14)
        value_font = ImageFont.truetype("arial.ttf", 12)
    except:
        title_font = ImageFont.load_default()
        label_font = ImageFont.load_default()
        value_font = ImageFont.load_default()
    
    # Title
    draw.text((width//2, 30), 'Benchmark Results', fill=LIGHT_TEXT, font=title_font, anchor='mm')
    
    # Bars
    benchmarks = [
        ('Index 1K files', 144, 'ms', ACCENT_BLUE),
        ('BM25 Search', 0.21, 'ms', ACCENT_GREEN),
        ('Hybrid Search', 0.35, 'ms', RUST_ORANGE),
        ('500 Concurrent', 28, 'ms', ACCENT_PURPLE),
    ]
    
    bar_height = 50
    max_value = 150
    start_x = 200
    start_y = 80
    
    for i, (label, value, unit, color) in enumerate(benchmarks):
        y = start_y + i * (bar_height + 20)
        
        # Label
        draw.text((start_x - 10, y + bar_height//2), label, fill=LIGHT_TEXT, font=label_font, anchor='rm')
        
        # Bar
        bar_width = int((value / max_value) * 400)
        bar_width = max(bar_width, 20)  # Minimum width
        
        draw.rounded_rectangle([start_x, y, start_x + bar_width, y + bar_height], radius=8, fill=color)
        
        # Value
        draw.text((start_x + bar_width + 10, y + bar_height//2), f'{value} {unit}', fill=color, font=value_font, anchor='lm')
    
    # Bottom note
    draw.text((width//2, height - 40), 'Hardware: AMD EPYC 7B13, 7.8GB RAM', fill='#8B949E', font=value_font, anchor='mm')
    draw.text((width//2, height - 20), '857 searches per human blink ', fill=ACCENT_GREEN, font=value_font, anchor='mm')
    
    bench_path = os.path.join(ASSETS_DIR, 'benchmarks.png')
    img.save(bench_path, 'PNG')
    print(f"[+] Benchmark chart saved: {bench_path}")
    return bench_path

if __name__ == '__main__':
    print("[*] Generating AIKD visual assets...\n")
    
    create_logo()
    create_banner()
    create_demo_screenshot()
    create_architecture_diagram()
    create_comparison_chart()
    create_benchmark_chart()
    
    print(f"\n[+] All assets saved to {ASSETS_DIR}/")
    print("\nFiles created:")
    for f in os.listdir(ASSETS_DIR):
        if f.endswith('.png'):
            size = os.path.getsize(os.path.join(ASSETS_DIR, f))
            print(f"  - {f} ({size:,} bytes)")
