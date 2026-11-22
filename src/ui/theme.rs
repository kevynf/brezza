#![expect(clippy::disallowed_methods)]

use egui::style::{Selection, WidgetVisuals};
use egui::{
    Color32, Context, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, Style, TextStyle,
    Theme, Visuals, vec2,
};

use crate::model::{Syntax, UNCOMMITTED};

pub mod space {
    pub const XS: f32 = 2.0;
    pub const S: f32 = 4.0;
    pub const M: f32 = 8.0;
    pub const L: f32 = 12.0;
    pub const XL: f32 = 16.0;
}

pub mod size {
    pub const TOP_BAR: f32 = 44.0;
    pub const HEADER: f32 = CONTROL + 2.0 * super::space::S;
    pub const CONTROL: f32 = 28.0;
    pub const LINE: f32 = 20.0;
    pub const BADGE: f32 = 18.0;
    pub const GLYPH: f32 = 16.0;
    pub const TAB_MIN: f32 = 56.0;
    pub const TAB_MIN_CLOSE: f32 = 80.0;
    pub const TAB_MAX: f32 = 220.0;
    pub const BRANCH_MAX: f32 = 240.0;
    pub const MENU: f32 = 200.0;
    pub const LOGO: f32 = 64.0;
    pub const PAGE_ACTION: f32 = 200.0;
    pub const DIALOG: f32 = 360.0;
    pub const PALETTE: f32 = 480.0;
    pub const PALETTE_LIST: f32 = 320.0;
    pub const SIDEBAR: f32 = 220.0;
    pub const SIDEBAR_MIN: f32 = 160.0;
    pub const SIDEBAR_MAX: f32 = 480.0;
    pub const LIST: f32 = 460.0;
    pub const LIST_MIN: f32 = 320.0;
}

pub mod radius {
    pub const S: u8 = 1;
    pub const M: u8 = S * 2;
    pub const L: u8 = M + super::space::S as u8;
}

pub mod text {
    pub const CAPTION: f32 = 11.0;
    pub const BODY: f32 = 13.0;
}

pub mod font {
    pub const WEIGHT: f32 = 400.0;
    pub const SANS: &[&str] = &[
        "Noto Sans",
        "Noto Sans SC",
        "Segoe UI",
        "Microsoft YaHei UI",
        "PingFang SC",
    ];
    pub const MONO: &[&str] = &[
        "Noto Sans Mono",
        "Cascadia Mono",
        "Consolas",
        "Menlo",
        "Noto Sans SC",
        "Microsoft YaHei UI",
    ];
}

