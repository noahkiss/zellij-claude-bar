# zellij-claude-bar Development Guide

## Project Overview

A Zellij plugin (WASM) written in Rust that displays a single-line status bar with:
1. Claude API usage (5-hour and 7-day rate limits)
2. Pace indicators with graduated color thresholds and directional arrows
3. Extra usage (overages) display when rate-limited
4. Time until reset
5. Clock with date (right-aligned)

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
- Passes through the full API response (with `fetched_at` timestamp added)
- Appends to history log at `$XDG_STATE_HOME/claude-usage/history.jsonl`

**Features:**
- Full API response passthrough (with jq) — captures all windows and extra usage
- Credential discovery: `$CLAUDE_CONFIG_DIR` → `$XDG_CONFIG_HOME/claude` → `$HOME/.claude`
- Tool fallbacks: `jq` (full response) or `grep/sed` (core fields only), `curl` → `wget`
- POSIX-compatible (works on macOS and Linux)
- History logging: each fetch appends a compact JSON line for later analysis

**Output files:**
- `usage.json` - Current state (plugin reads this)
- `history.jsonl` - Append-only log (one JSON object per line)

**JSON format (with jq — full passthrough):**
```json
{
  "five_hour": {
    "utilization": 55.0,
    "resets_at": "2026-01-26T06:59:59.819279+00:00"
  },
  "seven_day": {
    "utilization": 71.0,
    "resets_at": "2026-01-29T15:59:59.819305+00:00"
  },
  "seven_day_sonnet": {
    "utilization": 2.0,
    "resets_at": "2026-01-29T15:59:59.819305+00:00"
  },
  "fable": {
    "utilization": 10.0,
    "resets_at": "2026-01-29T15:59:59.819305+00:00"
  },
  "extra_usage": {
    "is_enabled": true,
    "monthly_limit": 4000,
    "used_credits": 2725.0,
    "utilization": 68.125
  },
  "fetched_at": "2026-01-26T04:24:42Z"
}
```

### Plugin: `src/main.rs`

Implements `ZellijPlugin` trait:
- `load()`: Request permissions, subscribe to events, start timer
- `update()`: Handle timer, mode updates, command results
- `render()`: Draw status bar with responsive layout

**Permissions needed:**
- `ReadApplicationState`: Access theme palette
- `RunCommands`: Read JSON file via `cat`

**Events used:**
- `Timer`: Periodic refresh (10s)
- `ModeUpdate`: Theme palette updates
- `RunCommandResult`: JSON file read results

## Display Modes

The plugin adapts to terminal width:

| Width | Mode | Usage Display | Clock Display |
|-------|------|---------------|---------------|
| < 6 | hidden | -- | -- |
| 6-17 | hidden | -- | `10:43` |
| 18-29 | minimal | `5h:45%↑ 7d:12%↓` | `10:43a` |
| 30-44 | compact | `5h ████░░ 7d ██░░░░` | `10:43 AM` |
| 45-69 | medium | `5h: 45%↑ (2h30m) │ 7d: 12%↓ (4d)` | `10:43 AM 1/27` |
| 70+ | full | `5h: 45%↑ (50% elapsed) 2h30m │ ...` | `10:43 AM Wed, Jan 27` |

**Layout:** Usage left-aligned, clock right-aligned, space-padded between.

When rate-limited with extra usage enabled, medium/full modes append: `│ ⚠ $27.25/$40`

## Pace Coloring

Graduated color thresholds ported from noah-statusline.js:

### 5-hour window
| Delta from pace | Color | Meaning |
|----------------|-------|---------|
| At or under | Green | On track |
| +5% | Yellow | Slightly over |
| +10% | Orange | Over pace |
| +15% | Blinking red | Well over |

### 7-day window (directional)
Urgency scales with remaining time via `sqrt(remaining)` — the same drift matters more as the window closes. The weekly pool is use-it-or-lose-it, so **over** and **under** pace mean opposite things and get opposite color languages: warm = burning too fast (ease off), pastel cool = headroom going to waste (lean in). Pastels are light-value so they stay legible on dark themes.

