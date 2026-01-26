use chrono::{DateTime, Datelike, Local, Timelike, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;
use zellij_tile::prelude::*;

// --- Locale Detection ---

/// Get the user's locale string from environment
fn get_locale() -> String {
    std::env::var("LC_TIME")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default()
}

/// Detect if we should use US date format (MM/DD) or international (DD/MM)
fn is_us_date_format() -> bool {
    get_locale().starts_with("en_US")
}

/// Detect if the locale typically uses 24-hour time
/// Most locales outside US/UK/Australia/etc use 24-hour
fn is_24h_locale() -> bool {
    let locale = get_locale();

    // Locales that typically use 12-hour time
    let twelve_hour_locales = [
        "en_US", "en_AU", "en_CA", "en_NZ", "en_PH",
        "es_US", "es_MX",
    ];

    // Check if locale starts with any 12-hour locale prefix
    !twelve_hour_locales.iter().any(|l| locale.starts_with(l))
}

// --- Configuration ---

/// Clock display mode
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum ClockMode {
    #[default]
    Auto,    // Detect from locale
    Hour12,  // Force 12-hour
    Hour24,  // Force 24-hour
    Off,     // No clock
}

impl ClockMode {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "12" | "12h" | "12-hour" => ClockMode::Hour12,
            "24" | "24h" | "24-hour" => ClockMode::Hour24,
            "off" | "none" | "false" => ClockMode::Off,
            _ => ClockMode::Auto,
        }
    }

    fn use_24h(&self) -> bool {
        match self {
            ClockMode::Auto => is_24h_locale(),
            ClockMode::Hour12 => false,
            ClockMode::Hour24 => true,
            ClockMode::Off => false,
        }
    }

    fn is_off(&self) -> bool {
        matches!(self, ClockMode::Off)
    }
}

/// AM/PM suffix style (only applies to 12-hour mode)
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum SuffixStyle {
    #[default]
    Short,   // "a" / "p"
    Long,    // "AM" / "PM"
    None,    // No suffix (not recommended for 12h but allowed)
}

impl SuffixStyle {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "long" | "full" | "am/pm" => SuffixStyle::Long,
            "none" | "off" | "false" => SuffixStyle::None,
            _ => SuffixStyle::Short,
        }
    }
}

/// Date format style
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum DateFormat {
    #[default]
    Auto,  // Detect from locale
    US,    // MM/DD, "Jan 27"
    Intl,  // DD/MM, "27 Jan"
    ISO,   // YYYY-MM-DD
}

impl DateFormat {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "us" | "american" | "mdy" => DateFormat::US,
            "intl" | "international" | "dmy" | "eu" => DateFormat::Intl,
            "iso" | "iso8601" | "ymd" => DateFormat::ISO,
            _ => DateFormat::Auto,
        }
    }

    fn use_us_format(&self) -> bool {
        match self {
            DateFormat::Auto => is_us_date_format(),
            DateFormat::US => true,
            DateFormat::Intl => false,
            DateFormat::ISO => false,
        }
    }

    fn is_iso(&self) -> bool {
        matches!(self, DateFormat::ISO)
    }
}

// --- Data Structures ---

/// Usage data for a single time window (5h or 7d)
#[derive(Debug, Clone, Default, Deserialize)]
struct WindowUsage {
    /// Current utilization as percentage (0-100)
    utilization: f64,
    /// When this window resets (ISO 8601 timestamp)
    resets_at: Option<String>,
}

/// Full usage data from claude-usage CLI
#[derive(Debug, Clone, Default, Deserialize)]
struct UsageData {
    /// When this data was fetched
    fetched_at: Option<String>,
    /// 5-hour window usage
    five_hour: WindowUsage,
    /// 7-day window usage
    seven_day: WindowUsage,
}

/// Pace status based on utilization vs elapsed time
#[derive(Debug, Clone, Copy, PartialEq)]
enum PaceStatus {
    /// Utilization roughly matches elapsed time - sustainable pace
    OnTrack,
    /// Utilization exceeds elapsed time - will exhaust before reset
    RunningHot,
    /// Utilization below elapsed time - capacity going unused
    Underutilizing,
    /// Cannot determine (no data or invalid timestamps)
    Unknown,
}

// --- Display Modes ---

