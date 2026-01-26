# zellij-claude-bar Development Guide

## Project Overview

A Zellij plugin (WASM) written in Rust that displays a single-line status bar with:
1. Current time (clock)
2. Claude API 5-hour rate limit usage and projection
3. Claude API 7-day rate limit usage and projection
4. Color-coded status indicators based on limit projection

## Architecture

### Plugin Lifecycle

Zellij plugins implement the `ZellijPlugin` trait from `zellij-tile`:

- `load()`: Called once on plugin initialization. Subscribe to events, request permissions, set up timers.
- `update(event)`: Called when subscribed events occur. Return `true` to trigger re-render.
- `render(rows, cols)`: Called to draw the UI. Print to STDOUT with ANSI codes.

### Key Dependencies

- `zellij-tile` (0.43+): The official Zellij plugin SDK
- `chrono`: Time handling and formatting
- `serde/serde_json`: JSON parsing for limits data

### Build Target

The plugin compiles to `wasm32-wasip1` (WebAssembly). Configuration in `.cargo/config.toml`.

## Theme Colors

Zellij provides theme-aware colors through the `Palette` struct (received via `ModeUpdate` event):

| Color | Use Case |
|-------|----------|
| `green` | On track - will hit limit close to reset |
| `orange` / `yellow` | Running hot - will hit limit before reset |
| `red` | Underutilizing - won't hit limit (wasted capacity) |

For colored text rendering, use `color_range(index, range)` on `Text` elements:
- Index 0: foreground
- Index 1: green
- Index 2: orange/yellow
- Index 3: red

## Responsive Layout

The plugin adapts to terminal width:

| Width | Display |
|-------|---------|
| < 20 | Clock only |
| 20-29 | Clock + minimal limits |
| 30-49 | Clock + usage ratios |
| 50-79 | Clock + labeled usage |
| 80+ | Full display with reset times |

## Open Questions / TODOs

### Limit Data Source

The Claude API doesn't have a public endpoint for checking current usage. Options:

1. **Local file tracking**: Write usage to a JSON file that the plugin reads
2. **API header parsing**: Extract rate limit headers from Claude API responses
3. **External service**: Build a small service that tracks usage
4. **Manual configuration**: User updates limits file manually

Current implementation uses a placeholder. The `limits_file` config option is stubbed.

### Enhanced Color Rendering

The current implementation prints plain text. For proper theme-colored output:

```rust
use zellij_tile::prelude::*;

// In render():
let text = Text::new("OK")
    .color_range(1, 0..2); // Color with green (index 1)
print_text_with_coordinates(text, x, y, None, None);
```

### Events Used

- `Timer`: Periodic updates (every 30 seconds)
- `ModeUpdate`: Receive theme palette
- `PermissionRequestResult`: Check web access permission
- `WebRequestResult`: Handle API responses

### Permissions Needed

- `ReadApplicationState`: Access mode/theme info
- `WebAccess`: Make HTTP requests (if fetching from API)

## Development Workflow

1. Edit source in `src/lib.rs`
2. Build: `cargo build`
3. Test: `zellij action start-or-reload-plugin file:target/wasm32-wasip1/debug/zellij_claude_bar.wasm`

Or use the Zellij development layout pattern for hot-reloading.

## File Structure

```
zellij-claude-bar/
├── .cargo/
│   └── config.toml      # WASM build target config
├── src/
│   └── lib.rs           # Main plugin code
├── Cargo.toml           # Dependencies
├── README.md            # User documentation
├── CLAUDE.md            # Points to AGENTS.md
└── AGENTS.md            # This file - dev context
```
