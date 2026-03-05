//! # Q-Shell Prompt Customization Engine
//!
//! Dynamic, context-aware prompt rendering for Q-Shell.
//! The prompt adapts based on:
//! - Current Silo context (name, color)
//! - Working object (Prism OID instead of directory)
//! - Git status (branch, dirty state)
//! - System health (Sentinel Q-Vibe score)
//! - Time, battery, network status
//!
//! Prompt format is defined via a template string with variables:
//! `{user}@{host} [{vibe}] {object} {git} ~> `

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// ANSI color codes for terminal styling.
#[derive(Debug, Clone, Copy)]
pub enum Color {
    Reset,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    Rgb(u8, u8, u8),
}

impl Color {
    /// Get ANSI escape code for foreground.
    pub fn fg_code(&self) -> String {
        match self {
            Color::Reset => String::from("\x1b[0m"),
            Color::Black => String::from("\x1b[30m"),
            Color::Red => String::from("\x1b[31m"),
            Color::Green => String::from("\x1b[32m"),
            Color::Yellow => String::from("\x1b[33m"),
            Color::Blue => String::from("\x1b[34m"),
            Color::Magenta => String::from("\x1b[35m"),
            Color::Cyan => String::from("\x1b[36m"),
            Color::White => String::from("\x1b[37m"),
            Color::BrightBlack => String::from("\x1b[90m"),
            Color::BrightRed => String::from("\x1b[91m"),
            Color::BrightGreen => String::from("\x1b[92m"),
            Color::BrightYellow => String::from("\x1b[93m"),
            Color::BrightBlue => String::from("\x1b[94m"),
            Color::BrightMagenta => String::from("\x1b[95m"),
            Color::BrightCyan => String::from("\x1b[96m"),
            Color::BrightWhite => String::from("\x1b[97m"),
            Color::Rgb(r, g, b) => alloc::format!("\x1b[38;2;{};{};{}m", r, g, b),
        }
    }
}

/// A styled text segment.
#[derive(Debug, Clone)]
pub struct StyledSegment {
    pub text: String,
    pub fg: Color,
    pub bold: bool,
}

impl StyledSegment {
    pub fn new(text: &str, fg: Color) -> Self {
        StyledSegment { text: String::from(text), fg, bold: false }
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    /// Render to ANSI string.
    pub fn render(&self) -> String {
        let mut out = String::new();
        if self.bold {
            out.push_str("\x1b[1m");
        }
        out.push_str(&self.fg.fg_code());
        out.push_str(&self.text);
        out.push_str("\x1b[0m");
        out
    }
}

/// Prompt variable — a dynamic value resolved at render time.
#[derive(Debug, Clone)]
pub enum PromptVar {
    /// Current user name
    User,
    /// Hostname
    Host,
    /// Current Prism object path / OID
    Object,
    /// Git branch (if in a version-controlled object)
    GitBranch,
    /// Git dirty indicator
    GitDirty,
    /// Sentinel Q-Vibe health score
    VibeScore,
    /// Current time (HH:MM)
    Time,
    /// Battery percentage
    Battery,
    /// Network status (online/offline/mesh)
    Network,
    /// Current Silo name
    SiloName,
    /// Exit code of last command
    LastExit,
    /// Literal text
    Literal(String),
    /// Newline
    Newline,
}

/// Prompt context — values resolved before rendering.
#[derive(Debug, Clone)]
pub struct PromptContext {
    pub user: String,
    pub host: String,
    pub object_path: String,
    pub git_branch: Option<String>,
    pub git_dirty: bool,
    pub vibe_score: u8,
    pub time: String,
    pub battery: Option<u8>,
    pub network: String,
    pub silo_name: String,
    pub last_exit: i32,
}

impl Default for PromptContext {
    fn default() -> Self {
        PromptContext {
            user: String::from("root"),
            host: String::from("qindows"),
            object_path: String::from("/"),
            git_branch: None,
            git_dirty: false,
            vibe_score: 100,
            time: String::from("00:00"),
            battery: None,
            network: String::from("mesh"),
            silo_name: String::from("master"),
            last_exit: 0,
        }
    }
}

/// A prompt theme (defines colors for each component).
#[derive(Debug, Clone)]
pub struct PromptTheme {
    pub user_color: Color,
    pub host_color: Color,
    pub object_color: Color,
    pub git_color: Color,
    pub git_dirty_color: Color,
    pub vibe_good_color: Color,
    pub vibe_warn_color: Color,
    pub vibe_bad_color: Color,
    pub separator_color: Color,
    pub arrow_color: Color,
    pub error_color: Color,
}

impl Default for PromptTheme {
    fn default() -> Self {
        PromptTheme {
            user_color: Color::BrightCyan,
            host_color: Color::BrightBlue,
            object_color: Color::BrightGreen,
            git_color: Color::BrightMagenta,
            git_dirty_color: Color::BrightYellow,
            vibe_good_color: Color::BrightGreen,
            vibe_warn_color: Color::BrightYellow,
            vibe_bad_color: Color::BrightRed,
            separator_color: Color::BrightBlack,
            arrow_color: Color::Rgb(6, 214, 160), // Qindows accent
            error_color: Color::BrightRed,
        }
    }
}

/// The Prompt Engine.
pub struct PromptEngine {
    /// Prompt template (list of variables to render)
    pub template: Vec<PromptVar>,
    /// Theme
    pub theme: PromptTheme,
    /// Right-side prompt template (optional)
    pub rprompt: Vec<PromptVar>,
    /// Enable powerline-style segments?
    pub powerline: bool,
    /// Transient prompt (simplified after execution)
    pub transient: bool,
}

impl PromptEngine {
    pub fn new() -> Self {
        PromptEngine {
            template: Self::default_template(),
            theme: PromptTheme::default(),
            rprompt: Self::default_rprompt(),
            powerline: false,
            transient: false,
        }
    }

