use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Vec2,
};

use super::config::ThemeMode;

// Warm paper, ink and a single signal accent. The dark palette is graphite
// with the same orange signal colour.
const ACCENT: Color32 = Color32::from_rgb(209, 73, 44);

const LIGHT_BG: Color32 = Color32::from_rgb(243, 241, 235);
const LIGHT_PANEL: Color32 = Color32::from_rgb(250, 249, 245);
const LIGHT_INPUT: Color32 = Color32::from_rgb(236, 233, 226);
const LIGHT_LINE: Color32 = Color32::from_rgb(210, 205, 195);
const LIGHT_LINE_SOFT: Color32 = Color32::from_rgb(227, 223, 214);
const LIGHT_TEXT: Color32 = Color32::from_rgb(24, 23, 20);
const LIGHT_MUTED: Color32 = Color32::from_rgb(106, 102, 94);
const LIGHT_DIM: Color32 = Color32::from_rgb(143, 137, 127);
const LIGHT_WARNING: Color32 = Color32::from_rgb(164, 98, 15);
const LIGHT_DANGER: Color32 = Color32::from_rgb(169, 46, 34);

const DARK_BG: Color32 = Color32::from_rgb(24, 24, 22);
const DARK_PANEL: Color32 = Color32::from_rgb(33, 33, 30);
const DARK_INPUT: Color32 = Color32::from_rgb(43, 42, 38);
const DARK_LINE: Color32 = Color32::from_rgb(69, 67, 60);
const DARK_LINE_SOFT: Color32 = Color32::from_rgb(50, 49, 44);
const DARK_TEXT: Color32 = Color32::from_rgb(238, 235, 226);
const DARK_MUTED: Color32 = Color32::from_rgb(174, 168, 156);
const DARK_DIM: Color32 = Color32::from_rgb(126, 121, 112);
const DARK_WARNING: Color32 = Color32::from_rgb(226, 166, 74);
const DARK_DANGER: Color32 = Color32::from_rgb(234, 108, 94);

const SANS_REGULAR: &str = "plex-sans-regular";
const SANS_MEDIUM: &str = "plex-sans-medium";
const SANS_SEMIBOLD: &str = "plex-sans-semibold";
const MONO_REGULAR: &str = "plex-mono-regular";
const MONO_MEDIUM: &str = "plex-mono-medium";

const FAMILY_SANS_MEDIUM: &str = "plex-sans-medium-family";
const FAMILY_SANS_SEMIBOLD: &str = "plex-sans-semibold-family";
const FAMILY_MONO_MEDIUM: &str = "plex-mono-medium-family";

static DARK: AtomicBool = AtomicBool::new(false);

pub fn install(ctx: &egui::Context, mode: ThemeMode) {
    install_fonts(ctx);
    DARK.store(mode == ThemeMode::Dark, Ordering::Relaxed);
    install_style(ctx);
    ctx.set_theme(theme_preference(mode));
}

pub fn set_mode(ctx: &egui::Context, mode: ThemeMode) {
    DARK.store(mode == ThemeMode::Dark, Ordering::Relaxed);
    ctx.set_theme(theme_preference(mode));
    ctx.request_repaint();
}

fn theme_preference(mode: ThemeMode) -> egui::ThemePreference {
    match mode {
        ThemeMode::Light => egui::ThemePreference::Light,
        ThemeMode::Dark => egui::ThemePreference::Dark,
    }
}

fn dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

pub fn bg() -> Color32 {
    if dark() { DARK_BG } else { LIGHT_BG }
}

pub fn panel() -> Color32 {
    if dark() { DARK_PANEL } else { LIGHT_PANEL }
}

pub fn input() -> Color32 {
    if dark() { DARK_INPUT } else { LIGHT_INPUT }
}

pub fn line() -> Color32 {
    if dark() { DARK_LINE } else { LIGHT_LINE }
}

pub fn line_soft() -> Color32 {
    if dark() {
        DARK_LINE_SOFT
    } else {
        LIGHT_LINE_SOFT
    }
}

pub fn text() -> Color32 {
    if dark() { DARK_TEXT } else { LIGHT_TEXT }
}

pub fn muted() -> Color32 {
    if dark() { DARK_MUTED } else { LIGHT_MUTED }
}

pub fn dim() -> Color32 {
    if dark() { DARK_DIM } else { LIGHT_DIM }
}

pub fn accent() -> Color32 {
    ACCENT
}

pub fn warning() -> Color32 {
    if dark() { DARK_WARNING } else { LIGHT_WARNING }
}