#[derive(Debug, Clone, Copy, PartialEq)]
enum DisplayMode {
    Hidden,   // < 18 chars
    Minimal,  // 18-29: "5h:45% 7d:12%"
    Compact,  // 30-44: bars
    Medium,   // 45-69: with reset times
    Full,     // 70+: with elapsed percentage
}

impl DisplayMode {
    fn from_width(cols: usize) -> Self {
        match cols {
            0..=17 => DisplayMode::Hidden,
            18..=29 => DisplayMode::Minimal,
            30..=44 => DisplayMode::Compact,
            45..=69 => DisplayMode::Medium,
            _ => DisplayMode::Full,
        }
    }
}

// --- Plugin State ---

#[derive(Default)]
struct ClaudeBar {
    /// Current terminal width
    cols: usize,
    /// Theme palette from Zellij
    palette: Option<Palette>,
    /// Usage data from file
    usage: Option<UsageData>,
    /// Path to usage data file
    data_file: String,
    /// Context for identifying our command results
    pending_read: bool,

    // Configuration
    /// Clock display mode (auto/12h/24h/off)
    clock_mode: ClockMode,
    /// AM/PM suffix style (short/long/none)
    suffix_style: SuffixStyle,
    /// Date format (auto/us/intl/iso)
    date_format: DateFormat,
}

impl ClaudeBar {
    /// Calculate what percentage of the time window has elapsed
    fn calc_period_elapsed(&self, resets_at: &Option<String>, period_hours: f64) -> Option<f64> {
        let reset_str = resets_at.as_ref()?;
        let reset_time = DateTime::parse_from_rfc3339(reset_str).ok()?;
        let now = Utc::now();

        let secs_until_reset = (reset_time.timestamp() - now.timestamp()).max(0) as f64;
        let period_secs = period_hours * 3600.0;
        let elapsed_secs = period_secs - secs_until_reset;

        Some((elapsed_secs / period_secs * 100.0).clamp(0.0, 100.0))
    }

    /// Calculate pace status by comparing utilization to elapsed time
    fn calc_pace_status(&self, utilization: f64, period_elapsed: Option<f64>) -> PaceStatus {
        let elapsed = match period_elapsed {
            Some(e) => e,
            None => return PaceStatus::Unknown,
        };

        // If very early in period, be lenient
        if elapsed < 5.0 {
            return PaceStatus::OnTrack;
        }

        let ratio = utilization / elapsed;

        if ratio >= 0.85 && ratio <= 1.15 {
            PaceStatus::OnTrack
        } else if ratio > 1.15 {
            PaceStatus::RunningHot
        } else {
            PaceStatus::Underutilizing
        }
    }

    /// Format duration until reset as human-readable string
    fn format_time_until(&self, resets_at: &Option<String>) -> String {
        let reset_str = match resets_at.as_ref() {
            Some(s) => s,
            None => return "?".to_string(),
        };

        let reset_time = match DateTime::parse_from_rfc3339(reset_str) {
            Ok(t) => t,
            Err(_) => return "?".to_string(),
        };

        let now = Utc::now();
        let secs = (reset_time.timestamp() - now.timestamp()).max(0);

        if secs == 0 {
            return "now".to_string();
        }

        let days = secs / 86400;
        let hours = (secs % 86400) / 3600;
        let mins = (secs % 3600) / 60;

        if days > 0 {
            format!("{}d{}h", days, hours)
        } else if hours > 0 {
            format!("{}h{}m", hours, mins)
        } else {
            format!("{}m", mins)
        }
    }

