//! External theme ("stylesheet") support built on Freya's native theming.
//!
//! The app's color palette lives in a `theme.json` file. This module loads
//! that file at startup and builds a Freya [`Theme`] / [`ColorsSheet`] from
//! it, which is then provided to the component tree through Freya's own
//! `use_provide_theme` / `use_theme` hooks. If the file is missing or
//! malformed, a sensible dark fallback palette is used so the app still runs.
//!
//! The file is looked up in the current working directory (`theme.json`) and,
//! as a fallback, in the `src/` directory (`src/theme.json`). This lets the
//! theme be edited and reloaded without touching the Rust source.

use freya::prelude::*;
use serde::Deserialize;
use std::path::Path;

/// A raw color as it appears in `theme.json`.
#[derive(Deserialize)]
struct RawColor {
    r: u8,
    g: u8,
    b: u8,
    #[serde(default)]
    a: Option<u8>,
}

impl RawColor {
    fn to_color(&self) -> Color {
        match self.a {
            Some(a) => Color::from_argb(a, self.r, self.g, self.b),
            None => Color::from_rgb(self.r, self.g, self.b),
        }
    }
}

/// The JSON shape of `theme.json`. Every field maps to a [`ColorsSheet`]
/// field, so the UI reads like a stylesheet: components reference
/// `c.surface_primary` instead of hardcoding `(40, 40, 40)`.
#[derive(Deserialize)]
struct RawTheme {
    colors: RawColors,
}

#[derive(Deserialize)]
struct RawColors {
    // Brand & Accent
    primary: RawColor,
    secondary: RawColor,
    tertiary: RawColor,
    // Status / Semantic
    success: RawColor,
    warning: RawColor,
    error: RawColor,
    info: RawColor,
    // Surfaces / Backgrounds
    background: RawColor,
    surface_primary: RawColor,
    surface_secondary: RawColor,
    surface_tertiary: RawColor,
    surface_inverse: RawColor,
    surface_inverse_secondary: RawColor,
    surface_inverse_tertiary: RawColor,
    // Borders
    border: RawColor,
    border_focus: RawColor,
    border_disabled: RawColor,
    // Text / Content
    text_primary: RawColor,
    text_secondary: RawColor,
    text_placeholder: RawColor,
    text_inverse: RawColor,
    text_highlight: RawColor,
    // States / Interaction
    focus: RawColor,
    active: RawColor,
    disabled: RawColor,
    // Utility
    overlay: RawColor,
    shadow: RawColor,
}

impl RawColors {
    fn to_colors_sheet(&self) -> ColorsSheet {
        ColorsSheet {
            primary: self.primary.to_color(),
            secondary: self.secondary.to_color(),
            tertiary: self.tertiary.to_color(),
            success: self.success.to_color(),
            warning: self.warning.to_color(),
            error: self.error.to_color(),
            info: self.info.to_color(),
            background: self.background.to_color(),
            surface_primary: self.surface_primary.to_color(),
            surface_secondary: self.surface_secondary.to_color(),
            surface_tertiary: self.surface_tertiary.to_color(),
            surface_inverse: self.surface_inverse.to_color(),
            surface_inverse_secondary: self.surface_inverse_secondary.to_color(),
            surface_inverse_tertiary: self.surface_inverse_tertiary.to_color(),
            border: self.border.to_color(),
            border_focus: self.border_focus.to_color(),
            border_disabled: self.border_disabled.to_color(),
            text_primary: self.text_primary.to_color(),
            text_secondary: self.text_secondary.to_color(),
            text_placeholder: self.text_placeholder.to_color(),
            text_inverse: self.text_inverse.to_color(),
            text_highlight: self.text_highlight.to_color(),
            focus: self.focus.to_color(),
            active: self.active.to_color(),
            disabled: self.disabled.to_color(),
            overlay: self.overlay.to_color(),
            shadow: self.shadow.to_color(),
        }
    }
}