pub struct Palette {
    pub canvas: Color32,
    pub panel: Color32,
    pub raised: Color32,
    pub input: Color32,
    pub overlay: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    pub control_active: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub text_faint: Color32,
    pub text_strong: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub hunk: Color32,
    pub accent_text: Color32,
    pub on_accent: Color32,
    pub tag_soft: Color32,
    pub tag_text: Color32,
    pub warn: Color32,
    pub danger: Color32,
    pub danger_fill: Color32,
    pub on_danger: Color32,
    pub added_text: Color32,
    pub removed_text: Color32,
    pub added: Color32,
    pub removed: Color32,
    pub graph: [Color32; 8],
    pub syntax: [Color32; 6],
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub const DARK: Palette = Palette {
    canvas: hex(0x0d0d0f),
    panel: hex(0x151517),
    raised: hex(0x1c1c1f),
    input: hex(0x1c1c1f),
    overlay: hex(0x1f1f23),
    border: hex(0x26262b),
    border_strong: hex(0x36363c),
    control: hex(0x222226),
    control_hover: hex(0x2a2a2f),
    control_active: hex(0x333339),
    text: hex(0xe4e4e7),
    text_muted: hex(0x9a9aa3),
    text_faint: hex(0x6b6b74),
    text_strong: hex(0xfafafa),
    accent: hex(0x3b82f6),
    accent_soft: hex(0x182a4d),
    hunk: hex(0x161c2a),
    accent_text: hex(0x93c5fd),
    on_accent: hex(0xffffff),
    tag_soft: hex(0x2e2410),
    tag_text: hex(0xfbbf24),
    warn: hex(0xfbbf24),
    danger: hex(0xf87171),
    danger_fill: hex(0xb91c1c),
    on_danger: hex(0xffffff),
    added_text: hex(0x4ade80),
    removed_text: hex(0xf87171),
    added: hex(0x22c55e),
    removed: hex(0xef4444),
    graph: [
        hex(0x60a5fa),
        hex(0xa78bfa),
        hex(0x34d399),
        hex(0xfb923c),
        hex(0xf472b6),
        hex(0x22d3ee),
        hex(0xfacc15),
        hex(0xf87171),
    ],
    syntax: [
        hex(0x6b6b74),
        hex(0x7dd3fc),
        hex(0xc4b5fd),
        hex(0xfdba74),
        hex(0x86efac),
        hex(0x9a9aa3),
    ],
};

pub const LIGHT: Palette = Palette {
    canvas: hex(0xf4f4f5),
    panel: hex(0xffffff),
    raised: hex(0xf8f8f9),
    input: hex(0xffffff),
    overlay: hex(0xffffff),
    border: hex(0xe4e4e7),
    border_strong: hex(0xd4d4d8),
    control: hex(0xf1f1f3),
    control_hover: hex(0xe8e8eb),
    control_active: hex(0xdedee2),
    text: hex(0x18181b),
    text_muted: hex(0x6b6b74),
    text_faint: hex(0xa1a1aa),
    text_strong: hex(0x09090b),
    accent: hex(0x2563eb),
    accent_soft: hex(0xdbeafe),
    hunk: hex(0xf3f8ff),
    accent_text: hex(0x1d4ed8),
    on_accent: hex(0xffffff),
    tag_soft: hex(0xfef3c7),
    tag_text: hex(0xb45309),
    warn: hex(0xb45309),
    danger: hex(0xdc2626),
    danger_fill: hex(0xdc2626),
    on_danger: hex(0xffffff),
    added_text: hex(0x15803d),
    removed_text: hex(0xb91c1c),
    added: hex(0x22c55e),
    removed: hex(0xef4444),
    graph: [
        hex(0x2563eb),
        hex(0x7c3aed),
        hex(0x059669),
        hex(0xea580c),
        hex(0xdb2777),
        hex(0x0891b2),
        hex(0xca8a04),
        hex(0xdc2626),
    ],
    syntax: [
        hex(0xa1a1aa),
        hex(0x0369a1),
        hex(0x7c3aed),
        hex(0xc2410c),
        hex(0x15803d),
        hex(0x71717a),
    ],
};

pub fn palette(visuals: &Visuals) -> &'static Palette {
    if visuals.dark_mode { &DARK } else { &LIGHT }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Hover,
    Selected,
}

impl Palette {
    pub fn item(&self, state: State) -> (Color32, Color32) {
        match state {
            State::Idle => (Color32::TRANSPARENT, self.text),
            State::Hover => (self.control_hover, self.text),
            State::Selected => (self.control_active, self.text_strong),
        }
    }

    pub fn row(&self, selected: bool, hovered: bool) -> Color32 {
        if selected {
            self.accent_soft
        } else if hovered {
            self.control_hover
        } else {
            Color32::TRANSPARENT
        }
    }
}

pub fn install(ctx: &Context) {
    ctx.set_style_of(Theme::Dark, style(&DARK, Visuals::dark()));
    ctx.set_style_of(Theme::Light, style(&LIGHT, Visuals::light()));
}