| Direction | Urgency score | Color | Meaning |
|-----------|--------------|-------|---------|
| either | ≤ 4 | Green | On pace |
| over | ≤ 10 | Yellow | Drifting over |
| over | ≤ 18 | Orange | Needs correction |
| over | > 18 | Blinking red | Well over |
| under | ≤ 10 | Pastel pink | A little headroom |
| under | ≤ 18 | Pastel violet | Lots of headroom |
| under | > 18 | Bold pastel violet | Way under — use it or lose it |

### Per-model segments (Sonnet + Fable)
When `seven_day_sonnet` / `fable` are present in `usage.json`, medium/full modes append labeled, pace-colored segments: `│ S:23%↓ │ F:11%↓`. Both share the 7d weekly reset (no separate timer). The `%` uses the directional 7d coloring above; only the label is tinted (`S:` dim, `F:` blue) to tell the models apart. Absent/null fields render nothing.

### Pace arrows
- ↑ shown when utilization exceeds period elapsed (over pace)
- ↓ shown when utilization is below period elapsed (under pace)

## Configuration Options

All options can be set in the Zellij layout plugin config:

```kdl
plugin location="file:/path/to/zellij_claude_bar.wasm" {
    data_file "/home/user/.local/state/claude-usage/usage.json"  // REQUIRED: absolute path
    clock "12h"         // "12h" | "24h" | "off" (auto = 12h)
    suffix "short"      // "short" (a/p) | "long" (AM/PM) | "none"
    date_format "us"    // "us" | "intl" | "iso" (auto = us)
}
```

| Option | Values | Default | Description |
|--------|--------|---------|-------------|
| `data_file` | absolute path | - | **Required.** WASM plugins cannot access env vars |
| `clock` | 12h/24h/off | 12h | Clock format |
| `suffix` | short/long/none | short | AM/PM style: "a/p", "AM/PM", or none |
| `date_format` | us/intl/iso | us | Date ordering |

**Note:** `auto` is accepted for `clock` and `date_format` but defaults to 12h/US since WASM plugins cannot read host locale environment variables.

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

# If you have Homebrew Rust alongside rustup, use:
RUSTC=~/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc ~/.cargo/bin/cargo build --release
```

### 4. Configure Zellij

Add to your Zellij layout:
```kdl
pane size=1 borderless=true {
    plugin location="file:~/.config/zellij/plugins/zellij_claude_bar.wasm" {
        data_file "/home/YOUR_USER/.local/state/claude-usage/usage.json"
    }
}
```

On first launch, grant the `RunCommands` permission when prompted.

## Development Workflow

1. Edit source in `src/main.rs`
2. Build: `RUSTC=~/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc ~/.cargo/bin/cargo build --release`
3. Deploy: `cp target/wasm32-wasip1/release/zellij-claude-bar.wasm ~/.config/zellij/plugins/zellij_claude_bar.wasm`
4. Test: Start a **new** zellij session (or `zellij ka` + relaunch). Hot-reload via `zellij action start-or-reload-plugin` has limitations (see below).

### Build notes
- Cargo produces `zellij-claude-bar.wasm` (hyphens). The installed filename uses underscores (`zellij_claude_bar.wasm`). Keep these consistent.
- If Homebrew Rust conflicts with rustup, use the explicit `RUSTC=` prefix shown above.

## Zellij Plugin API Reference (v0.43)

### run_command vs run_command_with_env_variables_and_cwd

**Always use `run_command_with_env_variables_and_cwd` with an absolute CWD path.**

`run_command()` in `zellij-tile/src/shim.rs` passes `PathBuf::from(".")` as CWD. The server (`zellij_exports.rs`) joins this with `env.plugin_cwd`: `let cwd = env.plugin_cwd.join(cwd)`. Then `std::process::Command::new(command).current_dir(cwd).output()` runs in `background_jobs.rs`.

If `plugin_cwd` points to a non-existent directory, **all** `run_command()` calls fail with `ENOENT ("No such file or directory")` — even if the binary path is absolute.

Fix: use `run_command_with_env_variables_and_cwd` with `PathBuf::from("/tmp")`. Since Rust's `PathBuf::join()` replaces the base when given an absolute path, this bypasses whatever `plugin_cwd` is set to.

```rust
use std::path::PathBuf;

