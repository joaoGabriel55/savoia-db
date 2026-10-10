//! Savoia's look, after Olivetti in Ivrea: a graphite IDE with a faint green
//! cast, solid Ivrea-green bands that name each zone of work, and one
//! Savoy-blue control (Run). Status colors use their own hues (teal,
//! vermilion, saffron, violet) so they never read as brand.
//!
//! A light appearance swaps graphite for Olivetti paper; bands, Run and the
//! blueprint keep their colors in both.
//!
//! Every color in the app comes from here: either the GPUI Kit theme slots
//! set in [`apply`], or the brand constants below.

use std::sync::Arc;

use gpui_kit::Styled as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonCustomVariant};
use gpui_kit::component::highlighter::SyntaxColors;
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{App, Hsla, rgb};

/// Ivrea green: the primary brand color. Fills bands and primary buttons.
pub const IVREA_GREEN: u32 = 0x1F5A43;
const IVREA_GREEN_HOVER: u32 = 0x286C51;
const IVREA_GREEN_PRESSED: u32 = 0x184634;
/// The same green, lifted for lines on graphite: focus ring, caret, active
/// cell, active result tab. Deep green alone is invisible as a 1px stroke.
/// Views take it as `theme.ring`, which follows the appearance.
const IVREA_LINE: u32 = 0x5FB088;
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

/// The surfaces, ink and status hues of one appearance. The brand colors
/// (bands, Run, the blueprint) are the same in both: bands carry their own
/// ink, so they read on graphite and on paper alike.
struct Palette {
    canvas: u32,
    panel: u32,
    seam: u32,
    rule: u32,
    hover: u32,
    row_hover: u32,
    selection: u32,
    ink: u32,
    ink_header: u32,
    ink_muted: u32,
    /// Ivrea green as a 1px line: focus ring, caret, active cell and tab.
    line: u32,
    teal: u32,
    vermilion: u32,
    saffron: u32,
    violet: u32,
    /// Ink on a status fill.
    status_ink: u32,
    syntax: &'static [(&'static str, &'static str)],
}

/// Graphite with a faint green cast.
const DARK: Palette = Palette {
    canvas: 0x161A17,
    panel: 0x1D211E,
    seam: 0x111412,
    rule: 0x272C28,
    hover: 0x2A302C,
    row_hover: 0x1F2420,
    selection: 0x213A31,
    ink: 0xE9E6DF,
    ink_header: 0xB8BDB8,
    ink_muted: 0x8D938F,
    line: IVREA_LINE,
    teal: 0x3DB9AE,
    vermilion: 0xF2713C,
    saffron: 0xE5B54A,
    violet: 0x9B8CE8,
    status_ink: 0x161A17,
    syntax: DARK_SYNTAX,
};

/// Olivetti paper: warm off-white, graphite ink. Status hues and the line
/// green are darkened to hold 4.5:1 on the canvas.
const LIGHT: Palette = Palette {
    canvas: 0xF7F6F2,
    panel: 0xEEEDE7,
    seam: 0xD6D4CB,
    rule: 0xE0DED6,
    hover: 0xE4E2DA,
    row_hover: 0xF0EFE9,
    selection: 0xD3E6DC,
    ink: 0x1C201D,
    ink_header: 0x48504A,
    ink_muted: 0x646B66,
    line: 0x1B7650,
    teal: 0x0A7870,
    vermilion: 0xB8441A,
    saffron: 0x836109,
    violet: 0x5E4DB8,
    status_ink: 0xFFFFFF,
    syntax: LIGHT_SYNTAX,
};

/// Pending changes in a data view take their status hue (teal added,
/// saffron edited, vermilion deleted) as a quiet wash under the cell, and
/// at full strength as the mark in the row gutter.
pub const PENDING_WASH: f32 = 0.10;

/// Which palette the app wears. `System` follows the OS and keeps following
/// it while the app runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Match system",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    /// Unknown or missing values fall back to `System`.
    pub fn parse(key: Option<&str>) -> Self {
        Self::ALL
            .into_iter()
            .find(|a| Some(a.key()) == key)
            .unwrap_or_default()
    }

    fn mode(self, cx: &App) -> ThemeMode {
        match self {
            Self::System => cx.window_appearance().into(),
            Self::Light => ThemeMode::Light,
            Self::Dark => ThemeMode::Dark,
        }
    }
}

fn palette(cx: &App) -> &'static Palette {
    if cx.theme().is_dark() { &DARK } else { &LIGHT }
}

/// Ink for read-only values from related tables: a step below body ink,
/// so they read as reference rather than as editable data.
pub fn related_ink(cx: &App) -> Hsla {
    c(palette(cx).ink_header)
}

