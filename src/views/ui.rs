//! Reusable Design System & UI Kit for RustAgent (Shadcn / Tailwind-inspired helpers for Freya).
//!
//! Centralizes cards, shadows, badges, pills, buttons, typography, and layout primitives
//! so views stay concise, readable, and visually consistent across all themes.

use freya::prelude::*;
use std::borrow::Cow;

/// Semantic color tone for buttons, badges, pills, and cards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Default surface button/badge using theme `surface_tertiary` and `border`.
    Neutral,
    /// Subtle surface button using `surface_secondary` (ideal inside cards).
    Subtle,
    /// Primary brand accent (`c.primary`).
    Primary,
    /// Emerald green for creations, validations, and positive stats (`+N`).
    Success,
    /// Rose/red for deletions, rejections, and stop actions (`-N`).
    Danger,
    /// Amber/gold for modifications and warnings.
    Warning,
}

// ============================================================================
// LAYOUT PRIMITIVES (Tailwind-like 1-liners)
// ============================================================================

/// Horizontal flex row with vertically centered items and `spacing` gap.
pub fn row(spacing: f32) -> Rect {
    rect()
        .horizontal()
        .cross_align(Alignment::Center)
        .spacing(spacing)
}

/// Full-width horizontal flex row (`Content::Flex`) with vertically centered items.
pub fn row_fill(spacing: f32) -> Rect {
    rect()
        .width(Size::fill())
        .horizontal()
        .cross_align(Alignment::Center)
        .content(Content::Flex)
        .spacing(spacing)
}

/// Full-width vertical stack with `spacing` gap.
pub fn col(spacing: f32) -> Rect {
    rect().width(Size::fill()).spacing(spacing)
}

/// Flexible spacer (`Size::flex(1.)`) to push subsequent items to the right in a `row_fill`.
pub fn spacer() -> Rect {
    rect().width(Size::flex(1.))
}

/// Zero-sized placeholder element when a conditional view is hidden.
pub fn empty() -> Element {
    rect().width(Size::px(0.)).height(Size::px(0.)).into_element()
}

/// Makes any `rect()` behave like a button: pointer cursor on hover, default cursor on leave.
pub fn clickable(r: Rect) -> Rect {
    r.on_pointer_enter(|_| Cursor::set(CursorIcon::Pointer))
        .on_pointer_leave(|_| Cursor::set(CursorIcon::default()))
}

/// Equal-height tile row for the bento grid (children share `height`, aligned to the top).
pub fn tile_row(height: f32) -> Rect {
    rect()
        .width(Size::fill())
        .height(Size::px(height))
        .horizontal()
        .content(Content::Flex)
        .spacing(10.)
}

/// Bento tile: fixed-height flex child with dense padding, thin border and 6px corners.
pub fn tile(c: &ColorsSheet) -> Rect {
    rect()
        .width(Size::flex(1.))
        .height(Size::fill())
        .padding(12.)
        .background(c.surface_primary)
        .corner_radius(6.)
        .border(Border::new().fill(c.border).width(1.))
        .spacing(6.)
}

/// Clickable bento tile with hover feedback (border highlights in the accent color).
pub fn action_tile(c: &ColorsSheet) -> Rect {
    clickable(tile(c))
}

// ============================================================================
// SURFACES, HEADERS & CARDS
// ============================================================================

/// Standard top bar for any workspace panel (`editor`, `diff_viewer`, `toolbar`, `settings`).
pub fn panel_header(c: &ColorsSheet, height: f32) -> Rect {
    rect()
        .width(Size::fill())
        .height(Size::px(height))
        .padding(Gaps::new(4., 12., 4., 12.))
        .background(c.surface_primary)
        .border(Border::new().fill(c.border).width(BorderWidth {
            top: 0.,
            right: 0.,
            bottom: 1.,
            left: 0.,
        }))
        .horizontal()
        .cross_align(Alignment::Center)
        .content(Content::Flex)
        .spacing(8.)
}

/// Standard surface card with clean rounded corners and crisp border.
pub fn card(c: &ColorsSheet) -> Rect {
    rect()
        .width(Size::fill())
        .padding(14.)
        .background(c.surface_primary)
        .corner_radius(6.)
        .border(Border::new().fill(c.border).width(1.))
        .spacing(8.)
}

/// Elevated showcase or review container with a custom accent border and clean corners.
pub fn accent_card(c: &ColorsSheet, accent: Color) -> Rect {
    rect()
        .width(Size::fill())
        .padding(12.)
        .background(c.surface_secondary)
        .corner_radius(6.)
        .border(Border::new().fill(accent).width(1.))
        .spacing(8.)
}

