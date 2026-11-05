use egui::{
    self, Align, Color32, CornerRadius, CursorIcon, Layout, Painter, Pos2, Rect, Response,
    RichText, Sense, Shape, Stroke, TextStyle, Ui, Vec2, pos2, vec2,
};

use super::theme;

pub fn status_badge(ui: &mut Ui, text: &str, dot: Color32, pulse: bool) -> Response {
    let text_color = if pulse { theme::TEXT } else { theme::MUTED };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), theme::mono_medium(8.5), text_color);
    let size = Vec2::new(galley.size().x + 16.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());

    let center = pos2(rect.left() + 3.0, rect.center().y);
    let painter = ui.painter();
    if pulse {
        painter.circle_filled(center, 5.0, theme::alpha(dot, 22));
    }
    painter.circle_filled(center, 2.4, dot);
    painter.galley(
        pos2(rect.left() + 11.0, rect.center().y - galley.size().y * 0.5),
        galley,
        text_color,
    );

    response
}

pub fn power_button(ui: &mut Ui, active: bool, busy: bool, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let response = if busy {
        response
    } else {
        response.on_hover_cursor(CursorIcon::PointingHand)
    };

    let ctx = ui.ctx().clone();
    let hover_t =
        ctx.animate_bool_with_time(response.id.with("hover"), response.hovered() && !busy, 0.16);
    let active_t = ctx.animate_bool_with_time(response.id.with("active"), active, 0.20);
    let time = ctx.input(|input| input.time) as f32;

    let center = rect.center();
    let radius = size * 0.40;
    let outer = size * 0.06;
    let painter = ui.painter();

    if hover_t > 0.01 {
        painter.circle_filled(
            center,
            radius,
            theme::alpha(theme::TEXT, (5.0 * hover_t) as u8),
        );
    }
    if active_t > 0.01 {
        painter.circle_filled(
            center,
            radius,
            theme::alpha(theme::ACCENT, (255.0 * active_t) as u8),
        );
        painter.circle_stroke(
            center,
            radius + outer,
            Stroke::new(
                1.0_f32,
                theme::alpha(theme::ACCENT, (46.0 * active_t) as u8),
            ),
        );
    }

    let ring = theme::mix(
        theme::TEXT,
        theme::mix(theme::ACCENT, theme::TEXT, 0.3),
        active_t,
    );
    painter.circle_stroke(center, radius, Stroke::new(1.2 + hover_t * 0.4, ring));

    let glyph = theme::mix(theme::TEXT, theme::BG, active_t);
    paint_power_glyph(
        painter,
        center,
        radius * 0.36,
        glyph,
        (size * 0.021).max(1.8),
    );

    if busy {
        arc(
            painter,
            center,
            radius + outer + size * 0.03,
            time * 3.2,
            time * 3.2 + 1.5,
            Stroke::new(1.5_f32, theme::ACCENT),
        );
    }

    response
}

pub fn paint_power_glyph(painter: &Painter, center: Pos2, radius: f32, color: Color32, width: f32) {
    let stroke = Stroke::new(width, color);
    arc(painter, center, radius, -0.87, 4.01, stroke);
    painter.line_segment(
        [
            center + vec2(0.0, -radius * 1.24),
            center + vec2(0.0, -radius * 0.18),
        ],
        stroke,
    );
}