    /// Get ANSI color code for a pace status
    fn status_ansi_color(&self, status: PaceStatus) -> &'static str {
        match status {
            PaceStatus::OnTrack => "\x1b[32m",        // Green
            PaceStatus::RunningHot => "\x1b[33m",    // Yellow
            PaceStatus::Underutilizing => "\x1b[31m", // Red (wasting capacity)
            PaceStatus::Unknown => "\x1b[37m",        // White/default
        }
    }

    /// Reset ANSI color
    fn ansi_reset(&self) -> &'static str {
        "\x1b[0m"
    }

    /// Build a progress bar string
    fn progress_bar(&self, percent: f64, width: usize, status: PaceStatus) -> String {
        let filled = ((percent / 100.0) * width as f64).round() as usize;
        let filled = filled.min(width);
        let empty = width - filled;

        let color = self.status_ansi_color(status);
        let reset = self.ansi_reset();
        let dim = "\x1b[90m"; // Dim gray for empty portion

        let filled_chars: String = std::iter::repeat('█').take(filled).collect();
        let empty_chars: String = std::iter::repeat('░').take(empty).collect();

        format!("{}{}{}{}{}{}", color, filled_chars, reset, dim, empty_chars, reset)
    }

    /// Format clock based on available space and configuration
    /// Returns (display_string, visible_length) - visible_length excludes ANSI codes
    fn format_clock(&self, mode: DisplayMode) -> (String, usize) {
        if self.clock_mode.is_off() {
            return ("".to_string(), 0);
        }

        let now = Local::now();
        let use_24h = self.clock_mode.use_24h();

        let (hour_display, suffix) = if use_24h {
            // 24-hour: no suffix needed
            (now.hour(), "".to_string())
        } else {
            // 12-hour: convert and add suffix
            let h = now.hour() % 12;
            let hour_12 = if h == 0 { 12 } else { h };
            let is_pm = now.hour() >= 12;

            let sfx = match self.suffix_style {
                SuffixStyle::Short => if is_pm { "p" } else { "a" }.to_string(),
                SuffixStyle::Long => if is_pm { " PM" } else { " AM" }.to_string(),
                SuffixStyle::None => "".to_string(),
            };
            (hour_12, sfx)
        };

        let minute = now.minute();
        let dim = "\x1b[90m";
        let reset = self.ansi_reset();

        match mode {
            DisplayMode::Hidden => ("".to_string(), 0),

            DisplayMode::Minimal => {
                // 12h: "10:43a" or 24h: "22:43"
                let s = format!("{}:{:02}{}", hour_display, minute, suffix);
                let len = s.len();
                (s, len)
            }

            DisplayMode::Compact => {
                // 12h: "10:43 AM" (or "10:43a" for short) or 24h: "22:43"
                let s = format!("{}:{:02}{}", hour_display, minute, suffix);
                let len = s.len();
                (s, len)
            }

            DisplayMode::Medium => {
                // "10:43a 1/27" or "22:43 27/1" or "22:43 2026-01-27"
                let date_str = if self.date_format.is_iso() {
                    format!("{}-{:02}-{:02}", now.year(), now.month(), now.day())
                } else if self.date_format.use_us_format() {
                    format!("{}/{}", now.month(), now.day())
                } else {
                    format!("{}/{}", now.day(), now.month())
                };
                let s = format!(
                    "{}:{:02}{} {}{}{}",
                    hour_display, minute, suffix, dim, date_str, reset
                );
                let len = format!("{}:{:02}{} {}", hour_display, minute, suffix, date_str).len();
                (s, len)
            }

            DisplayMode::Full => {
                // "10:43a Wed, Jan 27" or "22:43 Wed, 27 Jan" or "22:43 2026-01-27"
                let date_part = if self.date_format.is_iso() {
                    format!("{}-{:02}-{:02}", now.year(), now.month(), now.day())
                } else {
                    let weekday = match now.weekday() {
                        chrono::Weekday::Mon => "Mon",
                        chrono::Weekday::Tue => "Tue",
                        chrono::Weekday::Wed => "Wed",
                        chrono::Weekday::Thu => "Thu",
                        chrono::Weekday::Fri => "Fri",
                        chrono::Weekday::Sat => "Sat",
                        chrono::Weekday::Sun => "Sun",
                    };
                    let month = match now.month() {
                        1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr",
                        5 => "May", 6 => "Jun", 7 => "Jul", 8 => "Aug",
                        9 => "Sep", 10 => "Oct", 11 => "Nov", 12 => "Dec",
                        _ => "???",
                    };
                    let day = now.day();

                    if self.date_format.use_us_format() {
                        format!("{}, {} {}", weekday, month, day)
                    } else {
                        format!("{}, {} {}", weekday, day, month)
                    }
                };

                let s = format!(
                    "{}:{:02}{} {}{}{}",
                    hour_display, minute, suffix, dim, date_part, reset
                );
                let len = format!("{}:{:02}{} {}", hour_display, minute, suffix, date_part).len();
                (s, len)
            }
        }
    }

    /// Request to read the usage data file
    fn request_data_refresh(&self) {
        let cmd = vec!["cat".to_string(), self.data_file.clone()];
        let ctx = BTreeMap::from([("source".to_string(), "usage_read".to_string())]);
        run_command(&cmd, ctx);
    }

    /// Build usage display string and its visible length
    fn format_usage(&self, mode: DisplayMode) -> (String, usize) {
        let usage = match &self.usage {
            Some(u) => u,
            None => {
                let s = "Claude: --".to_string();
                let len = s.len();
                return (s, len);
            }
        };

        // Calculate derived values
        let elapsed_5h = self.calc_period_elapsed(&usage.five_hour.resets_at, 5.0);
        let elapsed_7d = self.calc_period_elapsed(&usage.seven_day.resets_at, 168.0);

        let status_5h = self.calc_pace_status(usage.five_hour.utilization, elapsed_5h);
        let status_7d = self.calc_pace_status(usage.seven_day.utilization, elapsed_7d);

        let color_5h = self.status_ansi_color(status_5h);
        let color_7d = self.status_ansi_color(status_7d);
        let reset = self.ansi_reset();

        match mode {
            DisplayMode::Hidden => ("".to_string(), 0),

            DisplayMode::Minimal => {
                // "5h:45% 7d:12%"
                let s = format!(
                    "{}5h:{:.0}%{} {}7d:{:.0}%{}",
                    color_5h, usage.five_hour.utilization, reset,
                    color_7d, usage.seven_day.utilization, reset
                );
                let len = format!(
                    "5h:{:.0}% 7d:{:.0}%",
                    usage.five_hour.utilization, usage.seven_day.utilization
                ).len();
                (s, len)
            }

            DisplayMode::Compact => {
                // "5h ████░░░░ 7d █░░░░░░░"
                let bar_5h = self.progress_bar(usage.five_hour.utilization, 6, status_5h);
                let bar_7d = self.progress_bar(usage.seven_day.utilization, 6, status_7d);
                let s = format!("5h {} 7d {}", bar_5h, bar_7d);
                // Visible: "5h ██████ 7d ██████" = 3 + 6 + 4 + 6 = 19
                let len = 3 + 6 + 4 + 6;
                (s, len)
            }

            DisplayMode::Medium => {
                // "5h: 45% (2h30m) │ 7d: 12% (4d)"
                let time_5h = self.format_time_until(&usage.five_hour.resets_at);
                let time_7d = self.format_time_until(&usage.seven_day.resets_at);
                let s = format!(
                    "5h: {}{:.0}%{} ({}) │ 7d: {}{:.0}%{} ({})",
                    color_5h, usage.five_hour.utilization, reset, time_5h,
                    color_7d, usage.seven_day.utilization, reset, time_7d
                );
                let len = format!(
                    "5h: {:.0}% ({}) │ 7d: {:.0}% ({})",
                    usage.five_hour.utilization, time_5h,
                    usage.seven_day.utilization, time_7d
                ).len();
                (s, len)
            }

            DisplayMode::Full => {
                // "5h: 45% (50% elapsed) 2h30m │ 7d: 12% (14% elapsed) 4d"
                let time_5h = self.format_time_until(&usage.five_hour.resets_at);
                let time_7d = self.format_time_until(&usage.seven_day.resets_at);
                let elapsed_5h_str = elapsed_5h.map(|e| format!("{:.0}%", e)).unwrap_or("?".to_string());
                let elapsed_7d_str = elapsed_7d.map(|e| format!("{:.0}%", e)).unwrap_or("?".to_string());
                let s = format!(
                    "5h: {}{:.0}%{} ({} elapsed) {} │ 7d: {}{:.0}%{} ({} elapsed) {}",
                    color_5h, usage.five_hour.utilization, reset, elapsed_5h_str, time_5h,
                    color_7d, usage.seven_day.utilization, reset, elapsed_7d_str, time_7d
                );
                let len = format!(
                    "5h: {:.0}% ({} elapsed) {} │ 7d: {:.0}% ({} elapsed) {}",
                    usage.five_hour.utilization, elapsed_5h_str, time_5h,
                    usage.seven_day.utilization, elapsed_7d_str, time_7d
                ).len();
                (s, len)
            }
        }
    }

    /// Format a minimal clock (just HH:MM) respecting 12/24h setting
    fn format_mini_clock(&self) -> (String, usize) {
        if self.clock_mode.is_off() {
            return ("".to_string(), 0);
        }

        let now = Local::now();
        let use_24h = self.clock_mode.use_24h();

        let hour = if use_24h {
            now.hour()
        } else {
            let h = now.hour() % 12;
            if h == 0 { 12 } else { h }
        };

        let s = format!("{}:{:02}", hour, now.minute());
        let len = s.len();
        (s, len)
    }

    /// Render the full bar: usage on left, clock on right
    fn render_content(&self, cols: usize) {
        let mode = DisplayMode::from_width(cols);

        if mode == DisplayMode::Hidden {
            // Even when hidden, show just the time if we have any space
            if cols >= 6 && !self.clock_mode.is_off() {
                let (mini_clock, _) = self.format_mini_clock();
                print!(" {}", mini_clock);
            }
            return;
        }

        let (usage_str, usage_len) = self.format_usage(mode);
        let (clock_str, clock_len) = self.format_clock(mode);

        // If clock is off, just show usage
        if self.clock_mode.is_off() {
            print!(" {}", usage_str);
            return;
        }

        // Layout: " {usage} ... {clock} "
        // We need 1 char padding on each side, plus at least 1 space between
        let content_width = usage_len + clock_len;
        let available = cols.saturating_sub(3); // 2 edge padding + 1 min separator

        if content_width <= available {
            // Both fit - add padding between
            let padding = cols.saturating_sub(usage_len + clock_len + 2);
            let spaces: String = std::iter::repeat(' ').take(padding).collect();
            print!(" {}{}{} ", usage_str, spaces, clock_str);
        } else if usage_len + 7 <= cols {
            // Clock doesn't fit at full size, show minimal time on right
            let (mini_clock, mini_len) = self.format_mini_clock();
            let padding = cols.saturating_sub(usage_len + mini_len + 2);
            let spaces: String = std::iter::repeat(' ').take(padding).collect();
            print!(" {}{}{} ", usage_str, spaces, mini_clock);
        } else {
            // Only usage fits
            print!(" {}", usage_str);
        }
    }
}