/// Compact item row card (used for individual files inside the review box).
pub fn item_card(c: &ColorsSheet, highlighted: bool) -> Rect {
    rect()
        .width(Size::fill())
        .padding(Gaps::new(8., 12., 8., 12.))
        .background(c.surface_primary)
        .corner_radius(4.)
        .border(
            Border::new()
                .fill(if highlighted { c.primary } else { c.border })
                .width(1.),
        )
        .spacing(5.)
}

// ============================================================================
// BADGES, PILLS & DIFF STATS
// ============================================================================

/// Resolves `(background, border_color, text_color)` for a semantic [`Tone`] badge.
fn tone_badge_colors(tone: Tone, c: &ColorsSheet) -> (Color, Color, Color) {
    match tone {
        Tone::Neutral => (c.surface_tertiary, c.border, c.text_secondary),
        Tone::Subtle => (c.surface_secondary, c.border, c.text_secondary),
        Tone::Primary => (
            Color::from_argb(38, 122, 162, 247),
            c.primary,
            c.text_primary,
        ),
        Tone::Success => (
            Color::from_argb(38, 16, 185, 129),
            Color::from_rgb(16, 185, 129),
            Color::from_rgb(74, 222, 128),
        ),
        Tone::Danger => (
            Color::from_argb(38, 239, 68, 68),
            Color::from_rgb(239, 68, 68),
            Color::from_rgb(248, 113, 113),
        ),
        Tone::Warning => (
            Color::from_argb(38, 234, 179, 8),
            Color::from_rgb(234, 179, 8),
            Color::from_rgb(250, 204, 21),
        ),
    }
}

/// Fully rounded capsule pill (`corner_radius(99.)`) with subtle border and bold label.
#[allow(dead_code)]
pub fn pill(text: impl Into<Cow<'static, str>>, tone: Tone, c: &ColorsSheet) -> Rect {
    let (bg, border_col, fg) = tone_badge_colors(tone, c);
    rect()
        .padding(Gaps::new(4., 12., 4., 12.))
        .background(bg)
        .border(Border::new().fill(border_col).width(1.))
        .corner_radius(99.)
        .child(
            label()
                .text(text)
                .color(fg)
                .font_size(10.5)
                .font_weight(FontWeight::BOLD),
        )
}

/// Compact rounded tag (`corner_radius(5.)`) for status indicators (`NOUVEAU`, `MODIFIÉ`, `+12`, `-3`).
pub fn tag(text: impl Into<Cow<'static, str>>, tone: Tone, c: &ColorsSheet) -> Rect {
    let (bg, _border_col, fg) = tone_badge_colors(tone, c);
    rect()
        .padding(Gaps::new(2., 6., 2., 6.))
        .background(bg)
        .corner_radius(5.)
        .child(
            label()
                .text(text)
                .color(fg)
                .font_size(10.5)
                .font_weight(FontWeight::BOLD),
        )
}

/// Renders the pair of `+added` (green) and `-deleted` (red) diff stat badges in a single call.
pub fn diff_stats(added: usize, deleted: usize, c: &ColorsSheet) -> Rect {
    row(4.)
        .child(tag(format!("+{}", added), Tone::Success, c))
        .child(tag(format!("-{}", deleted), Tone::Danger, c))
}

/// Monospace code badge (`<code>`) with subtle surface background, crisp border, and clean typography.
pub fn code_badge(text: impl Into<Cow<'static, str>>, c: &ColorsSheet) -> Rect {
    rect()
        .padding(Gaps::new(2., 6., 2., 6.))
        .background(c.surface_secondary)
        .corner_radius(4.)
        .border(Border::new().fill(c.border).width(1.))
        .child(mono(text, 11., c.text_primary))
}

/// Dedicated icon glyph for starters, action tiles, and cockpit badges.
pub fn icon(name: &str, size: f32, color: Color) -> Label {
    let glyph = match name {
        "shield" => "◈",
        "cpu" => "⬡",
        "file" => "≡",
        "blocks" => "⇄",
        "terminal" => ">_",
        "check" => "✔",
        _ => "●",
    };
    label().text(glyph).font_size(size).color(color)
}

// ============================================================================
// BUTTONS
// ============================================================================

