use chrono::{DateTime, Datelike, FixedOffset, Timelike, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use zellij_tile::prelude::*;

// --- Configuration ---

/// Clock display mode
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum ClockMode {
    #[default]
    Auto,    // Default: 12-hour (env vars unavailable in WASM)
    Hour12,
    Hour24,
    Off,
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
        matches!(self, ClockMode::Hour24)
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
    None,
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
    Auto,  // Default: US format (env vars unavailable in WASM)
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
        matches!(self, DateFormat::Auto | DateFormat::US)
    }

    fn is_iso(&self) -> bool {
        matches!(self, DateFormat::ISO)
    }
}

// --- Data Structures ---

/// Usage data for a single time window
#[derive(Debug, Clone, Default, Deserialize)]
struct WindowUsage {
    utilization: f64,
    resets_at: Option<String>,
}

/// Extra usage (overages) data
#[derive(Debug, Clone, Default, Deserialize)]
struct ExtraUsage {
    is_enabled: bool,
    monthly_limit: Option<f64>,
    used_credits: Option<f64>,
}

/// Full usage data from claude-usage CLI
#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
struct UsageData {
    fetched_at: Option<String>,
    five_hour: WindowUsage,
    seven_day: WindowUsage,
    seven_day_sonnet: Option<WindowUsage>,
    extra_usage: Option<ExtraUsage>,
}

// --- Pace Calculation ---
// Ported from noah-statusline.js — graduated thresholds with urgency scaling

/// Arrow indicating pace direction: ↑ over pace, ↓ under pace
fn pace_arrow(utilization: f64, pace: Option<f64>) -> &'static str {
    match pace {
        None => "",
        Some(p) => {
            if utilization > p + 0.5 { "\u{2191}" }      // ↑
            else if utilization < p - 0.5 { "\u{2193}" }  // ↓
            else { "" }
        }
    }
}

/// 5h color: green at/under pace, yellow +5%, orange +10%, blinking red +15%
fn pace_color_5h(utilization: f64, pace: Option<f64>) -> &'static str {
    match pace {
        None => "\x1b[32m",
        Some(p) if utilization <= p => "\x1b[32m",
        Some(p) => {
            let delta = utilization - p;
            if delta <= 5.0 { "\x1b[33m" }          // yellow
            else if delta <= 10.0 { "\x1b[38;5;208m" } // orange
            else { "\x1b[5;31m" }                    // blinking red
        }
    }
}

/// 7d color: urgency scales with remaining time via sqrt.
/// Same delta feels worse when there's less time to correct course.
fn pace_color_7d(utilization: f64, pace: Option<f64>) -> &'static str {
    match pace {
        None => "\x1b[32m",
        Some(p) => {
            let delta = (utilization - p).abs();
            let remaining = ((100.0 - p) / 100.0).max(0.05);
            let urgency = delta / remaining.sqrt();
            if urgency <= 4.0 { "\x1b[32m" }           // green
            else if urgency <= 10.0 { "\x1b[33m" }     // yellow
            else if urgency <= 18.0 { "\x1b[38;5;208m" } // orange
            else { "\x1b[5;31m" }                       // blinking red
        }
    }
}

// --- Display Modes ---

#[derive(Debug, Clone, Copy, PartialEq)]
enum DisplayMode {
    Hidden,   // < 18 chars
    Minimal,  // 18-29: "5h:45%↑ 7d:12%↓"
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
    cols: usize,
    palette: Option<Palette>,
    usage: Option<UsageData>,
    data_file: String,
    pending_read: bool,
    tz_offset_secs: i32, // UTC offset from host `date +%z`, 0 = UTC fallback

    clock_mode: ClockMode,
    suffix_style: SuffixStyle,
    date_format: DateFormat,
}

const ANSI_RESET: &str = "\x1b[0m";

impl ClaudeBar {
    /// Get current time adjusted for host timezone offset
    fn local_now(&self) -> DateTime<FixedOffset> {
        let offset = FixedOffset::east_opt(self.tz_offset_secs).unwrap_or(FixedOffset::east_opt(0).unwrap());
        Utc::now().with_timezone(&offset)
    }