fn style(p: &Palette, base: Visuals) -> Style {
    let widget = |fill: Color32, stroke: Color32, text: Color32| WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: Stroke::new(1.0, stroke),
        corner_radius: CornerRadius::same(radius::M),
        fg_stroke: Stroke::new(1.0, text),
        expansion: 0.0,
    };
    let mut visuals = base;
    visuals.panel_fill = p.panel;
    visuals.window_fill = p.overlay;
    visuals.extreme_bg_color = p.input;
    visuals.text_edit_bg_color = Some(p.input);
    visuals.faint_bg_color = p.raised;
    visuals.code_bg_color = p.raised;
    visuals.window_stroke = Stroke::new(1.0, p.border);
    visuals.window_corner_radius = CornerRadius::same(radius::L);
    visuals.menu_corner_radius = CornerRadius::same(radius::L);
    visuals.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if visuals.dark_mode { 120 } else { 40 }),
    };
    visuals.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(if visuals.dark_mode { 100 } else { 30 }),
    };
    visuals.hyperlink_color = p.accent_text;
    visuals.warn_fg_color = p.warn;
    visuals.error_fg_color = p.danger;
    visuals.weak_text_color = Some(p.text_muted);
    visuals.selection = Selection {
        bg_fill: p.accent_soft,
        stroke: Stroke::new(1.0, p.accent),
    };
    visuals.widgets.noninteractive = widget(p.panel, p.border, p.text);
    visuals.widgets.inactive = widget(p.control, p.border, p.text);
    visuals.widgets.hovered = widget(p.control_hover, p.border_strong, p.text_strong);
    visuals.widgets.active = widget(p.control_active, p.border_strong, p.text_strong);
    visuals.widgets.open = widget(p.control_hover, p.border_strong, p.text_strong);
    for state in [
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        state.fg_stroke.width = 0.0;
    }
    visuals.collapsing_header_frame = false;
    visuals.indent_has_left_vline = false;

    let mut style = Style {
        visuals,
        ..Style::default()
    };
    style.spacing.item_spacing = vec2(space::M, space::S);
    style.spacing.button_padding = vec2(space::M, space::S);
    style.spacing.interact_size.y = size::CONTROL;
    style.spacing.menu_margin = Margin::same(space::S as i8);
    style.spacing.window_margin = Margin::same(space::XL as i8);
    style.spacing.indent = space::XL;
    style.always_scroll_the_only_direction = true;
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(text::CAPTION)),
        (TextStyle::Body, FontId::proportional(text::BODY)),
        (TextStyle::Button, FontId::proportional(text::BODY)),
        (TextStyle::Heading, FontId::proportional(text::BODY)),
        (
            TextStyle::Monospace,
            FontId::new(text::BODY, FontFamily::Monospace),
        ),
    ]
    .into();
    style
}

pub fn graph(visuals: &Visuals, index: u16) -> Color32 {
    let p = palette(visuals);
    if index == UNCOMMITTED {
        p.text_faint
    } else {
        p.graph[index as usize % p.graph.len()]
    }
}

pub fn syntax(visuals: &Visuals, syntax: Syntax) -> Color32 {
    let index = match syntax {
        Syntax::Comment => 0,
        Syntax::Function => 1,
        Syntax::Keyword => 2,
        Syntax::Literal => 3,
        Syntax::String => 4,
        Syntax::Punctuation => 5,
    };
    palette(visuals).syntax[index]
}

pub fn added_fill(visuals: &Visuals, emphasis: bool) -> Color32 {
    tint(palette(visuals).added, visuals, emphasis)
}

pub fn removed_fill(visuals: &Visuals, emphasis: bool) -> Color32 {
    tint(palette(visuals).removed, visuals, emphasis)
}

fn tint(color: Color32, visuals: &Visuals, emphasis: bool) -> Color32 {
    let alpha = match (visuals.dark_mode, emphasis) {
        (true, false) => 0.10,
        (true, true) => 0.28,
        (false, false) => 0.10,
        (false, true) => 0.25,
    };
    color.gamma_multiply(alpha)
}