pub fn danger() -> Color32 {
    if dark() { DARK_DANGER } else { LIGHT_DANGER }
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

#[cfg(not(windows))]
pub fn shadow() -> Shadow {
    Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 0,
        color: if dark() {
            Color32::from_black_alpha(120)
        } else {
            Color32::from_black_alpha(38)
        },
    }
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
    let light = build_style((*base).clone(), false);
    let dark = build_style((*base).clone(), true);

    ctx.set_style_of(egui::Theme::Light, light);
    ctx.set_style_of(egui::Theme::Dark, dark);
}

fn build_style(mut style: egui::Style, dark: bool) -> egui::Style {
    style.animation_time = 0.16;
    style.spacing.item_spacing = Vec2::new(8.0, 3.0);
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    style.spacing.interact_size = Vec2::new(44.0, 30.0);
    style.spacing.slider_width = 180.0;
    style.spacing.menu_margin = Margin::same(8);
    style.spacing.window_margin = Margin::same(16);
    style.spacing.indent = 16.0;
    style.spacing.scroll = egui::style::ScrollStyle::thin();

    style.text_styles.insert(TextStyle::Small, sans(11.0));
    style.text_styles.insert(TextStyle::Body, sans(13.5));
    style
        .text_styles
        .insert(TextStyle::Button, sans_medium(13.0));
    style.text_styles.insert(TextStyle::Monospace, mono(12.0));
    style
        .text_styles
        .insert(TextStyle::Heading, sans_semibold(22.0));

    style.visuals = visuals(dark);
    style
}

fn visuals(dark_mode: bool) -> egui::Visuals {
    let bg = if dark_mode { DARK_BG } else { LIGHT_BG };
    let panel = if dark_mode { DARK_PANEL } else { LIGHT_PANEL };
    let input = if dark_mode { DARK_INPUT } else { LIGHT_INPUT };
    let line = if dark_mode { DARK_LINE } else { LIGHT_LINE };
    let text = if dark_mode { DARK_TEXT } else { LIGHT_TEXT };
    let warning = if dark_mode {
        DARK_WARNING
    } else {
        LIGHT_WARNING
    };
    let danger = if dark_mode { DARK_DANGER } else { LIGHT_DANGER };

    let mut visuals = if dark_mode {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.dark_mode = dark_mode;
    visuals.panel_fill = bg;
    visuals.window_fill = panel;
    visuals.faint_bg_color = input;
    visuals.extreme_bg_color = panel;
    visuals.code_bg_color = input;
    visuals.window_stroke = Stroke::new(1.0_f32, line);
    visuals.window_corner_radius = CornerRadius::same(16);
    visuals.window_shadow = shadow_for(dark_mode);
    visuals.popup_shadow = shadow_for(dark_mode);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = warning;
    visuals.error_fg_color = danger;
    visuals.selection.bg_fill = alpha(ACCENT, if dark_mode { 64 } else { 40 });
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.slider_trailing_fill = true;
    visuals.text_cursor.stroke = Stroke::new(1.5_f32, ACCENT);

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
        bg_fill: Color32::TRANSPARENT,
        weak_bg_fill: Color32::TRANSPARENT,
        bg_stroke: Stroke::NONE,
        corner_radius: CornerRadius::same(8),
        fg_stroke: Stroke::new(1.0_f32, text),
        expansion: 0.0,
    };
    widgets.hovered = egui::style::WidgetVisuals {
        bg_fill: alpha(text, if dark_mode { 14 } else { 7 }),
        weak_bg_fill: alpha(text, if dark_mode { 14 } else { 7 }),
        bg_stroke: Stroke::NONE,
        corner_radius: CornerRadius::same(8),
        fg_stroke: Stroke::new(1.0_f32, text),
        expansion: 0.0,
    };
    widgets.active = egui::style::WidgetVisuals {
        bg_fill: alpha(ACCENT, if dark_mode { 48 } else { 20 }),
        weak_bg_fill: alpha(ACCENT, if dark_mode { 48 } else { 20 }),
        bg_stroke: Stroke::NONE,
        corner_radius: CornerRadius::same(8),
        fg_stroke: Stroke::new(1.0_f32, text),
        expansion: 0.0,
    };
    visuals.widgets = widgets;

    visuals
}

fn shadow_for(dark_mode: bool) -> Shadow {
    Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 0,
        color: Color32::from_black_alpha(if dark_mode { 120 } else { 38 }),
    }
}

fn named_family(name: &str) -> FontFamily {
    FontFamily::Name(Arc::from(name))
}
