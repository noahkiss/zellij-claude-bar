# zellij-claude-bar Development Guide

## Project Overview

A Zellij plugin (WASM) written in Rust that displays a single-line status bar with:
1. Claude API usage (5-hour and 7-day rate limits)
2. Pace indicators (on track / running hot / underutilizing)
3. Time until reset
4. Clock with date (right-aligned)

## Architecture

### Components

```
┌─────────────────────────────────────────────────────────────────┐
│                        User's System                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐     cron      ┌─────────────────────────┐     │
│  │ claude-usage │ ──(5 min)───► │ ~/.local/state/         │     │
│  │ (shell CLI)  │               │ claude-usage/usage.json │     │
│  └──────────────┘               └───────────┬─────────────┘     │
│         │                                   │                    │
│         │ fetches                           │ reads              │
│         ▼                                   ▼                    │
│  ┌──────────────────┐              ┌────────────────────┐       │
│  │ Anthropic API    │              │ Zellij Plugin      │       │
│  │ /api/oauth/usage │              │ (WASM)             │       │
│  └──────────────────┘              └────────────────────┘       │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### CLI Tool: `bin/claude-usage`

A POSIX shell script that:
- Fetches usage from `https://api.anthropic.com/api/oauth/usage`
- Uses OAuth token from Claude credentials file
- Writes JSON to `$XDG_STATE_HOME/claude-usage/usage.json`

**Features:**
- Credential discovery: `$CLAUDE_CONFIG_DIR` → `$XDG_CONFIG_HOME/claude` → `$HOME/.claude`
- Tool fallbacks: `jq` → `grep/sed`, `curl` → `wget`
- POSIX-compatible (works on macOS and Linux)

**Output format:**
```json
{
  "fetched_at": "2026-01-26T04:24:42Z",
  "five_hour": {
    "utilization": 55.0,
    "resets_at": "2026-01-26T06:59:59.819279+00:00"
  },
  "seven_day": {
    "utilization": 71.0,
    "resets_at": "2026-01-29T15:59:59.819305+00:00"
  }
}
```

### Plugin: `src/lib.rs`

Implements `ZellijPlugin` trait:
- `load()`: Request permissions, subscribe to events, start timer
- `update()`: Handle timer, mode updates, command results
- `render()`: Draw status bar with responsive layout

**Permissions needed:**
- `ReadApplicationState`: Access theme palette
- `RunCommands`: Read JSON file via `cat`

**Events used:**
- `Timer`: Periodic refresh (60s)
- `ModeUpdate`: Theme palette updates
- `RunCommandResult`: JSON file read results

## Display Modes

The plugin adapts to terminal width:

| Width | Mode | Usage Display | Clock Display |
|-------|------|---------------|---------------|
| < 6 | hidden | -- | -- |
| 6-17 | hidden | -- | `10:43` |
| 18-29 | minimal | `5h:45% 7d:12%` | `10:43a` |
| 30-44 | compact | `5h ████░░ 7d ██░░░░` | `10:43 AM` |
| 45-69 | medium | `5h: 45% (2h30m) │ 7d: 12% (4d)` | `10:43 AM 1/27` |
| 70+ | full | `5h: 45% (50% elapsed) 2h30m │ ...` | `10:43 AM Wed, Jan 27` |

**Layout:** Usage left-aligned, clock right-aligned, space-padded between.

## Color Coding (Pace Status)

Colors indicate whether usage pace is sustainable:

| Status | Color | Meaning |
|--------|-------|---------|
| On Track | Green | Utilization ≈ elapsed time (±15%) |
| Running Hot | Yellow | Utilization > elapsed - will exhaust early |
| Underutilizing | Red | Utilization < elapsed - wasting capacity |
| Unknown | White | No data or invalid timestamps |

**Calculation:** `ratio = utilization% / period_elapsed%`
- ratio 0.85-1.15 → On Track
- ratio > 1.15 → Running Hot
- ratio < 0.85 → Underutilizing

## Configuration Options

All options can be set in the Zellij layout plugin config:

```kdl
plugin location="file:/path/to/zellij_claude_bar.wasm" {
    data_file "/custom/path/to/usage.json"  // default: auto-detected
    clock "auto"      // "auto" | "12h" | "24h" | "off"
    suffix "short"    // "short" (a/p) | "long" (AM/PM) | "none"
    date_format "auto"  // "auto" | "us" | "intl" | "iso"
}
```

| Option | Values | Default | Description |
|--------|--------|---------|-------------|
| `data_file` | path | XDG/home detection | Path to usage JSON file |
| `clock` | auto/12h/24h/off | auto | Clock format (auto detects from locale) |
| `suffix` | short/long/none | short | AM/PM style: "a/p", "AM/PM", or none |
| `date_format` | auto/us/intl/iso | auto | Date ordering (auto detects from locale) |

## Locale Detection

When set to "auto", the plugin checks `LC_TIME`, `LC_ALL`, or `LANG`:

**Clock format:**
- 12-hour: `en_US`, `en_AU`, `en_CA`, `en_NZ`, `en_PH`, `es_US`, `es_MX`
- 24-hour: All other locales

**Date format:**
- US (MM/DD, "Jan 27"): `en_US*`
- International (DD/MM, "27 Jan"): All other locales
- ISO option: `2026-01-27` (year-month-day)

## Installation

### 1. Install the CLI tool

```bash
# Copy to PATH
cp bin/claude-usage ~/.local/bin/
chmod +x ~/.local/bin/claude-usage

# Test it
claude-usage -v
```

### 2. Set up cron

```bash
# Run every 5 minutes
crontab -e
*/5 * * * * ~/.local/bin/claude-usage >/dev/null 2>&1
```

### 3. Build the plugin

```bash
# Install Rust if needed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add wasm32-wasip1

# Build
cargo build --release
```

### 4. Configure Zellij

Add to your Zellij layout:
```kdl
pane size=1 borderless=true {
    plugin location="file:/path/to/target/wasm32-wasip1/release/zellij_claude_bar.wasm"
}
```

## Development Workflow

1. Edit source in `src/lib.rs`
2. Build: `cargo build`
3. Test: `zellij action start-or-reload-plugin file:target/wasm32-wasip1/debug/zellij_claude_bar.wasm`

## File Structure

```
zellij-claude-bar/
├── .cargo/
│   └── config.toml      # WASM build target config
├── bin/
│   └── claude-usage     # CLI tool to fetch usage data
├── src/
│   └── lib.rs           # Zellij plugin
├── Cargo.toml           # Rust dependencies
├── README.md            # User documentation
├── CLAUDE.md            # Points to AGENTS.md
└── AGENTS.md            # This file
```

## Key Dependencies

- `zellij-tile` 0.43+: Zellij plugin SDK
- `chrono` 0.4: Time/date handling (with `clock` feature for Local timezone)
- `serde` + `serde_json`: JSON parsing
