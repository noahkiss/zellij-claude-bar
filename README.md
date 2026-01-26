# zellij-claude-bar

A Zellij status bar plugin that displays Claude API usage limits and projections.

## Features

- **Clock Display**: Shows current time in the status bar
- **5-Hour Limit Tracking**: Displays usage against the 5-hour rate limit window
- **7-Day Limit Tracking**: Displays usage against the 7-day rate limit window
- **Projection Indicators**: Color-coded status showing whether you're on track to use your full limit allocation:
  - **Green** (On Track): Projected to hit limit within 5% of reset time - optimal usage
  - **Orange** (Running Hot): Projected to hit limit before reset - may need to slow down
  - **Red** (Underutilizing): Won't hit limit before reset - capacity being wasted
- **Responsive Layout**: Adapts display based on terminal width, showing more detail on wider screens
- **Theme Integration**: Uses Zellij's theme colors for consistent styling

## Installation

### Prerequisites

- Rust with the `wasm32-wasip1` target:
  ```bash
  rustup target add wasm32-wasip1
  ```

### Building

```bash
cargo build --release
```

The compiled plugin will be at `target/wasm32-wasip1/release/zellij_claude_bar.wasm`.

### Installing

Copy the plugin to your Zellij plugins directory:

```bash
cp target/wasm32-wasip1/release/zellij_claude_bar.wasm ~/.config/zellij/plugins/
```

### Layout Configuration

Add the plugin to your Zellij layout (e.g., `~/.config/zellij/layouts/default.kdl`):

```kdl
layout {
    default_tab_template {
        // Tab bar
        pane size=1 borderless=true {
            plugin location="zellij:tab-bar"
        }
        // Claude bar - sits right under the tab bar
        pane size=1 borderless=true {
            plugin location="file:~/.config/zellij/plugins/zellij_claude_bar.wasm" {
                // Optional: path to a JSON file with limit data
                // limits_file "/path/to/claude-limits.json"
            }
        }
        // Main content
        children
        // Status bar
        pane size=1 borderless=true {
            plugin location="zellij:status-bar"
        }
    }
    tab
}
```

## Configuration

The plugin can be configured with the following options in your layout:

| Option | Description |
|--------|-------------|
| `limits_file` | Path to a JSON file containing limit data |

### Limits File Format

```json
{
  "usage_5h": 45,
  "limit_5h": 100,
  "reset_5h": 1706234567,
  "usage_7d": 200,
  "limit_7d": 500,
  "reset_7d": 1706834567
}
```

## Roadmap

- [ ] Automatic limit fetching from Claude API headers
- [ ] Usage history tracking for more accurate projections
- [ ] Clickable elements for detailed stats
- [ ] Notification when approaching limits

## License

MIT
