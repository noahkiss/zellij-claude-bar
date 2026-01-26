# zellij-claude-bar

A Zellij status bar plugin that displays Claude API usage limits with pace indicators and a clock.

## Features

- **Usage Display**: Shows 5-hour and 7-day rate limit utilization
- **Pace Indicators**: Color-coded status based on usage vs. time elapsed:
  - **Green** (On Track): Sustainable pace
  - **Yellow** (Running Hot): Will exhaust before reset
  - **Red** (Underutilizing): Capacity going unused
- **Clock**: Right-aligned with locale-aware formatting
- **Responsive Layout**: Adapts from minimal (`5h:45% 7d:12%`) to full detail
- **Configurable**: 12/24h clock, date format, all with smart defaults from locale

## Installation

### Option 1: Download Release

```bash
# Download latest release
curl -L https://github.com/noahkiss/zellij-claude-bar/releases/latest/download/zellij_claude_bar.wasm \
  -o ~/.config/zellij/plugins/zellij_claude_bar.wasm

curl -L https://github.com/noahkiss/zellij-claude-bar/releases/latest/download/claude-usage \
  -o ~/.local/bin/claude-usage
chmod +x ~/.local/bin/claude-usage
```

### Option 2: Build from Source

```bash
rustup target add wasm32-wasip1
cargo build --release
cp target/wasm32-wasip1/release/zellij_claude_bar.wasm ~/.config/zellij/plugins/
cp bin/claude-usage ~/.local/bin/
```

### Set Up Cron

The plugin reads usage data from a JSON file updated by the CLI tool:

```bash
# Add to crontab (crontab -e)
*/5 * * * * ~/.local/bin/claude-usage >/dev/null 2>&1
```

### Configure Zellij Layout

Add to your layout (e.g., `~/.config/zellij/layouts/default.kdl`):

```kdl
layout {
    default_tab_template {
        pane size=1 borderless=true {
            plugin location="zellij:tab-bar"
        }
        pane size=1 borderless=true {
            plugin location="file:~/.config/zellij/plugins/zellij_claude_bar.wasm"
        }
        children
        pane size=1 borderless=true {
            plugin location="zellij:status-bar"
        }
    }
    tab
}
```

## Configuration

All options are optional with smart defaults:

```kdl
plugin location="file:~/.config/zellij/plugins/zellij_claude_bar.wasm" {
    clock "auto"        // "auto" | "12h" | "24h" | "off"
    suffix "short"      // "short" (a/p) | "long" (AM/PM) | "none"
    date_format "auto"  // "auto" | "us" | "intl" | "iso"
    data_file "/custom/path/to/usage.json"  // default: auto-detected
}
```

| Option | Values | Default | Description |
|--------|--------|---------|-------------|
| `clock` | auto/12h/24h/off | auto | Clock format (auto detects from locale) |
| `suffix` | short/long/none | short | AM/PM style for 12h mode |
| `date_format` | auto/us/intl/iso | auto | Date ordering |
| `data_file` | path | auto | Path to usage JSON |

## Display Modes

The plugin adapts to available width:

| Width | Example |
|-------|---------|
| 18-29 | `5h:45% 7d:12%` ... `10:43a` |
| 30-44 | `5h ████░░ 7d ██░░░░` ... `10:43 AM` |
| 45-69 | `5h: 45% (2h30m) │ 7d: 12% (4d)` ... `10:43 AM 1/27` |
| 70+ | `5h: 45% (50% elapsed) 2h30m │ ...` ... `10:43 AM Wed, Jan 27` |

## CLI Tool

The `claude-usage` script fetches usage from the Anthropic API:

```bash
# Run manually
claude-usage -v

# Output location (default)
~/.local/state/claude-usage/usage.json
```

Features:
- Uses OAuth token from `~/.claude/.credentials.json`
- Falls back gracefully: `jq` → `grep/sed`, `curl` → `wget`
- Works on macOS and Linux

## Requirements

- Zellij 0.40+
- Claude Code CLI (for OAuth credentials)
- `curl` or `wget`
- `jq` (optional, falls back to grep/sed)

## License

MIT
