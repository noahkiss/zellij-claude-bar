use chrono::{Local, Timelike};
use serde::{Deserialize, Serialize};
use zellij_tile::prelude::*;

/// Represents Claude API usage limits and current state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LimitInfo {
    /// Current usage count for the 5-hour window
    usage_5h: u32,
    /// Maximum allowed for the 5-hour window
    limit_5h: u32,
    /// When the 5-hour window resets (unix timestamp)
    reset_5h: i64,
    /// Current usage count for the 7-day window
    usage_7d: u32,
    /// Maximum allowed for the 7-day window
    limit_7d: u32,
    /// When the 7-day window resets (unix timestamp)
    reset_7d: i64,
}

/// Status indicator for limit projection
#[derive(Debug, Clone, Copy, PartialEq)]
enum LimitStatus {
    /// Will hit limit close to reset time (within 5%) - optimal usage
    OnTrack,
    /// Will hit limit before reset - running hot
    RunningHot,
    /// Won't hit limit before reset - underutilizing
    Underutilizing,
    /// No data available
    Unknown,
}

/// Main plugin state
#[derive(Default)]
struct ClaudeBar {
    /// Current terminal width
    cols: usize,
    /// Theme palette from Zellij
    palette: Option<Palette>,
    /// Claude API limit information
    limits: Option<LimitInfo>,
    /// Whether we have permission to make web requests
    has_web_permission: bool,
    /// Path to limits file (configurable)
    limits_file: Option<String>,
}

impl ClaudeBar {
    /// Calculate limit status based on current usage and time remaining
    fn calculate_status(&self, usage: u32, limit: u32, reset_time: i64) -> LimitStatus {
        if limit == 0 {
            return LimitStatus::Unknown;
        }

        let now = Local::now().timestamp();
        let time_remaining = reset_time - now;
        if time_remaining <= 0 {
            return LimitStatus::Unknown;
        }

        // Calculate the rate of usage needed to hit the limit exactly at reset
        let remaining_capacity = limit.saturating_sub(usage);
        if remaining_capacity == 0 {
            return LimitStatus::RunningHot; // Already at limit
        }

        // Project when we'll hit the limit based on current usage rate
        // If usage is 0, we're definitely underutilizing
        if usage == 0 {
            return LimitStatus::Underutilizing;
        }

        // Estimate time to hit limit based on current rate
        // This is a simplification - real implementation would track usage history
        let usage_rate = usage as f64 / (time_remaining as f64).max(1.0);
        let time_to_limit = remaining_capacity as f64 / usage_rate.max(0.001);

        // Calculate what percentage of the window we'll use
        let projected_usage_ratio = time_to_limit / time_remaining as f64;

        if projected_usage_ratio >= 0.95 && projected_usage_ratio <= 1.05 {
            LimitStatus::OnTrack
        } else if projected_usage_ratio < 0.95 {
            LimitStatus::RunningHot
        } else {
            LimitStatus::Underutilizing
        }
    }

    /// Get the color index for a status (uses theme palette colors)
    /// Color indices map to theme colors:
    /// 0 = fg, 1 = green, 2 = yellow/orange, 3 = red
    fn status_color_index(&self, status: LimitStatus) -> usize {
        match status {
            LimitStatus::OnTrack => 1,       // green
            LimitStatus::RunningHot => 2,    // orange/yellow
            LimitStatus::Underutilizing => 3, // red
            LimitStatus::Unknown => 0,       // default fg
        }
    }

    /// Format a timestamp as a human-readable time
    fn format_reset_time(&self, timestamp: i64) -> String {
        let now = Local::now().timestamp();
        let diff = timestamp - now;

        if diff <= 0 {
            return "now".to_string();
        }

        let hours = diff / 3600;
        let mins = (diff % 3600) / 60;

        if hours > 0 {
            format!("{}h{}m", hours, mins)
        } else {
            format!("{}m", mins)
        }
    }

    /// Format the clock display
    fn format_clock(&self) -> String {
        let now = Local::now();
        format!("{:02}:{:02}", now.hour(), now.minute())
    }