    /// Request timezone offset from host
    fn request_tz_refresh(&self) {
        let ctx = BTreeMap::from([("source".to_string(), "tz_read".to_string())]);
        run_command_with_env_variables_and_cwd(
            &["/bin/date", "+%z"],
            BTreeMap::new(),
            PathBuf::from("/tmp"),
            ctx,
        );
    }

    /// Parse `date +%z` output (e.g., "-0400") into seconds
    fn parse_tz_offset(s: &str) -> Option<i32> {
        let s = s.trim();
        if s.len() < 5 { return None; }
        let sign = if s.starts_with('-') { -1 } else { 1 };
        let hours: i32 = s[1..3].parse().ok()?;
        let mins: i32 = s[3..5].parse().ok()?;
        Some(sign * (hours * 3600 + mins * 60))
    }

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

    /// Build a progress bar string with a given ANSI color
    fn progress_bar(&self, percent: f64, width: usize, color: &str) -> String {
        let filled = ((percent / 100.0) * width as f64).round() as usize;
        let filled = filled.min(width);
        let empty = width - filled;

        let dim = "\x1b[90m";

        let filled_chars: String = std::iter::repeat('\u{2588}').take(filled).collect();
        let empty_chars: String = std::iter::repeat('\u{2591}').take(empty).collect();

        format!("{}{}{}{}{}{}", color, filled_chars, ANSI_RESET, dim, empty_chars, ANSI_RESET)
    }

    /// Format clock based on available space and configuration
    /// Returns (display_string, visible_length)
    fn format_clock(&self, mode: DisplayMode) -> (String, usize) {
        if self.clock_mode.is_off() {
            return ("".to_string(), 0);
        }

        let now = self.local_now();
        let use_24h = self.clock_mode.use_24h();

        let (hour_display, suffix) = if use_24h {
            (now.hour(), "".to_string())
        } else {
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

        match mode {
            DisplayMode::Hidden => ("".to_string(), 0),

            DisplayMode::Minimal | DisplayMode::Compact => {
                let s = format!("{}:{:02}{}", hour_display, minute, suffix);
                let len = s.len();
                (s, len)
            }

            DisplayMode::Medium => {
                let date_str = if self.date_format.is_iso() {
                    format!("{}-{:02}-{:02}", now.year(), now.month(), now.day())
                } else if self.date_format.use_us_format() {
                    format!("{}/{}", now.month(), now.day())
                } else {
                    format!("{}/{}", now.day(), now.month())
                };
                let s = format!(
                    "{}:{:02}{} {}{}{}",
                    hour_display, minute, suffix, dim, date_str, ANSI_RESET
                );
                let len = format!("{}:{:02}{} {}", hour_display, minute, suffix, date_str).len();
                (s, len)
            }

            DisplayMode::Full => {
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
                    hour_display, minute, suffix, dim, date_part, ANSI_RESET
                );
                let len = format!("{}:{:02}{} {}", hour_display, minute, suffix, date_part).len();
                (s, len)
            }
        }
    }

    /// Request to read the usage data file
    fn request_data_refresh(&self) {
        let ctx = BTreeMap::from([("source".to_string(), "usage_read".to_string())]);
        run_command_with_env_variables_and_cwd(
            &["/bin/cat", &self.data_file],
            BTreeMap::new(),
            PathBuf::from("/tmp"),
            ctx,
        );
    }

    /// Check if currently rate-limited (any window at 100%)
    fn is_rate_limited(&self) -> bool {
        match &self.usage {
            Some(u) => u.five_hour.utilization >= 100.0 || u.seven_day.utilization >= 100.0,
            None => false,
        }
    }

