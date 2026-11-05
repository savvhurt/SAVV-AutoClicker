use std::sync::Arc;

use egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Vec2,
};

// Warm paper, ink and a single signal accent.
pub const BG: Color32 = Color32::from_rgb(243, 241, 235);
pub const PANEL: Color32 = Color32::from_rgb(250, 249, 245);
pub const INPUT: Color32 = Color32::from_rgb(236, 233, 226);
pub const LINE: Color32 = Color32::from_rgb(210, 205, 195);
pub const LINE_SOFT: Color32 = Color32::from_rgb(227, 223, 214);

pub const TEXT: Color32 = Color32::from_rgb(24, 23, 20);
pub const MUTED: Color32 = Color32::from_rgb(106, 102, 94);
pub const DIM: Color32 = Color32::from_rgb(143, 137, 127);

pub const ACCENT: Color32 = Color32::from_rgb(209, 73, 44);
pub const WARNING: Color32 = Color32::from_rgb(164, 98, 15);
pub const DANGER: Color32 = Color32::from_rgb(169, 46, 34);

const SANS_REGULAR: &str = "plex-sans-regular";
const SANS_MEDIUM: &str = "plex-sans-medium";
const SANS_SEMIBOLD: &str = "plex-sans-semibold";
const MONO_REGULAR: &str = "plex-mono-regular";
const MONO_MEDIUM: &str = "plex-mono-medium";

const FAMILY_SANS_MEDIUM: &str = "plex-sans-medium-family";
const FAMILY_SANS_SEMIBOLD: &str = "plex-sans-semibold-family";
const FAMILY_MONO_MEDIUM: &str = "plex-mono-medium-family";

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    install_style(ctx);
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
    Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 0,
        color: Color32::from_black_alpha(38),
    }
}

pub fn rule(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), egui::Sense::hover());
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        Stroke::new(1.0_f32, LINE_SOFT),
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
    let mut style = (*ctx.style_of(egui::Theme::Light)).clone();

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

    style.visuals = visuals();

    ctx.set_style_of(egui::Theme::Light, style);
    ctx.set_theme(egui::ThemePreference::Light);
}

fn visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::light();
    visuals.dark_mode = false;
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.faint_bg_color = INPUT;
    visuals.extreme_bg_color = PANEL;
    visuals.code_bg_color = INPUT;
    visuals.window_stroke = Stroke::new(1.0_f32, LINE);
    visuals.window_corner_radius = CornerRadius::same(16);
    visuals.window_shadow = shadow();
    visuals.popup_shadow = shadow();
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = WARNING;
    visuals.error_fg_color = DANGER;
    visuals.selection.bg_fill = alpha(ACCENT, 40);
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.slider_trailing_fill = true;
    visuals.text_cursor.stroke = Stroke::new(1.5_f32, ACCENT);

    let mut widgets = visuals.widgets.clone();
    widgets.noninteractive = egui::style::WidgetVisuals {
        bg_fill: BG,
        weak_bg_fill: BG,
        bg_stroke: Stroke::NONE,
        corner_radius: CornerRadius::same(8),
        fg_stroke: Stroke::new(1.0_f32, TEXT),
        expansion: 0.0,
    };
    widgets.inactive = egui::style::WidgetVisuals {
        bg_fill: Color32::TRANSPARENT,
        weak_bg_fill: Color32::TRANSPARENT,
        bg_stroke: Stroke::NONE,
        corner_radius: CornerRadius::same(8),
        fg_stroke: Stroke::new(1.0_f32, TEXT),
        expansion: 0.0,
    };
    widgets.hovered = egui::style::WidgetVisuals {
        bg_fill: alpha(TEXT, 7),
        weak_bg_fill: alpha(TEXT, 7),
        bg_stroke: Stroke::NONE,
        corner_radius: CornerRadius::same(8),
        fg_stroke: Stroke::new(1.0_f32, TEXT),
        expansion: 0.0,
    };
    widgets.active = egui::style::WidgetVisuals {
        bg_fill: alpha(ACCENT, 20),
        weak_bg_fill: alpha(ACCENT, 20),
        bg_stroke: Stroke::NONE,
        corner_radius: CornerRadius::same(8),
        fg_stroke: Stroke::new(1.0_f32, TEXT),
        expansion: 0.0,
    };
    visuals.widgets = widgets;

    visuals
}

fn named_family(name: &str) -> FontFamily {
    FontFamily::Name(Arc::from(name))
}
