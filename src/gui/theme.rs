use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Vec2,
};

use super::config::Theme;

// Each palette keeps the same signal accent for active clicks while the
// surrounding paper, graphite, sand, ocean, forest or contrast surface changes.
#[derive(Clone, Copy)]
struct Palette {
    dark: bool,
    bg: Color32,
    panel: Color32,
    input: Color32,
    line: Color32,
    line_soft: Color32,
    text: Color32,
    muted: Color32,
    dim: Color32,
    warning: Color32,
    danger: Color32,
    accent: Color32,
}

const PAPER: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(243, 241, 235),
    panel: Color32::from_rgb(250, 249, 245),
    input: Color32::from_rgb(236, 233, 226),
    line: Color32::from_rgb(210, 205, 195),
    line_soft: Color32::from_rgb(227, 223, 214),
    text: Color32::from_rgb(24, 23, 20),
    muted: Color32::from_rgb(106, 102, 94),
    dim: Color32::from_rgb(143, 137, 127),
    warning: Color32::from_rgb(164, 98, 15),
    danger: Color32::from_rgb(169, 46, 34),
    accent: Color32::from_rgb(209, 73, 44),
};

const GRAPHITE: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(24, 24, 22),
    panel: Color32::from_rgb(33, 33, 30),
    input: Color32::from_rgb(43, 42, 38),
    line: Color32::from_rgb(69, 67, 60),
    line_soft: Color32::from_rgb(50, 49, 44),
    text: Color32::from_rgb(238, 235, 226),
    muted: Color32::from_rgb(174, 168, 156),
    dim: Color32::from_rgb(126, 121, 112),
    warning: Color32::from_rgb(226, 166, 74),
    danger: Color32::from_rgb(234, 108, 94),
    accent: Color32::from_rgb(209, 73, 44),
};

const SAND: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(243, 235, 219),
    panel: Color32::from_rgb(250, 244, 232),
    input: Color32::from_rgb(235, 225, 205),
    line: Color32::from_rgb(216, 203, 179),
    line_soft: Color32::from_rgb(231, 220, 199),
    text: Color32::from_rgb(52, 42, 28),
    muted: Color32::from_rgb(124, 109, 89),
    dim: Color32::from_rgb(157, 144, 124),
    warning: Color32::from_rgb(171, 102, 18),
    danger: Color32::from_rgb(170, 52, 37),
    accent: Color32::from_rgb(184, 86, 46),
};

const OCEAN: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(17, 26, 36),
    panel: Color32::from_rgb(24, 35, 47),
    input: Color32::from_rgb(32, 46, 60),
    line: Color32::from_rgb(52, 70, 88),
    line_soft: Color32::from_rgb(37, 52, 67),
    text: Color32::from_rgb(226, 237, 245),
    muted: Color32::from_rgb(156, 177, 194),
    dim: Color32::from_rgb(110, 131, 148),
    warning: Color32::from_rgb(229, 172, 80),
    danger: Color32::from_rgb(238, 112, 104),
    accent: Color32::from_rgb(78, 178, 199),
};

const FOREST: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(19, 27, 22),
    panel: Color32::from_rgb(26, 36, 30),
    input: Color32::from_rgb(35, 48, 40),
    line: Color32::from_rgb(56, 74, 62),
    line_soft: Color32::from_rgb(41, 56, 47),
    text: Color32::from_rgb(228, 238, 229),
    muted: Color32::from_rgb(159, 177, 161),
    dim: Color32::from_rgb(112, 132, 115),
    warning: Color32::from_rgb(222, 169, 79),
    danger: Color32::from_rgb(234, 112, 96),
    accent: Color32::from_rgb(112, 180, 128),
};

const CONTRAST: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(255, 255, 255),
    panel: Color32::from_rgb(248, 248, 248),
    input: Color32::from_rgb(235, 235, 235),
    line: Color32::from_rgb(28, 28, 28),
    line_soft: Color32::from_rgb(160, 160, 160),
    text: Color32::from_rgb(0, 0, 0),
    muted: Color32::from_rgb(55, 55, 55),
    dim: Color32::from_rgb(90, 90, 90),
    warning: Color32::from_rgb(150, 80, 0),
    danger: Color32::from_rgb(190, 0, 0),
    accent: Color32::from_rgb(250, 204, 21),
};

const SANS_REGULAR: &str = "plex-sans-regular";
const SANS_MEDIUM: &str = "plex-sans-medium";
const SANS_SEMIBOLD: &str = "plex-sans-semibold";
const MONO_REGULAR: &str = "plex-mono-regular";
const MONO_MEDIUM: &str = "plex-mono-medium";

const FAMILY_SANS_MEDIUM: &str = "plex-sans-medium-family";
const FAMILY_SANS_SEMIBOLD: &str = "plex-sans-semibold-family";
const FAMILY_MONO_MEDIUM: &str = "plex-mono-medium-family";