    /// Format extra usage (overages) display
    fn format_extra_usage(&self) -> Option<(String, usize)> {
        let usage = self.usage.as_ref()?;
        let extra = usage.extra_usage.as_ref()?;

        let monthly_limit = extra.monthly_limit.unwrap_or(0.0);
        let used_credits = extra.used_credits.unwrap_or(0.0);

        if !extra.is_enabled || monthly_limit <= 0.0 || !self.is_rate_limited() {
            return None;
        }

        let used = used_credits / 100.0;
        let limit = (monthly_limit / 100.0).round() as i64;
        let color = if used_credits >= monthly_limit {
            "\x1b[31m" // red
        } else {
            "\x1b[38;5;208m" // orange
        };

        let s = format!("{}\u{26a0} ${:.2}/${}{}", color, used, limit, ANSI_RESET);
        let len = format!("\u{26a0} ${:.2}/${}", used, limit).len();
        Some((s, len))
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

        let elapsed_5h = self.calc_period_elapsed(&usage.five_hour.resets_at, 5.0);
        let elapsed_7d = self.calc_period_elapsed(&usage.seven_day.resets_at, 168.0);

        let util_5h = usage.five_hour.utilization;
        let util_7d = usage.seven_day.utilization;

        let color_5h = pace_color_5h(util_5h, elapsed_5h);
        let color_7d = pace_color_7d(util_7d, elapsed_7d);
        let arrow_5h = pace_arrow(util_5h, elapsed_5h);
        let arrow_7d = pace_arrow(util_7d, elapsed_7d);

        match mode {
            DisplayMode::Hidden => ("".to_string(), 0),

            DisplayMode::Minimal => {
                // "5h:45%↑ 7d:12%↓"
                let s = format!(
                    "{}5h:{:.0}%{}{} {}7d:{:.0}%{}{}",
                    color_5h, util_5h, arrow_5h, ANSI_RESET,
                    color_7d, util_7d, arrow_7d, ANSI_RESET
                );
                let len = format!(
                    "5h:{:.0}%{} 7d:{:.0}%{}",
                    util_5h, arrow_5h, util_7d, arrow_7d
                ).len();
                (s, len)
            }

            DisplayMode::Compact => {
                // "5h ████░░░░ 7d █░░░░░░░"
                let bar_5h = self.progress_bar(util_5h, 6, color_5h);
                let bar_7d = self.progress_bar(util_7d, 6, color_7d);
                let s = format!("5h {} 7d {}", bar_5h, bar_7d);
                let len = 3 + 6 + 4 + 6; // "5h ██████ 7d ██████"
                (s, len)
            }

            DisplayMode::Medium => {
                // "5h: 45%↑ (2h30m) │ 7d: 12%↓ (4d)"
                let time_5h = self.format_time_until(&usage.five_hour.resets_at);
                let time_7d = self.format_time_until(&usage.seven_day.resets_at);

                let mut s = format!(
                    "5h: {}{:.0}%{}{} ({}) \u{2502} 7d: {}{:.0}%{}{} ({})",
                    color_5h, util_5h, arrow_5h, ANSI_RESET, time_5h,
                    color_7d, util_7d, arrow_7d, ANSI_RESET, time_7d
                );
                let mut len = format!(
                    "5h: {:.0}%{} ({}) \u{2502} 7d: {:.0}%{} ({})",
                    util_5h, arrow_5h, time_5h,
                    util_7d, arrow_7d, time_7d
                ).len();

                // Append extra usage if rate-limited
                if let Some((extra_s, extra_len)) = self.format_extra_usage() {
                    s = format!("{} \u{2502} {}", s, extra_s);
                    len += 3 + extra_len; // " │ " + extra
                }

                (s, len)
            }

            DisplayMode::Full => {
                // "5h: 45%↑ (50% elapsed) 2h30m │ 7d: 12%↓ (14% elapsed) 4d"
                let time_5h = self.format_time_until(&usage.five_hour.resets_at);
                let time_7d = self.format_time_until(&usage.seven_day.resets_at);
                let elapsed_5h_str = elapsed_5h.map(|e| format!("{:.0}%", e)).unwrap_or("?".to_string());
                let elapsed_7d_str = elapsed_7d.map(|e| format!("{:.0}%", e)).unwrap_or("?".to_string());

                let mut s = format!(
                    "5h: {}{:.0}%{}{} ({} elapsed) {} \u{2502} 7d: {}{:.0}%{}{} ({} elapsed) {}",
                    color_5h, util_5h, arrow_5h, ANSI_RESET, elapsed_5h_str, time_5h,
                    color_7d, util_7d, arrow_7d, ANSI_RESET, elapsed_7d_str, time_7d
                );
                let mut len = format!(
                    "5h: {:.0}%{} ({} elapsed) {} \u{2502} 7d: {:.0}%{} ({} elapsed) {}",
                    util_5h, arrow_5h, elapsed_5h_str, time_5h,
                    util_7d, arrow_7d, elapsed_7d_str, time_7d
                ).len();

                // Append extra usage if rate-limited
                if let Some((extra_s, extra_len)) = self.format_extra_usage() {
                    s = format!("{} \u{2502} {}", s, extra_s);
                    len += 3 + extra_len;
                }

                (s, len)
            }
        }
    }