run_command_with_env_variables_and_cwd(
    &["/bin/cat", &self.data_file],
    BTreeMap::new(),
    PathBuf::from("/tmp"),
    context,
);
```

### plugin_cwd

`plugin_cwd` is set in `plugin_loader.rs` when the plugin is created. For layout-loaded plugins, it comes from the session/tab CWD. It's stored in `PluginEnv` and does not update dynamically. If the original CWD directory is deleted or doesn't exist, all `run_command()` calls silently fail.

### WASI Sandbox

Plugins run in a WASI sandbox. Key restrictions:
- `std::fs::read_to_string()` **cannot** read host files — only pre-opened WASI directories are accessible
- Pre-opened dirs: `/host` (maps to `plugin_cwd`), `/data` (plugin data dir), `/cache`, `/tmp`
- `std::env::var("HOME")` **does not work** — zellij only injects `CLICOLOR_FORCE=1` via `WasiCtxBuilder::env()`. No HOME, PATH, XDG vars, etc.
- To read host files, use `run_command` with `cat` (or similar) — the command runs on the host, not in WASI

### Permissions

Permissions are cached in `~/Library/Caches/org.Zellij-Contributors.Zellij/permissions.kdl` on macOS (Linux: may vary, check `ZELLIJ_CACHE_DIR`). Format:
```kdl
"/path/to/plugin.wasm" {
    RunCommands
    ReadApplicationState
}
```

- Keyed by **plugin file path** (not hash). Recompiling the plugin keeps the same permissions as long as the path doesn't change.
- `_allow_exec_host_cmd true` in layout config is a legacy mechanism. Modern plugins should use `request_permission` + `PermissionRequestResult` event.
- If permissions seem stuck: delete the relevant entry from `permissions.kdl` and restart the session.

### Plugin Reload / Hot Reload

`zellij action start-or-reload-plugin file:path/to/plugin.wasm`:
- Reloads the plugin from the given path
- **Does NOT pass layout configuration** (`_allow_exec_host_cmd`, `data_file`, etc.) — the plugin gets an empty `BTreeMap` in `load()`
- Supports `-c key=value` flag for passing config, but you must pass ALL config keys manually
- Permission cache may not be honored on reload — may require re-granting

**Recommended dev workflow**: `zellij ka` + fresh session for reliable testing. Hot-reload is useful for quick iteration but unreliable for permission/config-dependent plugins.

### WASM Cache

Zellij caches compiled WASM modules in `~/Library/Caches/zellij/` by content hash. To force recompilation:
```bash
# Clear the WASM cache
rm -f ~/Library/Caches/zellij/*.wasm 2>/dev/null
# Or just replace the plugin file — hash change triggers recompile
```

### Alternative Data Ingestion: zellij pipe

Instead of `run_command` + `cat`, plugins can receive data via `zellij pipe`:
```bash
echo '{"data":"here"}' | zellij pipe --plugin file:path/to/plugin.wasm
```
The plugin receives this as a `PipeMessage` event. This avoids CWD issues entirely but requires an external process to push data.

### Events Used by This Plugin

| Event | Purpose |
|-------|---------|
| `Timer` | Periodic refresh (10s) |
| `ModeUpdate` | Theme palette updates |
| `PermissionRequestResult` | Triggers initial data fetch after permissions granted |
| `RunCommandResult` | Receives output from `cat` (usage JSON) and `date` (timezone) |

## File Structure

```
zellij-claude-bar/
├── .cargo/
│   └── config.toml      # WASM build target config
├── bin/
│   └── claude-usage     # CLI tool to fetch usage data
├── src/
│   └── main.rs          # Zellij plugin (binary crate for _start export)
├── Cargo.toml           # Rust dependencies
├── README.md            # User documentation
├── CLAUDE.md            # Points to AGENTS.md
└── AGENTS.md            # This file
```

## Key Dependencies

- `zellij-tile` 0.43+: Zellij plugin SDK
- `chrono` 0.4: Time/date handling (with `clock` feature for Local timezone)
- `serde` + `serde_json`: JSON parsing

## Lessons Learned

Hard-won knowledge from debugging this plugin. Read before making changes.

### 1. WASI is a sandbox — your plugin code runs in a different world than the host

The plugin's Rust code runs inside a WASI sandbox. The host commands (`run_command`) run on the actual host. These are two completely separate execution environments:

- **Inside WASI (plugin code)**: No HOME, no PATH, no XDG vars. Zellij only injects `CLICOLOR_FORCE=1` via `WasiCtxBuilder::env()`. `std::env::var("HOME")` will fail. `std::fs::read_to_string()` can only see WASI-mounted dirs (`/host`, `/data`, `/cache`, `/tmp`), not the real filesystem.
- **On host (run_command)**: Full host environment. But paths are constructed inside the plugin, so if the plugin can't resolve `~` or env vars, the host command gets a broken path.

**Consequence**: Never use `~` or env vars in config paths. The layout config must use absolute paths because the plugin has no way to expand them. The `data_file` config looked correct (`"~/.local/state/..."`) but silently broke because the tilde expansion fallback in `load()` depends on HOME which doesn't exist in WASI.

### 2. run_command CWD is a landmine

`run_command()` defaults to `PathBuf::from(".")` as CWD, which the server resolves to `env.plugin_cwd`. This is set once at plugin load time from the session/tab CWD and never updates. If that directory gets deleted, moved, or never existed, every single `run_command()` call fails with ENOENT — even with absolute binary paths — because `std::process::Command::current_dir()` validates the CWD before exec.

**Rule**: Always use `run_command_with_env_variables_and_cwd` with `PathBuf::from("/tmp")` (or another guaranteed-to-exist absolute path). Rust's `PathBuf::join()` replaces the base when given an absolute path, so this overrides whatever `plugin_cwd` is set to.

### 3. Errors are silent by default

When `run_command` fails, you get a `RunCommandResult` with a non-zero exit code and empty stdout. There's no log, no crash, no visible error. The plugin just keeps rendering with stale/empty data. This makes debugging extremely difficult — the bar shows `Claude: --` with no indication of why.

**Tip**: During development, add temporary logging in the `RunCommandResult` handler to surface stderr and exit codes. Check the zellij log (`/tmp/zellij-*/zellij.log`) for plugin errors.

### 4. Permission flow is async — don't fire commands in load()

`request_permission()` is async. If you call `run_command` in `load()`, permissions haven't been granted yet and the command silently fails. Subscribe to `PermissionRequestResult` and fire initial commands there.

### 5. The API response schema changes

Anthropic's `/api/oauth/usage` endpoint returns `null` for fields when features are disabled (e.g., `monthly_limit: null` and `used_credits: null` when extra usage is off). Use `Option<f64>` (not `f64`) for any numeric field that could be null. Serde will fail to deserialize the entire response if a single `null` hits a non-Option field.

Also use `#[serde(default)]` or `Option<T>` for top-level window fields — new windows like `seven_day_oauth_apps`, `seven_day_opus`, `seven_day_cowork`, `iguana_necktie` may appear as `null` at any time.

### 6. Hot reload loses config

`zellij action start-or-reload-plugin` does NOT pass layout configuration. The plugin's `load()` receives an empty `BTreeMap`. This means `data_file` is empty, clock settings are defaults, etc. For reliable testing, always use `zellij ka` + fresh session. If you must hot-reload, pass config via `-c key=value` flags.

### 7. Binary naming mismatch

Cargo produces `zellij-claude-bar.wasm` (hyphens from the crate name). The installed location uses `zellij_claude_bar.wasm` (underscores). If you copy the wrong file or an old build artifact, the plugin may load a stale version with no error. Always verify you're copying from the correct build output path: `target/wasm32-wasip1/release/zellij-claude-bar.wasm`.