pub fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash,
    selected: &mut T,
    options: &[(T, &str)],
) -> bool {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
    let response = ui
        .interact(rect, ui.make_persistent_id(id_salt), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    let count = options.len().max(1);
    let segment_width = rect.width() / count as f32;

    let pointer = ui.ctx().pointer_hover_pos();
    let hovered = pointer.and_then(|position| {
        if rect.contains(position) {
            Some(
                ((position.x - rect.left()) / segment_width)
                    .floor()
                    .clamp(0.0, (count - 1) as f32) as usize,
            )
        } else {
            None
        }
    });

    let mut clicked = None;
    if response.clicked() {
        if let Some(position) = response.interact_pointer_pos() {
            if rect.contains(position) {
                clicked = Some(
                    ((position.x - rect.left()) / segment_width)
                        .floor()
                        .clamp(0.0, (count - 1) as f32) as usize,
                );
            }
        }
    }

    let before = *selected;
    let painter = ui.painter();
    painter.line_segment(
        [
            pos2(rect.left(), rect.bottom() - 0.5),
            pos2(rect.right(), rect.bottom() - 0.5),
        ],
        Stroke::new(1.0_f32, theme::LINE_SOFT),
    );

    for (index, (value, label)) in options.iter().enumerate() {
        let segment = Rect::from_min_size(
            pos2(rect.left() + index as f32 * segment_width, rect.top()),
            vec2(segment_width, rect.height()),
        );
        let is_selected = *value == before;
        let is_hovered = hovered == Some(index);

        if is_hovered && !is_selected {
            painter.rect_filled(
                segment.shrink2(vec2(2.0, 3.0)),
                CornerRadius::same(6),
                theme::alpha(theme::TEXT, 5),
            );
        }

        let color = if is_selected || is_hovered {
            theme::TEXT
        } else {
            theme::MUTED
        };
        let galley = painter.layout_no_wrap((*label).to_owned(), theme::mono_medium(8.5), color);
        let text_pos = pos2(
            segment.center().x - galley.size().x * 0.5,
            segment.center().y - galley.size().y * 0.5,
        );
        painter.galley(text_pos, galley.clone(), color);

        if is_selected {
            let half = galley.size().x * 0.5 + 7.0;
            painter.line_segment(
                [
                    pos2(segment.center().x - half, rect.bottom() - 1.5),
                    pos2(segment.center().x + half, rect.bottom() - 1.5),
                ],
                Stroke::new(1.5_f32, theme::ACCENT),
            );
        }
    }

    if let Some(index) = clicked {
        *selected = options[index].0;
    }

    *selected != before
}

pub fn parameter_slider(
    ui: &mut Ui,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    logarithmic: bool,
    step: f64,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 14.0),
        Sense::click_and_drag(),
    );
    let mut response = response.on_hover_cursor(CursorIcon::ResizeHorizontal);

    let before = *value;
    if response.dragged() || response.clicked() {
        if let Some(position) = response.interact_pointer_pos() {
            let left = rect.left() + 7.0;
            let right = rect.right() - 7.0;
            let t = ((position.x - left) / (right - left).max(1.0)).clamp(0.0, 1.0);
            let mut next = from_norm(t, &range, logarithmic);
            if step > 0.0 {
                next = (next / step).round() * step;
            }
            *value = next.clamp(*range.start(), *range.end());
        }
    }

    if (*value - before).abs() > f64::EPSILON {
        response.mark_changed();
    }

    let t = to_norm(*value, &range, logarithmic);
    let left = rect.left() + 7.0;
    let right = rect.right() - 7.0;
    let center_y = rect.center().y;
    let handle_x = left + t * (right - left);

    let painter = ui.painter();
    let track = Rect::from_min_max(pos2(left, center_y - 1.5), pos2(right, center_y + 1.5));
    painter.rect_filled(track, CornerRadius::same(2), theme::LINE);

    if handle_x > left + 0.5 {
        let fill = Rect::from_min_max(pos2(left, center_y - 1.5), pos2(handle_x, center_y + 1.5));
        painter.rect_filled(fill, CornerRadius::same(2), theme::ACCENT);
    }

    let hot = response.hovered() || response.dragged();
    let handle = pos2(handle_x, center_y);
    let radius = if hot { 5.0 } else { 4.0 };
    painter.circle_filled(handle, radius, if hot { theme::ACCENT } else { theme::BG });
    painter.circle_stroke(handle, radius, Stroke::new(1.5_f32, theme::ACCENT));

    response
}