    /// Format a minimal clock (just HH:MM) respecting 12/24h setting
    fn format_mini_clock(&self) -> (String, usize) {
        if self.clock_mode.is_off() {
            return ("".to_string(), 0);
        }

        let now = self.local_now();
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
            if cols >= 6 && !self.clock_mode.is_off() {
                let (mini_clock, _) = self.format_mini_clock();
                print!(" {}", mini_clock);
            }
            return;
        }

        let (usage_str, usage_len) = self.format_usage(mode);
        let (clock_str, clock_len) = self.format_clock(mode);

        if self.clock_mode.is_off() {
            print!(" {}", usage_str);
            return;
        }

        let content_width = usage_len + clock_len;
        let available = cols.saturating_sub(3);

        if content_width <= available {
            let padding = cols.saturating_sub(usage_len + clock_len + 2);
            let spaces: String = std::iter::repeat(' ').take(padding).collect();
            print!(" {}{}{} ", usage_str, spaces, clock_str);
        } else if usage_len + 7 <= cols {
            let (mini_clock, mini_len) = self.format_mini_clock();
            let padding = cols.saturating_sub(usage_len + mini_len + 2);
            let spaces: String = std::iter::repeat(' ').take(padding).collect();
            print!(" {}{}{} ", usage_str, spaces, mini_clock);
        } else {
            print!(" {}", usage_str);
        }
    }
}

impl ZellijPlugin for ClaudeBar {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::RunCommands,
        ]);

        subscribe(&[
            EventType::Timer,
            EventType::ModeUpdate,
            EventType::PermissionRequestResult,
            EventType::RunCommandResult,
        ]);

        let raw_path = configuration
            .get("data_file")
            .cloned()
            .unwrap_or_else(|| {
                if let Ok(state_home) = std::env::var("XDG_STATE_HOME") {
                    format!("{}/claude-usage/usage.json", state_home)
                } else if let Ok(home) = std::env::var("HOME") {
                    format!("{}/.local/state/claude-usage/usage.json", home)
                } else {
                    "/tmp/claude-usage.json".to_string()
                }
            });
        // Expand ~ since run_command doesn't go through a shell
        self.data_file = if raw_path.starts_with("~/") {
            if let Ok(home) = std::env::var("HOME") {
                format!("{}{}", home, &raw_path[1..])
            } else {
                raw_path
            }
        } else {
            raw_path
        };

        self.clock_mode = configuration
            .get("clock")
            .map(|s| ClockMode::from_str(s))
            .unwrap_or_default();

        self.suffix_style = configuration
            .get("suffix")
            .map(|s| SuffixStyle::from_str(s))
            .unwrap_or_default();

        self.date_format = configuration
            .get("date_format")
            .map(|s| DateFormat::from_str(s))
            .unwrap_or_default();

        set_timeout(1.0);

        self.pending_read = true;
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::Timer(_) => {
                set_timeout(10.0);
                self.request_data_refresh();
                self.request_tz_refresh(); // re-fetch for DST changes
                true
            }

            Event::ModeUpdate(mode_info) => {
                self.palette = Some(mode_info.style.colors.into());
                true
            }

            Event::PermissionRequestResult(_) => {
                self.request_data_refresh();
                self.request_tz_refresh();
                self.pending_read = false;
                true
            }

            Event::RunCommandResult(exit_code, stdout, _stderr, context) => {
                match context.get("source").map(|s| s.as_str()) {
                    Some("usage_read") => {
                        self.pending_read = false;
                        if exit_code == Some(0) {
                            if let Ok(data) = serde_json::from_slice::<UsageData>(&stdout) {
                                self.usage = Some(data);
                                return true;
                            }
                        }
                        false
                    }
                    Some("tz_read") => {
                        if exit_code == Some(0) {
                            if let Ok(s) = String::from_utf8(stdout) {
                                if let Some(offset) = Self::parse_tz_offset(&s) {
                                    if offset != self.tz_offset_secs {
                                        self.tz_offset_secs = offset;
                                        return true; // re-render with new offset
                                    }
                                }
                            }
                        }
                        false
                    }
                    _ => false,
                }
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