    /// Render the status bar content based on available width
    fn render_content(&self, cols: usize) {
        let clock = self.format_clock();

        // Determine what to show based on width
        // Full: "HH:MM | 5h: 45/100 (reset 2h30m) | 7d: 200/500 (reset 3d5h)"
        // Medium: "HH:MM | 5h: 45/100 | 7d: 200/500"
        // Compact: "HH:MM | 45/100 | 200/500"
        // Minimal: "HH:MM"

        if cols < 20 {
            // Minimal - just clock
            print!(" {}", clock);
            return;
        }

        let mut output = format!(" {} ", clock);

        if let Some(ref limits) = self.limits {
            let status_5h = self.calculate_status(limits.usage_5h, limits.limit_5h, limits.reset_5h);
            let status_7d = self.calculate_status(limits.usage_7d, limits.limit_7d, limits.reset_7d);

            if cols >= 80 {
                // Full width - show everything
                let reset_5h = self.format_reset_time(limits.reset_5h);
                let reset_7d = self.format_reset_time(limits.reset_7d);

                output.push_str(&format!(
                    "| 5h: {}/{} (reset {}) | 7d: {}/{} (reset {})",
                    limits.usage_5h, limits.limit_5h, reset_5h,
                    limits.usage_7d, limits.limit_7d, reset_7d
                ));
            } else if cols >= 50 {
                // Medium width - usage without reset times
                output.push_str(&format!(
                    "| 5h: {}/{} | 7d: {}/{}",
                    limits.usage_5h, limits.limit_5h,
                    limits.usage_7d, limits.limit_7d
                ));
            } else if cols >= 30 {
                // Compact - just ratios
                output.push_str(&format!(
                    "| {}/{} | {}/{}",
                    limits.usage_5h, limits.limit_5h,
                    limits.usage_7d, limits.limit_7d
                ));
            }

            // For now, print plain text - color support requires more complex rendering
            // TODO: Use print_text_with_coordinates with color_range for colored output
            print!("{}", output);

            // Print status indicators with colors if we have limits
            if cols >= 30 {
                let indicator_5h = match status_5h {
                    LimitStatus::OnTrack => "[OK]",
                    LimitStatus::RunningHot => "[!]",
                    LimitStatus::Underutilizing => "[~]",
                    LimitStatus::Unknown => "[?]",
                };
                let indicator_7d = match status_7d {
                    LimitStatus::OnTrack => "[OK]",
                    LimitStatus::RunningHot => "[!]",
                    LimitStatus::Underutilizing => "[~]",
                    LimitStatus::Unknown => "[?]",
                };
                print!(" {} {}", indicator_5h, indicator_7d);
            }
        } else {
            // No limit data - show placeholder
            if cols >= 40 {
                output.push_str("| Claude limits: --");
            }
            print!("{}", output);
        }
    }

    /// Try to load limits from the configured file
    fn load_limits_from_file(&mut self) {
        if let Some(ref path) = self.limits_file {
            // Request file read - will receive result via FileSystemRead event
            // For now, we use a placeholder
            // In a real implementation, we'd use the file system events
        }
    }
}

impl ZellijPlugin for ClaudeBar {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        // Request permissions we need
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::WebAccess,
        ]);

        // Subscribe to events we care about
        subscribe(&[
            EventType::Timer,
            EventType::ModeUpdate,
            EventType::PermissionRequestResult,
            EventType::WebRequestResult,
        ]);

        // Load configuration
        if let Some(path) = configuration.get("limits_file") {
            self.limits_file = Some(path.clone());
        }

        // Set up periodic timer for clock updates (every 30 seconds)
        set_timeout(30.0);

        // TODO: Load initial limits from file or API
        // For demo purposes, set some placeholder limits
        self.limits = Some(LimitInfo {
            usage_5h: 0,
            limit_5h: 100,
            reset_5h: Local::now().timestamp() + 5 * 3600,
            usage_7d: 0,
            limit_7d: 500,
            reset_7d: Local::now().timestamp() + 7 * 24 * 3600,
        });
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::Timer(_) => {
                // Refresh timer for next update
                set_timeout(30.0);
                // TODO: Refresh limits from file/API
                true // Re-render
            }
            Event::ModeUpdate(mode_info) => {
                // Capture the palette for themed colors
                self.palette = Some(mode_info.style.colors);
                true
            }
            Event::PermissionRequestResult(result) => {
                match result {
                    PermissionStatus::Granted => {
                        self.has_web_permission = true;
                    }
                    _ => {}
                }
                false
            }
            Event::WebRequestResult(status, _headers, body, _context) => {
                // Parse limit data from API response
                if status == 200 {
                    if let Ok(limits) = serde_json::from_slice::<LimitInfo>(&body) {
                        self.limits = Some(limits);
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn render(&mut self, _rows: usize, cols: usize) {
        self.cols = cols;
        self.render_content(cols);
    }
}

register_plugin!(ClaudeBar);