static CURRENT: AtomicUsize = AtomicUsize::new(0);

pub fn install(ctx: &egui::Context, theme: Theme) {
    install_fonts(ctx);
    CURRENT.store(theme.index(), Ordering::Relaxed);
    install_style(ctx);
    ctx.set_theme(theme_preference(theme));
}

pub fn set_theme(ctx: &egui::Context, theme: Theme) {
    CURRENT.store(theme.index(), Ordering::Relaxed);
    install_style(ctx);
    ctx.set_theme(theme_preference(theme));
    ctx.request_repaint();
}

fn theme_preference(theme: Theme) -> egui::ThemePreference {
    if theme.is_dark() {
        egui::ThemePreference::Dark
    } else {
        egui::ThemePreference::Light
    }
}

fn palette() -> Palette {
    match Theme::from_index(CURRENT.load(Ordering::Relaxed)) {
        Theme::Paper => PAPER,
        Theme::Graphite => GRAPHITE,
        Theme::Sand => SAND,
        Theme::Ocean => OCEAN,
        Theme::Forest => FOREST,
        Theme::Contrast => CONTRAST,
    }
}

pub fn bg() -> Color32 {
    palette().bg
}

pub fn panel() -> Color32 {
    palette().panel
}

pub fn input() -> Color32 {
    palette().input
}

pub fn line() -> Color32 {
    palette().line
}

pub fn line_soft() -> Color32 {
    palette().line_soft
}

pub fn text() -> Color32 {
    palette().text
}

pub fn muted() -> Color32 {
    palette().muted
}

pub fn dim() -> Color32 {
    palette().dim
}

pub fn accent() -> Color32 {
    palette().accent
}

/// Readable foreground for content drawn on top of [`accent`].
pub fn on_accent() -> Color32 {
    let accent = palette().accent;
    let luma = 0.299 * accent.r() as f32 + 0.587 * accent.g() as f32 + 0.114 * accent.b() as f32;
    if luma > 150.0 {
        Color32::from_rgb(24, 23, 20)
    } else {
        Color32::from_rgb(252, 250, 245)
    }
}

pub fn warning() -> Color32 {
    palette().warning
}

pub fn danger() -> Color32 {
    palette().danger
}

pub fn sans(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn sans_medium(size: f32) -> FontId {
    FontId::new(size, named_family(FAMILY_SANS_MEDIUM))
}

pub fn sans_semibold(size: f32) -> FontId {
    FontId::new(size, named_family(FAMILY_SANS_SEMIBOLD))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn mono_medium(size: f32) -> FontId {
    FontId::new(size, named_family(FAMILY_MONO_MEDIUM))
}

pub fn alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

pub fn mix(from: Color32, to: Color32, t: f32) -> Color32 {
    from.lerp_to_gamma(to, t.clamp(0.0, 1.0))
}

pub fn shadow() -> Shadow {
    shadow_for(palette().dark)
}

pub fn rule(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), egui::Sense::hover());
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        Stroke::new(1.0_f32, line_soft()),
    );
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    let files: [(&str, &'static [u8]); 5] = [
        (
            SANS_REGULAR,
            include_bytes!("../../assets/fonts/IBMPlexSans-Regular.ttf").as_slice(),
        ),
        (
            SANS_MEDIUM,
            include_bytes!("../../assets/fonts/IBMPlexSans-Medium.ttf").as_slice(),
        ),
        (
            SANS_SEMIBOLD,
            include_bytes!("../../assets/fonts/IBMPlexSans-SemiBold.ttf").as_slice(),
        ),
        (
            MONO_REGULAR,
            include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf").as_slice(),
        ),
        (
            MONO_MEDIUM,
            include_bytes!("../../assets/fonts/IBMPlexMono-Medium.ttf").as_slice(),
        ),
    ];

    for (name, bytes) in files {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }

    prepend(&mut fonts, FontFamily::Proportional, SANS_REGULAR);
    prepend(&mut fonts, FontFamily::Monospace, MONO_REGULAR);

    fonts.families.insert(
        named_family(FAMILY_SANS_MEDIUM),
        vec![SANS_MEDIUM.to_owned(), SANS_REGULAR.to_owned()],
    );
    fonts.families.insert(
        named_family(FAMILY_SANS_SEMIBOLD),
        vec![SANS_SEMIBOLD.to_owned(), SANS_REGULAR.to_owned()],
    );
    fonts.families.insert(
        named_family(FAMILY_MONO_MEDIUM),
        vec![MONO_MEDIUM.to_owned(), MONO_REGULAR.to_owned()],
    );

    ctx.set_fonts(fonts);
}