impl ZellijPlugin for ClaudeBar {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        // Request permissions
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::RunCommands,
        ]);

        // Subscribe to events
        subscribe(&[
            EventType::Timer,
            EventType::ModeUpdate,
            EventType::PermissionRequestResult,
            EventType::RunCommandResult,
        ]);

        // Determine data file path
        self.data_file = configuration
            .get("data_file")
            .cloned()
            .unwrap_or_else(|| {
                // Default path matching claude-usage CLI output
                if let Ok(state_home) = std::env::var("XDG_STATE_HOME") {
                    format!("{}/claude-usage/usage.json", state_home)
                } else if let Ok(home) = std::env::var("HOME") {
                    format!("{}/.local/state/claude-usage/usage.json", home)
                } else {
                    "/tmp/claude-usage.json".to_string()
                }
            });

        // Parse clock configuration
        // clock = "auto" | "12h" | "24h" | "off"
        self.clock_mode = configuration
            .get("clock")
            .map(|s| ClockMode::from_str(s))
            .unwrap_or_default();

        // suffix = "short" | "long" | "none" (only applies to 12h mode)
        self.suffix_style = configuration
            .get("suffix")
            .map(|s| SuffixStyle::from_str(s))
            .unwrap_or_default();

        // date_format = "auto" | "us" | "intl" | "iso"
        self.date_format = configuration
            .get("date_format")
            .map(|s| DateFormat::from_str(s))
            .unwrap_or_default();

        // Set up refresh timer (every 60 seconds - file updates every 5 min via cron)
        set_timeout(1.0); // Initial read after 1 second

        // Request initial data read
        self.pending_read = true;
        self.request_data_refresh();
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::Timer(_) => {
                // Refresh data periodically
                set_timeout(60.0);
                self.request_data_refresh();
                true // Re-render for clock/time updates
            }

            Event::ModeUpdate(mode_info) => {
                self.palette = Some(mode_info.style.colors);
                true
            }

            Event::PermissionRequestResult(_) => {
                // Re-request data read once permissions granted
                if self.pending_read {
                    self.request_data_refresh();
                }
                false
            }

            Event::RunCommandResult(exit_code, stdout, _stderr, context) => {
                // Check if this is our usage read
                if context.get("source").map(|s| s.as_str()) == Some("usage_read") {
                    self.pending_read = false;
                    if exit_code == Some(0) {
                        if let Ok(data) = serde_json::from_slice::<UsageData>(&stdout) {
                            self.usage = Some(data);
                            return true;
                        }
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
