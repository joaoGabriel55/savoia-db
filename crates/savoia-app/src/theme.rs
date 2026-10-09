//! Savoia's look, after Olivetti in Ivrea: a graphite IDE with a faint green
//! cast, solid Ivrea-green bands that name each zone of work, and one
//! Savoy-blue control (Run). Status colors use their own hues (teal,
//! vermilion, saffron, violet) so they never read as brand.
//!
//! Every color in the app comes from here: either the GPUI Kit theme slots
//! set in [`apply`], or the brand constants below.

use std::sync::Arc;

use gpui_kit::Styled as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonCustomVariant};
use gpui_kit::component::highlighter::SyntaxColors;
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{App, Hsla, rgb};

/// Ivrea green: the primary brand color. Fills bands and primary buttons.
pub const IVREA_GREEN: u32 = 0x1F5A43;
const IVREA_GREEN_HOVER: u32 = 0x286C51;
const IVREA_GREEN_PRESSED: u32 = 0x184634;
/// The same green, lifted for lines on graphite: focus ring, caret, active
/// cell, active result tab. Deep green alone is invisible as a 1px stroke.
pub const IVREA_LINE: u32 = 0x5FB088;
/// Azzurro Savoia: the accent, the House of Savoy's own blue. Run is its only
/// control; it never decorates. Red stays reserved for danger.
pub const SAVOY_BLUE: u32 = 0x1F64B4;
const SAVOY_BLUE_HOVER: u32 = 0x2B74C7;
const SAVOY_BLUE_PRESSED: u32 = 0x1A5498;
const SAVOY_INK: u32 = 0xFFFFFF;

/// The ER diagram's board: a blueprint in deep Ivrea green (blue is Run's
/// alone), ruled with band ink at low opacity.
pub const BLUEPRINT: u32 = 0x10372A;

/// Text on an Ivrea band, and secondary text on it (tinted from the green).
pub const BAND_INK: u32 = 0xF2EFE8;
pub const BAND_INK_MUTED: u32 = 0xB5D0C2;
/// Disabled controls on a band (about 3.4:1 on Ivrea green). GPUI Kit's own
/// disabled ink is tuned for graphite and nearly vanishes on the band.
const BAND_INK_DISABLED: u32 = 0x8FAF9F;
/// Hairline separators inside a band.
pub const BAND_RULE: u32 = 0x2F7458;

const GRAPHITE_CANVAS: u32 = 0x161A17;
const GRAPHITE_PANEL: u32 = 0x1D211E;
const GRAPHITE_SEAM: u32 = 0x111412;
const GRAPHITE_RULE: u32 = 0x272C28;
const GRAPHITE_HOVER: u32 = 0x2A302C;
const GRAPHITE_ROW_HOVER: u32 = 0x1F2420;
const SELECTION: u32 = 0x213A31;
const INK: u32 = 0xE9E6DF;
const INK_HEADER: u32 = 0xB8BDB8;
const INK_MUTED: u32 = 0x8D938F;

const STATUS_TEAL: u32 = 0x3DB9AE;
const STATUS_VERMILION: u32 = 0xF2713C;
const STATUS_SAFFRON: u32 = 0xE5B54A;
const STATUS_VIOLET: u32 = 0x9B8CE8;

pub fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub fn apply(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    // `update` (not `global_mut`) so GPUI Kit re-derives its component tokens
    // from these colors; buttons and tabs paint from those tokens.
    Theme::update(cx, edit);
}