    /// Default prompt template.
    fn default_template() -> Vec<PromptVar> {
        alloc::vec![
            PromptVar::User,
            PromptVar::Literal(String::from("@")),
            PromptVar::Host,
            PromptVar::Literal(String::from(" ")),
            PromptVar::Literal(String::from("[")),
            PromptVar::VibeScore,
            PromptVar::Literal(String::from("] ")),
            PromptVar::Object,
            PromptVar::Literal(String::from(" ")),
            PromptVar::GitBranch,
            PromptVar::GitDirty,
            PromptVar::Newline,
            PromptVar::Literal(String::from("~> ")),
        ]
    }

    /// Default right prompt.
    fn default_rprompt() -> Vec<PromptVar> {
        alloc::vec![
            PromptVar::Time,
            PromptVar::Literal(String::from(" ")),
            PromptVar::Network,
        ]
    }

    /// Render the prompt string.
    pub fn render(&self, ctx: &PromptContext) -> String {
        let mut segments = Vec::new();

        for var in &self.template {
            segments.push(self.resolve_var(var, ctx));
        }

        segments.iter()
            .map(|s| s.render())
            .collect::<Vec<String>>()
            .join("")
    }

    /// Render the right-side prompt.
    pub fn render_rprompt(&self, ctx: &PromptContext) -> String {
        self.rprompt.iter()
            .map(|var| self.resolve_var(var, ctx).render())
            .collect::<Vec<String>>()
            .join("")
    }

    /// Resolve a variable to a styled segment.
    fn resolve_var(&self, var: &PromptVar, ctx: &PromptContext) -> StyledSegment {
        match var {
            PromptVar::User => StyledSegment::new(&ctx.user, self.theme.user_color).bold(),
            PromptVar::Host => StyledSegment::new(&ctx.host, self.theme.host_color),
            PromptVar::Object => StyledSegment::new(&ctx.object_path, self.theme.object_color).bold(),
            PromptVar::GitBranch => {
                if let Some(ref branch) = ctx.git_branch {
                    StyledSegment::new(&alloc::format!(" {}", branch), self.theme.git_color)
                } else {
                    StyledSegment::new("", Color::Reset)
                }
            }
            PromptVar::GitDirty => {
                if ctx.git_dirty {
                    StyledSegment::new("*", self.theme.git_dirty_color).bold()
                } else {
                    StyledSegment::new("", Color::Reset)
                }
            }
            PromptVar::VibeScore => {
                let color = if ctx.vibe_score >= 80 {
                    self.theme.vibe_good_color
                } else if ctx.vibe_score >= 50 {
                    self.theme.vibe_warn_color
                } else {
                    self.theme.vibe_bad_color
                };
                let icon = if ctx.vibe_score >= 80 { "●" }
                    else if ctx.vibe_score >= 50 { "◐" }
                    else { "○" };
                StyledSegment::new(icon, color)
            }
            PromptVar::Time => StyledSegment::new(&ctx.time, self.theme.separator_color),
            PromptVar::Battery => {
                if let Some(pct) = ctx.battery {
                    let color = if pct > 50 { Color::Green }
                        else if pct > 20 { Color::Yellow }
                        else { Color::Red };
                    StyledSegment::new(&alloc::format!("{}%", pct), color)
                } else {
                    StyledSegment::new("⚡", Color::Green)
                }
            }
            PromptVar::Network => StyledSegment::new(&ctx.network, self.theme.separator_color),
            PromptVar::SiloName => StyledSegment::new(&ctx.silo_name, self.theme.host_color),
            PromptVar::LastExit => {
                if ctx.last_exit != 0 {
                    StyledSegment::new(
                        &alloc::format!("✗{}", ctx.last_exit),
                        self.theme.error_color,
                    ).bold()
                } else {
                    StyledSegment::new("", Color::Reset)
                }
            }
            PromptVar::Literal(text) => StyledSegment::new(text, self.theme.separator_color),
            PromptVar::Newline => StyledSegment::new("\n", Color::Reset),
        }
    }
}