pub fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub fn apply(appearance: Appearance, cx: &mut App) {
    let mode = appearance.mode(cx);
    // `change` loads the toolkit's own theme for the mode (and its highlight
    // theme); the edit then paints Savoia over it.
    Theme::change(mode, None, cx);
    let palette = if mode.is_dark() { &DARK } else { &LIGHT };
    // `update` (not `global_mut`) so GPUI Kit re-derives its component tokens
    // from these colors; buttons and tabs paint from those tokens.
    Theme::update(cx, |theme| edit(theme, palette));
}

fn edit(theme: &mut Theme, p: &Palette) {
    // The default highlight themes paint the editor their own background;
    // keep it on the canvas like the rest of the console.
    let mut highlight = (*theme.highlight_theme).clone();
    highlight.style.editor_background = Some(c(p.canvas));
    recolor_syntax(&mut highlight.style.syntax, p.syntax);
    theme.highlight_theme = Arc::new(highlight);

    let colors = &mut theme.colors;

    colors.background = c(p.canvas);
    colors.foreground = c(p.ink);
    colors.muted_foreground = c(p.ink_muted);
    colors.border = c(p.rule);
    colors.input = c(p.rule);
    colors.selection = c(p.selection);
    // Not left to the toolkit default, which falls outside the palette.
    colors.muted = c(p.hover);

    colors.title_bar = c(p.panel);
    colors.title_bar_border = c(p.seam);
    colors.status_bar = c(p.panel);
    colors.status_bar_border = c(p.seam);

    colors.sidebar = c(p.panel);
    colors.sidebar_foreground = c(p.ink);
    colors.sidebar_border = c(p.seam);

    colors.list_hover = c(p.hover);
    colors.list_active = c(p.selection);
    colors.list_active_border = c(p.selection);

    colors.tab_bar = c(p.canvas);
    colors.tab = c(p.canvas);
    colors.tab_active = c(p.canvas);
    colors.tab_active_foreground = c(p.ink);
    colors.tab_foreground = c(p.ink_muted);

    colors.table = c(p.canvas);
    colors.table_head = c(p.panel);
    colors.table_head_foreground = c(p.ink_header);
    colors.table_even = c(p.canvas);
    colors.table_hover = c(p.row_hover);
    // GPUI Kit lays the active cell's fill *over* its content, so it must
    // be see-through: the line green at low strength lands near Selection
    // on the canvas while the value stays readable.
    colors.table_active = c(p.line).opacity(0.18);
    colors.table_active_border = c(p.line);
    colors.table_row_border = c(p.rule);

    colors.primary = c(IVREA_GREEN);
    colors.primary_hover = c(IVREA_GREEN_HOVER);
    colors.primary_active = c(IVREA_GREEN_PRESSED);
    colors.primary_foreground = c(BAND_INK);
    colors.button_primary = c(IVREA_GREEN);
    colors.button_primary_hover = c(IVREA_GREEN_HOVER);
    colors.button_primary_active = c(IVREA_GREEN_PRESSED);
    colors.button_primary_foreground = c(BAND_INK);
    colors.caret = c(p.line);
    colors.ring = c(p.line);

    colors.success = c(p.teal);
    colors.success_foreground = c(p.status_ink);
    colors.danger = c(p.vermilion);
    colors.danger_foreground = c(p.status_ink);
    colors.warning = c(p.saffron);
    colors.warning_foreground = c(p.status_ink);
    colors.info = c(p.violet);
    colors.info_foreground = c(p.status_ink);
}

/// SQL syntax colors. The GPUI Kit defaults paint keywords, functions and
/// attributes blue, but Savoy blue belongs to Run alone. Keywords take the
/// line green, so strings move from green to sand to stay distinct.
const DARK_SYNTAX: &[(&str, &str)] = &[
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

/// The same roles on paper, each at 4.5:1 or better on the light canvas.
const LIGHT_SYNTAX: &[(&str, &str)] = &[
    ("keyword", "#1B7650"),
    ("title", "#1B7650"),
    ("function", "#3B5A4B"),
    ("string", "#87561A"),
    ("string.special", "#87561A"),
    ("string.special.symbol", "#87561A"),
    ("text.literal", "#87561A"),
    ("text.code.span", "#87561A"),
    ("string.escape", "#6F4A10"),
    ("string.regex", "#6F4A10"),
    ("number", "#A0365C"),
    ("constant", "#A0365C"),
    ("boolean", "#A0365C"),
    ("type", "#6A4596"),
    ("constructor", "#6A4596"),
    ("attribute", "#2D6B52"),
    ("property", "#3C423E"),
    ("variable.special", "#27704E"),
    ("tag", "#27704E"),
    ("link_text", "#27704E"),
    ("link_uri", "#2D6B52"),
];

/// GPUI Kit's syntax styles have private fields and are built from theme JSON,
/// so the overrides go through the same JSON shape. Each override replaces the
/// token's color and keeps its font style and weight.
fn recolor_syntax(syntax: &mut SyntaxColors, colors: &[(&str, &str)]) {
    let Ok(serde_json::Value::Object(mut map)) = serde_json::to_value(&*syntax) else {
        return;
    };
    for (token, color) in colors {
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