fn edit(theme: &mut Theme) {
    // The default dark highlight theme paints the editor near-black; keep it
    // on the graphite canvas like the rest of the console.
    let mut highlight = (*theme.highlight_theme).clone();
    highlight.style.editor_background = Some(c(GRAPHITE_CANVAS));
    recolor_syntax(&mut highlight.style.syntax);
    theme.highlight_theme = Arc::new(highlight);

    let colors = &mut theme.colors;

    colors.background = c(GRAPHITE_CANVAS);
    colors.foreground = c(INK);
    colors.muted_foreground = c(INK_MUTED);
    colors.border = c(GRAPHITE_RULE);
    colors.input = c(GRAPHITE_RULE);
    colors.selection = c(SELECTION);

    colors.title_bar = c(GRAPHITE_PANEL);
    colors.title_bar_border = c(GRAPHITE_SEAM);
    colors.status_bar = c(GRAPHITE_PANEL);
    colors.status_bar_border = c(GRAPHITE_SEAM);

    colors.sidebar = c(GRAPHITE_PANEL);
    colors.sidebar_foreground = c(INK);
    colors.sidebar_border = c(GRAPHITE_SEAM);

    colors.list_hover = c(GRAPHITE_HOVER);
    colors.list_active = c(SELECTION);
    colors.list_active_border = c(SELECTION);

    colors.tab_bar = c(GRAPHITE_CANVAS);
    colors.tab = c(GRAPHITE_CANVAS);
    colors.tab_active = c(GRAPHITE_CANVAS);
    colors.tab_active_foreground = c(INK);
    colors.tab_foreground = c(INK_MUTED);

    colors.table = c(GRAPHITE_CANVAS);
    colors.table_head = c(GRAPHITE_PANEL);
    colors.table_head_foreground = c(INK_HEADER);
    colors.table_even = c(GRAPHITE_CANVAS);
    colors.table_hover = c(GRAPHITE_ROW_HOVER);
    colors.table_active = c(SELECTION);
    colors.table_active_border = c(IVREA_LINE);
    colors.table_row_border = c(GRAPHITE_RULE);

    colors.primary = c(IVREA_GREEN);
    colors.primary_hover = c(IVREA_GREEN_HOVER);
    colors.primary_active = c(IVREA_GREEN_PRESSED);
    colors.primary_foreground = c(BAND_INK);
    colors.button_primary = c(IVREA_GREEN);
    colors.button_primary_hover = c(IVREA_GREEN_HOVER);
    colors.button_primary_active = c(IVREA_GREEN_PRESSED);
    colors.button_primary_foreground = c(BAND_INK);
    colors.caret = c(IVREA_LINE);
    colors.ring = c(IVREA_LINE);

    colors.success = c(STATUS_TEAL);
    colors.success_foreground = c(GRAPHITE_CANVAS);
    colors.danger = c(STATUS_VERMILION);
    colors.danger_foreground = c(GRAPHITE_CANVAS);
    colors.warning = c(STATUS_SAFFRON);
    colors.warning_foreground = c(GRAPHITE_CANVAS);
    colors.info = c(STATUS_VIOLET);
    colors.info_foreground = c(GRAPHITE_CANVAS);
}

/// SQL syntax colors. The GPUI Kit default paints keywords, functions and
/// attributes blue, but Savoy blue belongs to Run alone. Keywords take the
/// Ivrea line green, so strings move from green to sand to stay distinct.
const SYNTAX: &[(&str, &str)] = &[
    ("keyword", "#6DBE95"),
    ("title", "#6DBE95"),
    ("function", "#B9CFC2"),
    ("string", "#D8BC8C"),
    ("string.special", "#D8BC8C"),
    ("string.special.symbol", "#D8BC8C"),
    ("text.literal", "#D8BC8C"),
    ("text.code.span", "#D8BC8C"),
    ("string.escape", "#E6D3AC"),
    ("string.regex", "#E6D3AC"),
    ("number", "#D69AAE"),
    ("constant", "#D69AAE"),
    ("boolean", "#D69AAE"),
    ("type", "#C3A9D9"),
    ("constructor", "#C3A9D9"),
    ("attribute", "#9FC7B4"),
    ("property", "#C9CCC4"),
    ("variable.special", "#8FC9A9"),
    ("tag", "#8FC9A9"),
    ("link_text", "#8FC9A9"),
    ("link_uri", "#9FC7B4"),
];

/// GPUI Kit's syntax styles have private fields and are built from theme JSON,
/// so the overrides go through the same JSON shape. Each override replaces the
/// token's color and keeps its font style and weight.
fn recolor_syntax(syntax: &mut SyntaxColors) {
    let Ok(serde_json::Value::Object(mut map)) = serde_json::to_value(&*syntax) else {
        return;
    };
    for (token, color) in SYNTAX {
        let entry = map.entry(*token).or_insert_with(|| serde_json::json!({}));
        if entry.is_null() {
            *entry = serde_json::json!({});
        }
        if let Some(style) = entry.as_object_mut() {
            style.insert("color".into(), (*color).into());
        }
    }
    if let Ok(recolored) = serde_json::from_value(serde_json::Value::Object(map)) {
        *syntax = recolored;
    }
}

/// Ghost-style button for controls sitting on an Ivrea band.
pub fn band_button(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(c(IVREA_GREEN))
        .foreground(c(BAND_INK))
        .hover(c(IVREA_GREEN_HOVER))
        .active(c(IVREA_GREEN_PRESSED))
        .shadow(false)
}

/// `disabled` for controls on an Ivrea band, with ink that stays legible on
/// the green.
pub trait BandDisabled {
    fn band_disabled(self, disabled: bool) -> Self;
}

impl BandDisabled for Button {
    fn band_disabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
            .when(disabled, |this| this.text_color(c(BAND_INK_DISABLED)))
    }
}

/// The one blue control: Run. GPUI Kit paints a custom variant's resting fill
/// at 20% strength, so callers also set [`run_fill`] as the button's `bg`.
pub fn run_button(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(c(SAVOY_BLUE))
        .foreground(c(SAVOY_INK))
        .hover(c(SAVOY_BLUE_HOVER))
        .active(c(SAVOY_BLUE_PRESSED))
        .shadow(false)
}

pub fn run_fill() -> Hsla {
    c(SAVOY_BLUE)
}