fn prepend(fonts: &mut FontDefinitions, family: FontFamily, name: &str) {
    let entries = fonts.families.entry(family).or_default();
    entries.retain(|entry| entry != name);
    entries.insert(0, name.to_owned());
}

fn install_style(ctx: &egui::Context) {
    let base = ctx.style_of(egui::Theme::Light);
    let style = build_style((*base).clone(), palette());

    ctx.set_style_of(egui::Theme::Light, style.clone());
    ctx.set_style_of(egui::Theme::Dark, style);
}

fn build_style(mut style: egui::Style, palette: Palette) -> egui::Style {
    style.animation_time = 0.16;
    style.spacing.item_spacing = Vec2::new(8.0, 3.0);
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    style.spacing.interact_size = Vec2::new(44.0, 30.0);
    style.spacing.slider_width = 180.0;
    style.spacing.menu_margin = Margin::same(8);
    style.spacing.window_margin = Margin::same(16);
    style.spacing.indent = 16.0;
    style.spacing.scroll = egui::style::ScrollStyle::thin();

    style.text_styles.insert(TextStyle::Small, sans(11.5));
    style.text_styles.insert(TextStyle::Body, sans(14.0));
    style
        .text_styles
        .insert(TextStyle::Button, sans_medium(13.5));
    style.text_styles.insert(TextStyle::Monospace, mono(12.5));
    style
        .text_styles
        .insert(TextStyle::Heading, sans_semibold(22.0));

    style.visuals = visuals(palette);
    style
}

fn visuals(palette: Palette) -> egui::Visuals {
    let Palette {
        dark,
        bg,
        panel,
        input,
        line,
        text,
        warning,
        danger,
        accent,
        ..
    } = palette;

    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.dark_mode = dark;
    visuals.panel_fill = bg;
    visuals.window_fill = panel;
    visuals.faint_bg_color = input;
    visuals.extreme_bg_color = panel;
    visuals.code_bg_color = input;
    visuals.window_stroke = Stroke::new(1.0_f32, line);
    visuals.window_corner_radius = CornerRadius::same(16);
    visuals.window_shadow = shadow_for(dark);
    visuals.popup_shadow = shadow_for(dark);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.hyperlink_color = accent;
    visuals.warn_fg_color = warning;
    visuals.error_fg_color = danger;
    visuals.selection.bg_fill = alpha(accent, if dark { 64 } else { 40 });
    visuals.selection.stroke = Stroke::new(1.0_f32, accent);
    visuals.slider_trailing_fill = true;
    visuals.text_cursor.stroke = Stroke::new(1.5_f32, accent);

    let mut widgets = visuals.widgets.clone();
    widgets.noninteractive = egui::style::WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: bg,
        bg_stroke: Stroke::NONE,
        corner_radius: CornerRadius::same(8),
        fg_stroke: Stroke::new(1.0_f32, text),
        expansion: 0.0,
    };
    widgets.inactive = egui::style::WidgetVisuals {
        bg_fill: input,
        weak_bg_fill: input,
        bg_stroke: Stroke::new(1.0_f32, line),
        corner_radius: CornerRadius::same(7),
        fg_stroke: Stroke::new(1.0_f32, text),
        expansion: 0.0,
    };
    widgets.hovered = egui::style::WidgetVisuals {
        bg_fill: mix(input, accent, if dark { 0.14 } else { 0.08 }),
        weak_bg_fill: mix(input, accent, if dark { 0.14 } else { 0.08 }),
        bg_stroke: Stroke::new(1.0_f32, alpha(accent, 170)),
        corner_radius: CornerRadius::same(7),
        fg_stroke: Stroke::new(1.0_f32, text),
        expansion: 0.0,
    };
    widgets.active = egui::style::WidgetVisuals {
        bg_fill: alpha(accent, if dark { 52 } else { 26 }),
        weak_bg_fill: alpha(accent, if dark { 52 } else { 26 }),
        bg_stroke: Stroke::new(1.0_f32, accent),
        corner_radius: CornerRadius::same(7),
        fg_stroke: Stroke::new(1.0_f32, text),
        expansion: 0.0,
    };
    widgets.open = egui::style::WidgetVisuals {
        bg_fill: mix(input, accent, if dark { 0.18 } else { 0.10 }),
        weak_bg_fill: mix(input, accent, if dark { 0.18 } else { 0.10 }),
        bg_stroke: Stroke::new(1.0_f32, accent),
        corner_radius: CornerRadius::same(7),
        fg_stroke: Stroke::new(1.0_f32, text),
        expansion: 0.0,
    };
    visuals.widgets = widgets;

    visuals
}

fn shadow_for(dark: bool) -> Shadow {
    Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 120 } else { 38 }),
    }
}

fn named_family(name: &str) -> FontFamily {
    FontFamily::Name(Arc::from(name))
}