/// Resolves `(bg, hover_bg, border_col, text_col)` for a button of the given [`Tone`].
fn tone_button_colors(tone: Tone, c: &ColorsSheet) -> (Color, Color, Color, Color) {
    match tone {
        Tone::Neutral => (
            c.surface_secondary,
            c.surface_tertiary,
            c.border,
            c.text_primary,
        ),
        Tone::Subtle => (
            c.surface_primary,
            c.surface_secondary,
            c.border,
            c.text_secondary,
        ),
        Tone::Primary => (
            c.primary,
            Color::from_rgb(194, 65, 12),
            Color::TRANSPARENT,
            Color::WHITE,
        ),
        Tone::Success => (
            c.surface_secondary,
            c.surface_tertiary,
            Color::from_rgb(34, 197, 94),
            Color::from_rgb(74, 222, 128),
        ),
        Tone::Danger => (
            c.surface_secondary,
            c.surface_tertiary,
            Color::from_rgb(239, 68, 68),
            Color::from_rgb(248, 113, 113),
        ),
        Tone::Warning => (
            c.surface_secondary,
            c.surface_tertiary,
            Color::from_rgb(245, 158, 11),
            Color::from_rgb(250, 204, 21),
        ),
    }
}

/// Standard styled button with semantic [`Tone`].
pub fn btn<H>(
    text: impl Into<Cow<'static, str>>,
    tone: Tone,
    c: &ColorsSheet,
    on_press: H,
) -> Button
where
    H: Into<EventHandler<Event<PressEventData>>>,
{
    let (bg, hover_bg, border_col, fg) = tone_button_colors(tone, c);
    Button::new()
        .cursor_icon(CursorIcon::Pointer)
        .background(bg)
        .hover_background(hover_bg)
        .border_fill(border_col)
        .corner_radius(4.)
        .padding(Gaps::new(7., 14., 7., 14.))
        .on_press(on_press)
        .child(
            label()
                .text(text)
                .font_size(12.5)
                .font_weight(FontWeight::BOLD)
                .color(fg),
        )
}

/// Compact button (`btn_sm`) balanced to match the height and corner radius of [`select`].
pub fn btn_sm<H>(
    text: impl Into<Cow<'static, str>>,
    tone: Tone,
    c: &ColorsSheet,
    on_press: H,
) -> Button
where
    H: Into<EventHandler<Event<PressEventData>>>,
{
    let (bg, hover_bg, border_col, fg) = tone_button_colors(tone, c);
    Button::new()
        .cursor_icon(CursorIcon::Pointer)
        .background(bg)
        .hover_background(hover_bg)
        .border_fill(border_col)
        .corner_radius(4.)
        .padding(Gaps::new(6., 12., 6., 12.))
        .on_press(on_press)
        .child(
            label()
                .text(text)
                .font_size(12.)
                .font_weight(FontWeight::BOLD)
                .color(fg),
        )
}

/// Styled dropdown [`Select`] with reduced `12.5px` typography to align with [`btn_sm`].
pub fn select(
    selected_text: impl Into<Cow<'static, str>>,
    items: impl IntoIterator<Item = impl IntoElement>,
    c: &ColorsSheet,
) -> Select {
    Select::new()
        .selected_item(
            label()
                .text(selected_text)
                .font_size(12.)
                .font_weight(FontWeight::BOLD)
                .color(c.text_primary),
        )
        .children(items)
}

/// Styled dropdown [`MenuItem`] with compact `12px` font size.
pub fn menu_item<H>(
    text: impl Into<Cow<'static, str>>,
    is_selected: bool,
    c: &ColorsSheet,
    on_press: H,
) -> MenuItem
where
    H: Into<EventHandler<Event<PressEventData>>>,
{
    MenuItem::new()
        .selected(is_selected)
        .on_press(on_press)
        .child(
            label()
                .text(text)
                .font_size(12.)
                .color(if is_selected {
                    c.text_primary
                } else {
                    c.text_secondary
                }),
        )
}

// ============================================================================
// TYPOGRAPHY HELPERS
// ============================================================================

/// Bold primary section/card title (`14px`).
pub fn title(text: impl Into<Cow<'static, str>>, c: &ColorsSheet) -> Label {
    label()
        .text(text)
        .color(c.text_primary)
        .font_size(14.)
        .font_weight(FontWeight::BOLD)
}

/// Secondary descriptive text (`12px`).
pub fn subtitle(text: impl Into<Cow<'static, str>>, c: &ColorsSheet) -> Label {
    label().text(text).color(c.text_secondary).font_size(12.)
}

/// Muted caption or file path text (`11px`).
pub fn muted(text: impl Into<Cow<'static, str>>, c: &ColorsSheet) -> Label {
    label().text(text).color(c.text_placeholder).font_size(11.)
}

/// Monospace code label (`Jetbrains Mono`).
pub fn mono(text: impl Into<Cow<'static, str>>, size: f32, color: Color) -> Label {
    label()
        .font_family("Jetbrains Mono")
        .font_size(size)
        .color(color)
        .text(text)
}