/// The fallback palette used when `theme.json` cannot be loaded. This mirrors
/// the values shipped in `src/theme.json` so the app looks identical whether
/// or not the file is present.
fn fallback_colors() -> ColorsSheet {
    ColorsSheet {
        primary: Color::from_rgb(103, 80, 164),
        secondary: Color::from_rgb(202, 193, 227),
        tertiary: Color::from_rgb(79, 61, 130),
        success: Color::from_rgb(129, 199, 132),
        warning: Color::from_rgb(255, 200, 120),
        error: Color::from_rgb(229, 115, 115),
        info: Color::from_rgb(100, 181, 246),
        background: Color::from_rgb(30, 30, 30),
        surface_primary: Color::from_rgb(40, 40, 40),
        surface_secondary: Color::from_rgb(20, 20, 20),
        surface_tertiary: Color::from_rgb(65, 65, 65),
        surface_inverse: Color::from_rgb(45, 55, 70),
        surface_inverse_secondary: Color::from_rgb(25, 25, 35),
        surface_inverse_tertiary: Color::from_rgb(60, 60, 70),
        border: Color::from_rgb(55, 55, 55),
        border_focus: Color::from_rgb(110, 110, 110),
        border_disabled: Color::from_rgb(80, 80, 80),
        text_primary: Color::from_rgb(245, 245, 245),
        text_secondary: Color::from_rgb(200, 200, 200),
        text_placeholder: Color::from_rgb(150, 150, 160),
        text_inverse: Color::from_rgb(220, 220, 230),
        text_highlight: Color::from_rgb(180, 180, 190),
        focus: Color::from_rgb(100, 100, 120),
        active: Color::from_rgb(70, 70, 70),
        disabled: Color::from_rgb(50, 50, 50),
        overlay: Color::from_argb(180, 0, 0, 0),
        shadow: Color::from_argb(120, 0, 0, 0),
    }
}

/// Read the raw theme JSON from disk, trying the working directory first and
/// then the `src/` directory.
fn read_raw_theme() -> Option<RawTheme> {
    for candidate in [Path::new("theme.json"), Path::new("src/theme.json")] {
        if let Ok(text) = std::fs::read_to_string(candidate)
            && let Ok(raw) = serde_json::from_str::<RawTheme>(&text)
        {
            return Some(raw);
        }
    }
    None
}

/// Available presets for fast theme switching with modern, clean, minimalist styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemePreset {
    ObsidianDark,
    TokyoNight,
    CatppuccinMocha,
    NordicMinimal,
    OneDarkPro,
    MidnightBlue,
    GitHubLight,
}

