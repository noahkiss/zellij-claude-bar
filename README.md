# zellij-claude-bar

A Zellij status bar plugin that displays Claude API usage limits with pace indicators and a clock.

## Features

- **Usage Display**: Shows 5-hour and 7-day rate limit utilization with pace arrows (↑/↓)
- **Pace Indicators**: Graduated color thresholds — green → yellow → orange → blinking red
  - 5h: thresholds based on delta from pace (+5%, +10%, +15%)
  - 7d: urgency scales with remaining time — same delta feels worse as window closes
- **Extra Usage**: Shows overage spend when rate-limited (e.g., `⚠ $27.25/$40`)
- **Clock**: Right-aligned with configurable formatting
- **Responsive Layout**: Adapts from minimal (`5h:45%↑ 7d:12%↓`) to full detail
- **Configurable**: 12/24h clock, date format, AM/PM style

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
cp target/wasm32-wasip1/release/zellij-claude-bar.wasm ~/.config/zellij/plugins/zellij_claude_bar.wasm
cp bin/claude-usage ~/.local/bin/
```

> **Note**: If you have Homebrew Rust installed alongside rustup, you may need to explicitly use rustup's toolchain:
> ```bash
> RUSTC=~/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc ~/.cargo/bin/cargo build --release
> ```

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
            plugin location="file:~/.config/zellij/plugins/zellij_claude_bar.wasm" {
                data_file "/home/YOUR_USERNAME/.local/state/claude-usage/usage.json"
            }
        }
        children
        pane size=1 borderless=true {
            plugin location="zellij:status-bar"
        }
    }
    tab
}
```

> **Important**: You must specify `data_file` with an absolute path. WASM plugins cannot access environment variables like `$HOME`.

On first launch, Zellij will prompt you to grant the `RunCommands` permission. Focus the plugin pane and press `y` to allow it to read the usage data file.

## Configuration

```kdl
plugin location="file:~/.config/zellij/plugins/zellij_claude_bar.wasm" {
    data_file "/home/user/.local/state/claude-usage/usage.json"  // REQUIRED: absolute path
    clock "12h"         // "12h" | "24h" | "off"
    suffix "short"      // "short" (a/p) | "long" (AM/PM) | "none"
    date_format "us"    // "us" | "intl" | "iso"
}
```

| Option | Values | Default | Description |
|--------|--------|---------|-------------|
| `data_file` | absolute path | - | **Required.** Path to usage JSON file |
| `clock` | 12h/24h/off | 12h | Clock format (`auto` accepted, defaults to 12h) |
| `suffix` | short/long/none | short | AM/PM style for 12h mode |
| `date_format` | us/intl/iso | us | Date ordering (`auto` accepted, defaults to US) |

## Display Modes

The plugin adapts to available width:

| Width | Example |
|-------|---------|
| 18-29 | `5h:45%↑ 7d:12%↓` ... `10:43a` |
| 30-44 | `5h ████░░ 7d ██░░░░` ... `10:43a` |
| 45-69 | `5h: 45%↑ (2h30m) │ 7d: 12%↓ (4d)` ... `10:43a 1/27` |
| 70+ | `5h: 45%↑ (50% elapsed) 2h30m │ ...` ... `10:43a Wed, Jan 27` |

## CLI Tool

The `claude-usage` script fetches usage from the Anthropic API:

```bash
# Run manually
claude-usage -v

# Output locations (default)
~/.local/state/claude-usage/usage.json      # Current state (plugin reads this)
~/.local/state/claude-usage/history.jsonl   # Append-only history log
```

Features:
- Passes through full API response (with jq) — captures all windows and extra usage data
- Uses OAuth token from `~/.claude/.credentials.json`
- Falls back gracefully: `jq` (full response) or `grep/sed` (core fields only), `curl` → `wget`
- Works on macOS and Linux
- Appends each fetch to history log (JSONL format) for later analysis

### Analyzing History

```bash
# Count entries
wc -l ~/.local/state/claude-usage/history.jsonl

# View as JSON array
cat ~/.local/state/claude-usage/history.jsonl | jq -s '.'

# Extract utilization over time (TSV)
cat ~/.local/state/claude-usage/history.jsonl | jq -r '[.fetched_at, .five_hour.utilization, .seven_day.utilization] | @tsv'
```

## Requirements

- Zellij 0.40+
- Claude Code CLI (for OAuth credentials)
- `curl` or `wget`
- `jq` (optional, falls back to grep/sed)

## License

MIT