pub fn numeric_field(
    ui: &mut Ui,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
    decimals: usize,
    suffix: &str,
) -> bool {
    ui.spacing_mut().item_spacing.x = 5.0;
    ui.label(
        RichText::new(suffix)
            .font(theme::mono_medium(8.0))
            .color(theme::DIM),
    );

    let (rect, _) = ui.allocate_exact_size(Vec2::new(54.0, 18.0), Sense::hover());
    let hovered = ui.rect_contains_pointer(rect);

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    child.style_mut().override_text_style = Some(TextStyle::Monospace);
    child.style_mut().override_font_id = Some(theme::mono_medium(12.0));
    child.spacing_mut().interact_size = Vec2::new(46.0, 18.0);
    child.visuals_mut().extreme_bg_color = theme::PANEL;
    child.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    child.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
    child.visuals_mut().widgets.hovered.weak_bg_fill = Color32::TRANSPARENT;
    child.visuals_mut().widgets.hovered.bg_stroke = Stroke::NONE;
    child.visuals_mut().widgets.active.weak_bg_fill = theme::alpha(theme::ACCENT, 14);
    child.visuals_mut().widgets.active.bg_stroke = Stroke::NONE;

    let changed = child
        .add_sized(
            Vec2::new(46.0, 18.0),
            egui::DragValue::new(value)
                .range(range)
                .speed(speed)
                .max_decimals(decimals),
        )
        .changed();

    ui.painter().line_segment(
        [
            pos2(rect.left() + 2.0, rect.bottom() - 0.5),
            pos2(rect.right() - 2.0, rect.bottom() - 0.5),
        ],
        Stroke::new(1.0_f32, if hovered { theme::MUTED } else { theme::LINE }),
    );

    changed
}

pub fn banner(ui: &mut Ui, tone: Color32, title: &str, text: &str) {
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(Vec2::splat(6.0), Sense::hover());
        ui.painter().circle_filled(dot.center(), 3.0, tone);
        ui.label(
            RichText::new(title)
                .font(theme::mono_medium(8.5))
                .color(tone),
        );
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .font(theme::sans(11.0))
                    .color(theme::TEXT),
            )
            .wrap(),
        );
    });
}

pub fn text_button(ui: &mut Ui, label: &str, idle: Color32, hover: Color32) -> Response {
    let enabled = ui.is_enabled();
    let idle = if enabled { idle } else { theme::DIM };
    let prepared = ui
        .painter()
        .layout_no_wrap(label.to_owned(), theme::sans_medium(10.5), idle);
    let size = Vec2::new(prepared.size().x + 20.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(
        size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let response = if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    };

    let color = if enabled && response.hovered() {
        hover
    } else {
        idle
    };
    let painter = ui.painter();
    if enabled && response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(6), theme::alpha(hover, 8));
    }
    let galley = painter.layout_no_wrap(label.to_owned(), theme::sans_medium(10.5), color);
    painter.galley(
        pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );

    response
}

pub fn solid_button(ui: &mut Ui, label: &str, fill: Color32, text: Color32) -> Response {
    let button = egui::Button::new(
        RichText::new(label)
            .font(theme::sans_medium(11.5))
            .color(text),
    )
    .fill(fill)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(8))
    .min_size(Vec2::new(0.0, 32.0));
    ui.add(button)
}

fn arc(painter: &Painter, center: Pos2, radius: f32, start: f32, end: f32, stroke: Stroke) {
    const STEPS: usize = 48;
    let points = (0..=STEPS)
        .map(|index| {
            let t = index as f32 / STEPS as f32;
            let angle = start + (end - start) * t;
            center + vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();
    painter.add(Shape::line(points, stroke));
}

fn to_norm(value: f64, range: &std::ops::RangeInclusive<f64>, logarithmic: bool) -> f32 {
    let start = *range.start();
    let end = *range.end();
    if end <= start {
        return 0.0;
    }
    let value = value.clamp(start, end);

    let t = if logarithmic && start > 0.0 {
        (value / start).ln() / (end / start).ln()
    } else {
        (value - start) / (end - start)
    };

    t.clamp(0.0, 1.0) as f32
}

fn from_norm(t: f32, range: &std::ops::RangeInclusive<f64>, logarithmic: bool) -> f64 {
    let start = *range.start();
    let end = *range.end();
    let t = t.clamp(0.0, 1.0) as f64;

    if logarithmic && start > 0.0 {
        start * (end / start).powf(t)
    } else {
        start + (end - start) * t
    }
}