impl ThemePreset {
    pub const ALL: &'static [ThemePreset] = &[
        ThemePreset::ObsidianDark,
        ThemePreset::TokyoNight,
        ThemePreset::CatppuccinMocha,
        ThemePreset::NordicMinimal,
        ThemePreset::OneDarkPro,
        ThemePreset::MidnightBlue,
        ThemePreset::GitHubLight,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            ThemePreset::ObsidianDark => "Obsidian (Par défaut)",
            ThemePreset::TokyoNight => "Tokyo Night",
            ThemePreset::CatppuccinMocha => "Catppuccin Mocha",
            ThemePreset::NordicMinimal => "Nord Minimal",
            ThemePreset::OneDarkPro => "One Dark Pro",
            ThemePreset::MidnightBlue => "Midnight Clean",
            ThemePreset::GitHubLight => "GitHub Light",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            ThemePreset::ObsidianDark => "🌑",
            ThemePreset::TokyoNight => "🌆",
            ThemePreset::CatppuccinMocha => "☕",
            ThemePreset::NordicMinimal => "❄️",
            ThemePreset::OneDarkPro => "⚡",
            ThemePreset::MidnightBlue => "🌙",
            ThemePreset::GitHubLight => "☀️",
        }
    }

    pub fn to_theme(self) -> Theme {
        let colors = match self {
            ThemePreset::ObsidianDark => match read_raw_theme() {
                Some(raw) => raw.colors.to_colors_sheet(),
                None => fallback_colors(),
            },
            // Tokyo Night: Subtle, modern dark slate with refined indigo/cyan accents
            ThemePreset::TokyoNight => ColorsSheet {
                primary: Color::from_rgb(122, 162, 247), // Crisp blue accent
                secondary: Color::from_rgb(187, 154, 247), // Lavender
                tertiary: Color::from_rgb(41, 46, 66),   // Subtle dark slate
                success: Color::from_rgb(158, 206, 106),
                warning: Color::from_rgb(224, 175, 104),
                error: Color::from_rgb(247, 118, 142),
                info: Color::from_rgb(122, 162, 247),
                background: Color::from_rgb(26, 27, 38), // Clean dark background
                surface_primary: Color::from_rgb(31, 35, 53), // Panels
                surface_secondary: Color::from_rgb(22, 22, 30),
                surface_tertiary: Color::from_rgb(41, 46, 66), // Hover / cards
                surface_inverse: Color::from_rgb(52, 59, 88),  // User message bubble
                surface_inverse_secondary: Color::from_rgb(29, 32, 48), // AI bubble
                surface_inverse_tertiary: Color::from_rgb(65, 72, 104),
                border: Color::from_rgb(44, 49, 70), // Subtle border
                border_focus: Color::from_rgb(122, 162, 247),
                border_disabled: Color::from_rgb(38, 42, 60),
                text_primary: Color::from_rgb(192, 202, 245),
                text_secondary: Color::from_rgb(154, 165, 206),
                text_placeholder: Color::from_rgb(100, 110, 145),
                text_inverse: Color::from_rgb(240, 244, 255),
                text_highlight: Color::from_rgb(255, 255, 255),
                focus: Color::from_rgb(122, 162, 247),
                active: Color::from_rgb(45, 52, 75),
                disabled: Color::from_rgb(30, 32, 44),
                overlay: Color::from_argb(160, 10, 10, 15),
                shadow: Color::from_argb(140, 0, 0, 0),
            },
            // Catppuccin Mocha: Balanced warm dark palette loved by developers
            ThemePreset::CatppuccinMocha => ColorsSheet {
                primary: Color::from_rgb(137, 180, 250),   // Blue accent
                secondary: Color::from_rgb(203, 166, 247), // Mauve
                tertiary: Color::from_rgb(49, 50, 68),     // Surface0
                success: Color::from_rgb(166, 227, 161),
                warning: Color::from_rgb(249, 226, 175),
                error: Color::from_rgb(243, 139, 168),
                info: Color::from_rgb(137, 220, 235),
                background: Color::from_rgb(30, 30, 46), // Base
                surface_primary: Color::from_rgb(36, 37, 54), // Mantle
                surface_secondary: Color::from_rgb(24, 24, 37), // Crust
                surface_tertiary: Color::from_rgb(49, 50, 68), // Surface0
                surface_inverse: Color::from_rgb(69, 71, 90), // Surface1
                surface_inverse_secondary: Color::from_rgb(33, 34, 50),
                surface_inverse_tertiary: Color::from_rgb(88, 91, 112),
                border: Color::from_rgb(52, 54, 74), // Subtle border
                border_focus: Color::from_rgb(137, 180, 250),
                border_disabled: Color::from_rgb(40, 42, 60),
                text_primary: Color::from_rgb(205, 214, 244), // Text
                text_secondary: Color::from_rgb(166, 173, 200), // Subtext0
                text_placeholder: Color::from_rgb(115, 120, 142),
                text_inverse: Color::from_rgb(245, 245, 255),
                text_highlight: Color::from_rgb(255, 255, 255),
                focus: Color::from_rgb(137, 180, 250),
                active: Color::from_rgb(55, 57, 78),
                disabled: Color::from_rgb(33, 34, 48),
                overlay: Color::from_argb(160, 15, 15, 25),
                shadow: Color::from_argb(140, 0, 0, 0),
            },
            // Nord Minimal: Pure understated arctic grays and cool muted tones
            ThemePreset::NordicMinimal => ColorsSheet {
                primary: Color::from_rgb(136, 192, 208), // Muted Cyan
                secondary: Color::from_rgb(129, 161, 193),
                tertiary: Color::from_rgb(59, 66, 82),
                success: Color::from_rgb(163, 190, 140),
                warning: Color::from_rgb(235, 203, 139),
                error: Color::from_rgb(191, 97, 106),
                info: Color::from_rgb(136, 192, 208),
                background: Color::from_rgb(46, 52, 64), // Nord0
                surface_primary: Color::from_rgb(52, 59, 73), // Nord1
                surface_secondary: Color::from_rgb(40, 45, 55),
                surface_tertiary: Color::from_rgb(67, 76, 94), // Nord2
                surface_inverse: Color::from_rgb(76, 86, 106), // Nord3
                surface_inverse_secondary: Color::from_rgb(48, 54, 66),
                surface_inverse_tertiary: Color::from_rgb(86, 97, 120),
                border: Color::from_rgb(67, 76, 94),
                border_focus: Color::from_rgb(136, 192, 208),
                border_disabled: Color::from_rgb(55, 62, 77),
                text_primary: Color::from_rgb(229, 233, 240), // Nord5
                text_secondary: Color::from_rgb(180, 188, 204),
                text_placeholder: Color::from_rgb(130, 138, 155),
                text_inverse: Color::from_rgb(245, 247, 250),
                text_highlight: Color::from_rgb(236, 239, 244),
                focus: Color::from_rgb(136, 192, 208),
                active: Color::from_rgb(67, 76, 94),
                disabled: Color::from_rgb(42, 47, 58),
                overlay: Color::from_argb(160, 20, 25, 35),
                shadow: Color::from_argb(130, 0, 0, 0),
            },
            // One Dark Pro: The standard VS Code / Atom dark theme
            ThemePreset::OneDarkPro => ColorsSheet {
                primary: Color::from_rgb(97, 175, 239),    // One Blue
                secondary: Color::from_rgb(198, 120, 221), // Purple
                tertiary: Color::from_rgb(44, 49, 58),
                success: Color::from_rgb(152, 195, 121),
                warning: Color::from_rgb(229, 192, 123),
                error: Color::from_rgb(224, 108, 117),
                info: Color::from_rgb(86, 182, 194),
                background: Color::from_rgb(33, 37, 43), // Neutral dark gray
                surface_primary: Color::from_rgb(40, 44, 52), // Editor background
                surface_secondary: Color::from_rgb(28, 31, 36),
                surface_tertiary: Color::from_rgb(49, 54, 63),
                surface_inverse: Color::from_rgb(58, 64, 76),
                surface_inverse_secondary: Color::from_rgb(36, 40, 48),
                surface_inverse_tertiary: Color::from_rgb(70, 78, 92),
                border: Color::from_rgb(49, 54, 63),
                border_focus: Color::from_rgb(97, 175, 239),
                border_disabled: Color::from_rgb(42, 46, 54),
                text_primary: Color::from_rgb(220, 223, 228),
                text_secondary: Color::from_rgb(171, 178, 191),
                text_placeholder: Color::from_rgb(115, 122, 135),
                text_inverse: Color::from_rgb(245, 245, 245),
                text_highlight: Color::from_rgb(255, 255, 255),
                focus: Color::from_rgb(97, 175, 239),
                active: Color::from_rgb(50, 56, 66),
                disabled: Color::from_rgb(35, 39, 45),
                overlay: Color::from_argb(160, 15, 18, 22),
                shadow: Color::from_argb(140, 0, 0, 0),
            },
            // Midnight Clean: Neutral deep charcoal with subtle sapphire touches
            ThemePreset::MidnightBlue => ColorsSheet {
                primary: Color::from_rgb(56, 139, 253), // Modern clean blue
                secondary: Color::from_rgb(139, 148, 158),
                tertiary: Color::from_rgb(33, 38, 45),
                success: Color::from_rgb(63, 185, 80),
                warning: Color::from_rgb(210, 153, 34),
                error: Color::from_rgb(248, 81, 73),
                info: Color::from_rgb(88, 166, 255),
                background: Color::from_rgb(13, 17, 23), // GitHub Dark Dimmed base
                surface_primary: Color::from_rgb(22, 27, 34), // GitHub Dark surface
                surface_secondary: Color::from_rgb(10, 13, 18),
                surface_tertiary: Color::from_rgb(33, 38, 45),
                surface_inverse: Color::from_rgb(40, 46, 56),
                surface_inverse_secondary: Color::from_rgb(18, 22, 29),
                surface_inverse_tertiary: Color::from_rgb(50, 57, 70),
                border: Color::from_rgb(48, 54, 61),
                border_focus: Color::from_rgb(56, 139, 253),
                border_disabled: Color::from_rgb(35, 40, 46),
                text_primary: Color::from_rgb(230, 237, 243),
                text_secondary: Color::from_rgb(139, 148, 158),
                text_placeholder: Color::from_rgb(100, 110, 120),
                text_inverse: Color::from_rgb(255, 255, 255),
                text_highlight: Color::from_rgb(240, 246, 252),
                focus: Color::from_rgb(56, 139, 253),
                active: Color::from_rgb(33, 38, 45),
                disabled: Color::from_rgb(25, 30, 36),
                overlay: Color::from_argb(170, 5, 8, 12),
                shadow: Color::from_argb(150, 0, 0, 0),
            },
            // GitHub Light: Clean, crisp, professional white/slate light theme
            ThemePreset::GitHubLight => ColorsSheet {
                primary: Color::from_rgb(9, 105, 218), // GitHub Light Blue
                secondary: Color::from_rgb(87, 96, 106),
                tertiary: Color::from_rgb(234, 238, 242),
                success: Color::from_rgb(26, 127, 55),
                warning: Color::from_rgb(154, 103, 0),
                error: Color::from_rgb(207, 34, 46),
                info: Color::from_rgb(9, 105, 218),
                background: Color::from_rgb(255, 255, 255), // Pure white canvas
                surface_primary: Color::from_rgb(246, 248, 250), // Off-white clean surface
                surface_secondary: Color::from_rgb(250, 251, 252),
                surface_tertiary: Color::from_rgb(234, 238, 242),
                surface_inverse: Color::from_rgb(222, 228, 234), // Subtle message card
                surface_inverse_secondary: Color::from_rgb(243, 245, 247),
                surface_inverse_tertiary: Color::from_rgb(210, 216, 222),
                border: Color::from_rgb(216, 222, 228), // Subtle crisp light border
                border_focus: Color::from_rgb(9, 105, 218),
                border_disabled: Color::from_rgb(230, 235, 240),
                text_primary: Color::from_rgb(31, 35, 40), // High-contrast readable charcoal
                text_secondary: Color::from_rgb(87, 96, 106),
                text_placeholder: Color::from_rgb(140, 149, 159),
                text_inverse: Color::from_rgb(20, 24, 28),
                text_highlight: Color::from_rgb(9, 105, 218),
                focus: Color::from_rgb(9, 105, 218),
                active: Color::from_rgb(225, 230, 236),
                disabled: Color::from_rgb(238, 242, 246),
                overlay: Color::from_argb(120, 0, 0, 0),
                shadow: Color::from_argb(50, 0, 0, 0),
            },
        };

        let mut theme = if matches!(self, ThemePreset::GitHubLight) {
            light_theme()
        } else {
            dark_theme()
        };
        theme.name = self.name();
        theme.colors = colors;
        theme
    }
}

/// Load the initial theme from `theme.json` and build a Freya [`Theme`]. Falls back
/// to the built-in palette on any error.
pub fn load_theme() -> Theme {
    ThemePreset::ObsidianDark.to_theme()
}
