use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, Rect, Response, Sense, Stroke,
    StrokeKind, Ui, Vec2,
};

use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolIcon {
    Search,
    Bolt,
    Document,
    Copies,
    Pencil,
    Sync,
    Layers,
    Grid,
    Broom,
    Gear,
    Monitor,
    Folder,
    Play,
    Stop,
    Open,
    Trash,
    Export,
    Refresh,
    Plus,
    Close,
    ChevronDown,
    Check,
    Clock,
    Shield,
    Settings,
    Minus,
}

pub fn paint_icon(ui: &Ui, rect: Rect, icon: ToolIcon, color: Color32) {
    let p = ui.painter();
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let stroke = Stroke::new((s * 0.095).clamp(1.2, 1.8), color);
    let r = s * 0.32;
    match icon {
        ToolIcon::Search => {
            p.circle_stroke(c + Vec2::new(-s * 0.07, -s * 0.07), r, stroke);
            p.line_segment(
                [
                    c + Vec2::new(s * 0.11, s * 0.11),
                    c + Vec2::new(s * 0.33, s * 0.33),
                ],
                stroke,
            );
        }
        ToolIcon::Bolt => {
            let pts = vec![
                c + Vec2::new(s * 0.06, -s * 0.36),
                c + Vec2::new(-s * 0.20, s * 0.05),
                c + Vec2::new(-s * 0.01, s * 0.05),
                c + Vec2::new(-s * 0.08, s * 0.36),
                c + Vec2::new(s * 0.22, -s * 0.08),
                c + Vec2::new(s * 0.02, -s * 0.08),
            ];
            p.add(egui::Shape::closed_line(pts, stroke));
        }
        ToolIcon::Document => {
            let rect = Rect::from_center_size(c, Vec2::new(s * 0.56, s * 0.72));
            p.rect_stroke(rect, s * 0.08, stroke, StrokeKind::Inside);
            for i in 0..3 {
                let y = rect.top() + s * (0.22 + i as f32 * 0.16);
                p.line_segment(
                    [
                        egui::pos2(rect.left() + s * 0.12, y),
                        egui::pos2(rect.right() - s * 0.12, y),
                    ],
                    Stroke::new(stroke.width * 0.8, color.gamma_multiply(0.8)),
                );
            }
        }
        ToolIcon::Copies => {
            let back = Rect::from_center_size(c - Vec2::new(s * 0.09, s * 0.09), Vec2::splat(s * 0.46));
            let front = Rect::from_center_size(c + Vec2::new(s * 0.09, s * 0.09), Vec2::splat(s * 0.46));
            p.rect_stroke(back, s * 0.07, stroke, StrokeKind::Inside);
            p.rect_filled(front, s * 0.07, theme::PANEL);
            p.rect_stroke(front, s * 0.07, stroke, StrokeKind::Inside);
        }
        ToolIcon::Pencil => {
            p.line_segment(
                [
                    c + Vec2::new(-s * 0.24, s * 0.24),
                    c + Vec2::new(s * 0.22, -s * 0.22),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(s * 0.12, -s * 0.32),
                    c + Vec2::new(s * 0.32, -s * 0.12),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(-s * 0.24, s * 0.24),
                    c + Vec2::new(-s * 0.33, s * 0.33),
                ],
                stroke,
            );
        }
        ToolIcon::Sync => {
            let rad = s * 0.26;
            p.circle_stroke(c, rad, stroke);
            p.line_segment(
                [
                    c + Vec2::new(rad - s * 0.10, -s * 0.05),
                    c + Vec2::new(rad, 0.0),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(rad - s * 0.10, s * 0.05),
                    c + Vec2::new(rad, 0.0),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(-rad + s * 0.10, s * 0.05),
                    c + Vec2::new(-rad, 0.0),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(-rad + s * 0.10, -s * 0.05),
                    c + Vec2::new(-rad, 0.0),
                ],
                stroke,
            );
        }
        ToolIcon::Layers => {
            for i in 0..3 {
                let y = c.y - s * 0.18 + i as f32 * s * 0.16;
                let pts = vec![
                    egui::pos2(c.x - s * 0.28, y + s * 0.08),
                    egui::pos2(c.x, y - s * 0.06),
                    egui::pos2(c.x + s * 0.28, y + s * 0.08),
                ];
                p.add(egui::Shape::line(pts, stroke));
            }
        }
        ToolIcon::Grid => {
            let rect = Rect::from_center_size(c, Vec2::splat(s * 0.60));
            for i in 1..3 {
                let x = rect.left() + rect.width() * i as f32 / 3.0;
                p.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    stroke,
                );
                let y = rect.top() + rect.height() * i as f32 / 3.0;
                p.line_segment(
                    [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                    stroke,
                );
            }
            p.rect_stroke(rect, s * 0.06, stroke, StrokeKind::Inside);
        }
        ToolIcon::Broom => {
            p.line_segment(
                [
                    c + Vec2::new(s * 0.20, -s * 0.32),
                    c + Vec2::new(-s * 0.04, s * 0.04),
                ],
                stroke,
            );
            let pts = vec![
                c + Vec2::new(-s * 0.04, s * 0.04),
                c + Vec2::new(-s * 0.28, s * 0.12),
                c + Vec2::new(-s * 0.12, s * 0.32),
                c + Vec2::new(s * 0.06, s * 0.14),
            ];
            p.add(egui::Shape::closed_line(pts, stroke));
            p.line_segment(
                [
                    c + Vec2::new(s * 0.16, -s * 0.32),
                    c + Vec2::new(s * 0.30, -s * 0.32),
                ],
                Stroke::new(stroke.width * 0.8, color),
            );
        }
        ToolIcon::Gear | ToolIcon::Settings => {
            p.circle_stroke(c, s * 0.18, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let dir = Vec2::new(a.cos(), a.sin());
                p.line_segment(
                    [c + dir * s * 0.25, c + dir * s * 0.35],
                    Stroke::new(stroke.width * 1.4, color),
                );
            }
        }
        ToolIcon::Monitor => {
            let rect = Rect::from_center_size(c - Vec2::new(0.0, s * 0.05), Vec2::new(s * 0.62, s * 0.44));
            p.rect_stroke(rect, s * 0.05, stroke, StrokeKind::Inside);
            p.line_segment(
                [
                    c + Vec2::new(-s * 0.12, s * 0.30),
                    c + Vec2::new(s * 0.12, s * 0.30),
                ],
                stroke,
            );
            let pulse = vec![
                rect.left_top() + Vec2::new(s * 0.10, s * 0.22),
                rect.left_top() + Vec2::new(s * 0.20, s * 0.22),
                rect.left_top() + Vec2::new(s * 0.26, s * 0.12),
                rect.left_top() + Vec2::new(s * 0.32, s * 0.28),
                rect.left_top() + Vec2::new(s * 0.38, s * 0.22),
                rect.left_top() + Vec2::new(s * 0.50, s * 0.22),
            ];
            p.add(egui::Shape::line(pulse, Stroke::new(stroke.width * 0.8, color)));
        }
        ToolIcon::Folder => {
            let r = Rect::from_center_size(c + Vec2::new(0.0, s * 0.03), Vec2::new(s * 0.64, s * 0.48));
            p.rect_stroke(r, s * 0.05, stroke, StrokeKind::Inside);
            p.line_segment(
                [
                    r.left_top() + Vec2::new(0.0, -s * 0.10),
                    r.left_top() + Vec2::new(s * 0.22, -s * 0.10),
                ],
                stroke,
            );
        }
        ToolIcon::Play => {
            let pts = vec![
                c + Vec2::new(-s * 0.16, -s * 0.28),
                c + Vec2::new(s * 0.28, 0.0),
                c + Vec2::new(-s * 0.16, s * 0.28),
            ];
            p.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        ToolIcon::Stop => {
            p.rect_filled(
                Rect::from_center_size(c, Vec2::splat(s * 0.42)),
                s * 0.06,
                color,
            );
        }
        ToolIcon::Open => {
            let r = Rect::from_center_size(c + Vec2::new(-s * 0.03, s * 0.05), Vec2::splat(s * 0.40));
            p.rect_stroke(r, s * 0.04, stroke, StrokeKind::Inside);
            p.line_segment(
                [
                    c + Vec2::new(s * 0.02, -s * 0.10),
                    c + Vec2::new(s * 0.32, -s * 0.32),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(s * 0.16, -s * 0.32),
                    c + Vec2::new(s * 0.32, -s * 0.32),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(s * 0.32, -s * 0.32),
                    c + Vec2::new(s * 0.32, -s * 0.16),
                ],
                stroke,
            );
        }
        ToolIcon::Trash => {
            let r = Rect::from_center_size(c + Vec2::new(0.0, s * 0.06), Vec2::new(s * 0.44, s * 0.50));
            p.rect_stroke(r, s * 0.04, stroke, StrokeKind::Inside);
            p.line_segment(
                [
                    r.left_top() + Vec2::new(0.0, -s * 0.10),
                    r.right_top() + Vec2::new(0.0, -s * 0.10),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(-s * 0.08, -s * 0.24),
                    c + Vec2::new(s * 0.08, -s * 0.24),
                ],
                stroke,
            );
            for i in 0..2 {
                let x = c.x - s * 0.09 + i as f32 * s * 0.18;
                p.line_segment(
                    [
                        egui::pos2(x, r.top() + s * 0.10),
                        egui::pos2(x, r.bottom() - s * 0.08),
                    ],
                    stroke,
                );
            }
        }
        ToolIcon::Export => {
            p.line_segment(
                [
                    c + Vec2::new(-s * 0.28, s * 0.18),
                    c + Vec2::new(-s * 0.28, s * 0.30),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(-s * 0.28, s * 0.30),
                    c + Vec2::new(s * 0.28, s * 0.30),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(s * 0.28, s * 0.18),
                    c + Vec2::new(s * 0.28, s * 0.30),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(0.0, -s * 0.30),
                    c + Vec2::new(0.0, s * 0.10),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(-s * 0.12, 0.0),
                    c + Vec2::new(0.0, s * 0.12),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(s * 0.12, 0.0),
                    c + Vec2::new(0.0, s * 0.12),
                ],
                stroke,
            );
        }
        ToolIcon::Refresh => {
            let r = s * 0.24;
            let start = -std::f32::consts::FRAC_PI_2 + 0.5;
            let end = std::f32::consts::PI * 1.45;
            let pts: Vec<_> = (0..24)
                .map(|i| {
                    let a = start + (end - start) * i as f32 / 23.0;
                    c + Vec2::new(a.cos(), a.sin()) * r
                })
                .collect();
            p.add(egui::Shape::line(pts, stroke));
            let tip = c + Vec2::new(end.cos(), end.sin()) * r;
            p.line_segment([tip, tip + Vec2::new(-s * 0.10, -s * 0.02)], stroke);
            p.line_segment([tip, tip + Vec2::new(-s * 0.06, -s * 0.12)], stroke);
        }
        ToolIcon::Plus => {
            p.line_segment(
                [
                    c + Vec2::new(-s * 0.24, 0.0),
                    c + Vec2::new(s * 0.24, 0.0),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(0.0, -s * 0.24),
                    c + Vec2::new(0.0, s * 0.24),
                ],
                stroke,
            );
        }
        ToolIcon::Close => {
            p.line_segment(
                [
                    c + Vec2::new(-s * 0.22, -s * 0.22),
                    c + Vec2::new(s * 0.22, s * 0.22),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(s * 0.22, -s * 0.22),
                    c + Vec2::new(-s * 0.22, s * 0.22),
                ],
                stroke,
            );
        }
        ToolIcon::ChevronDown => {
            p.line_segment(
                [
                    c + Vec2::new(-s * 0.16, -s * 0.06),
                    c + Vec2::new(0.0, s * 0.10),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(s * 0.16, -s * 0.06),
                    c + Vec2::new(0.0, s * 0.10),
                ],
                stroke,
            );
        }
        ToolIcon::Check => {
            p.line_segment(
                [
                    c + Vec2::new(-s * 0.22, 0.0),
                    c + Vec2::new(-s * 0.05, s * 0.16),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(-s * 0.05, s * 0.16),
                    c + Vec2::new(s * 0.24, -s * 0.18),
                ],
                stroke,
            );
        }
        ToolIcon::Clock => {
            p.circle_stroke(c, s * 0.28, stroke);
            p.line_segment([c, c + Vec2::new(0.0, -s * 0.16)], stroke);
            p.line_segment([c, c + Vec2::new(s * 0.12, s * 0.05)], stroke);
        }
        ToolIcon::Shield => {
            let pts = vec![
                c + Vec2::new(0.0, -s * 0.32),
                c + Vec2::new(s * 0.26, -s * 0.20),
                c + Vec2::new(s * 0.22, s * 0.10),
                c + Vec2::new(0.0, s * 0.32),
                c + Vec2::new(-s * 0.22, s * 0.10),
                c + Vec2::new(-s * 0.26, -s * 0.20),
            ];
            p.add(egui::Shape::closed_line(pts, stroke));
            p.line_segment(
                [
                    c + Vec2::new(-s * 0.09, 0.0),
                    c + Vec2::new(-s * 0.01, s * 0.09),
                ],
                stroke,
            );
            p.line_segment(
                [
                    c + Vec2::new(-s * 0.01, s * 0.09),
                    c + Vec2::new(s * 0.12, -s * 0.10),
                ],
                stroke,
            );
        }
        ToolIcon::Minus => {
            p.line_segment(
                [
                    c + Vec2::new(-s * 0.24, 0.0),
                    c + Vec2::new(s * 0.24, 0.0),
                ],
                stroke,
            );
        }
    }
}

pub struct IconButton<'a> {
    icon: ToolIcon,
    tooltip: &'a str,
    active: bool,
}

impl<'a> IconButton<'a> {
    pub fn new(icon: ToolIcon, tooltip: &'a str) -> Self {
        Self { icon, tooltip, active: false }
    }
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
    pub fn show(self, ui: &mut Ui) -> Response {
        let size = Vec2::splat(32.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let hovered = response.hovered();
        let bg = if self.active {
            theme::ACCENT.gamma_multiply(0.18)
        } else if hovered {
            ui.visuals().widgets.hovered.bg_fill
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, CornerRadius::same(8), bg);
        let color = if self.active {
            theme::ACCENT
        } else if hovered {
            ui.visuals().text_color()
        } else {
            theme::dim(ui)
        };
        paint_icon(ui, rect.shrink(7.0), self.icon, color);
        response.on_hover_text(self.tooltip)
    }
}

pub fn toggle_switch(ui: &mut Ui, on: &mut bool) -> Response {
    let desired = Vec2::new(38.0, 21.0);
    let (rect, mut response) = ui.allocate_exact_size(desired, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(response.id, *on, 0.12);
    let bg = if *on {
        theme::ACCENT.gamma_multiply(0.85)
    } else {
        ui.visuals().widgets.inactive.bg_fill
    };
    ui.painter().rect_filled(rect, rect.height() / 2.0, bg);
    let cx = egui::lerp((rect.left() + rect.height() / 2.0)..=(rect.right() - rect.height() / 2.0), t);
    ui.painter().circle_filled(
        egui::pos2(cx, rect.center().y),
        rect.height() / 2.0 - 3.0,
        if *on { Color32::WHITE } else { ui.visuals().text_color() },
    );
    response
}

pub fn pill(ui: &mut Ui, text: impl Into<String>, color: Color32) {
    let text: String = text.into();
    let galley = ui.painter().layout_no_wrap(text, FontId::proportional(11.5), color);
    let size = galley.size() + Vec2::new(16.0, 6.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, size.y / 2.0, color.gamma_multiply(0.17));
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
}

pub fn section_label(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(text)
            .size(12.0)
            .strong()
            .color(theme::dim(ui)),
    );
}

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let mut frame = egui::Frame::new()
        .fill(ui.visuals().widgets.noninteractive.bg_fill)
        .stroke(Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(14));
    if !ui.is_enabled() {
        frame = frame.fill(ui.visuals().faint_bg_color);
    }
    frame.show(ui, add).inner
}

pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str, right: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(title).size(25.0).strong());
            ui.add_space(2.0);
            ui.label(egui::RichText::new(subtitle).size(12.5).color(theme::dim(ui)));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), right);
    });
    ui.add_space(10.0);
}

pub fn primary_button(ui: &mut Ui, icon: Option<ToolIcon>, text: &str) -> Response {
    let text_color = theme::BG;
    let label = if icon.is_some() { format!("      {text}") } else { text.to_string() };
    let mut button = egui::Button::new(
        egui::RichText::new(label)
            .color(text_color)
            .strong()
            .size(13.5),
    )
    .fill(theme::ACCENT)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(8));
    if icon.is_some() {
        button = button.min_size(Vec2::new(0.0, 32.0));
    }
    let response = ui.add(button);
    if let Some(icon) = icon {
        let rect = response.rect;
        let icon_rect = Rect::from_center_size(
            egui::pos2(rect.left() + 18.0, rect.center().y),
            Vec2::splat(16.0),
        );
        paint_icon(ui, icon_rect, icon, text_color);
    }
    response
}

pub fn ghost_button(ui: &mut Ui, icon: Option<ToolIcon>, text: &str) -> Response {
    let fill = ui.visuals().widgets.inactive.bg_fill;
    let stroke = Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color);
    let label = if icon.is_some() { format!("     {text}") } else { text.to_string() };
    let mut button = egui::Button::new(egui::RichText::new(label).size(13.0))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(8));
    if icon.is_some() {
        button = button.min_size(Vec2::new(0.0, 30.0));
    }
    let response = ui.add(button);
    if let Some(icon) = icon {
        let rect = response.rect;
        let icon_rect = Rect::from_center_size(
            egui::pos2(rect.left() + 16.0, rect.center().y),
            Vec2::splat(14.0),
        );
        paint_icon(ui, icon_rect, icon, ui.visuals().text_color());
    }
    response
}

pub fn danger_button(ui: &mut Ui, icon: Option<ToolIcon>, text: &str) -> Response {
    let label = if icon.is_some() { format!("     {text}") } else { text.to_string() };
    let mut button = egui::Button::new(
        egui::RichText::new(label)
            .color(theme::DANGER)
            .size(13.0)
            .strong(),
    )
    .fill(theme::DANGER.gamma_multiply(0.12))
    .stroke(Stroke::new(1.0, theme::DANGER.gamma_multiply(0.45)))
    .corner_radius(CornerRadius::same(8));
    if icon.is_some() {
        button = button.min_size(Vec2::new(0.0, 30.0));
    }
    let response = ui.add(button);
    if let Some(icon) = icon {
        let rect = response.rect;
        let icon_rect = Rect::from_center_size(
            egui::pos2(rect.left() + 17.0, rect.center().y),
            Vec2::splat(14.0),
        );
        paint_icon(ui, icon_rect, icon, theme::DANGER);
    }
    response
}

pub fn empty_state(ui: &mut Ui, icon: ToolIcon, title: &str, description: &str) {
    ui.add_space(30.0);
    ui.vertical_centered(|ui| {
        let size = Vec2::splat(52.0);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 26.0, theme::ACCENT.gamma_multiply(0.08));
        paint_icon(ui, rect.shrink(13.0), icon, theme::ACCENT.gamma_multiply(0.8));
        ui.add_space(10.0);
        ui.label(egui::RichText::new(title).size(16.0).strong());
        ui.add_space(3.0);
        ui.label(
            egui::RichText::new(description)
                .size(12.5)
                .color(theme::dim(ui)),
        );
        ui.add_space(12.0);
    });
}

pub fn stat_card(ui: &mut Ui, label: &str, value: &str, accent: Color32) {
    egui::Frame::new()
        .fill(accent.gamma_multiply(0.07))
        .stroke(Stroke::new(1.0, accent.gamma_multiply(0.22)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.set_min_width(112.0);
            ui.label(egui::RichText::new(label).size(11.0).color(theme::dim(ui)));
            ui.label(egui::RichText::new(value).size(16.0).strong().color(accent));
        });
}

pub fn text_field(ui: &mut Ui, value: &mut String, hint: &str, width: f32) -> Response {
    let response = ui.add_sized(
        [width, 30.0],
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .margin(Vec2::new(9.0, 6.0)),
    );
    response
}

pub fn path_row(
    ui: &mut Ui,
    label: &str,
    value: &mut String,
    hint: &str,
    pick: &mut dyn FnMut() -> Option<String>,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(12.5).color(theme::dim(ui)));
        let available = (ui.available_width() - 100.0).max(160.0);
        changed |= ui
            .add_sized(
                [available, 30.0],
                egui::TextEdit::singleline(value)
                    .hint_text(hint)
                    .margin(Vec2::new(9.0, 6.0)),
            )
            .changed();
        if ghost_button(ui, Some(ToolIcon::Folder), "浏览").clicked() {
            if let Some(path) = pick() {
                *value = path;
                changed = true;
            }
        }
    });
    changed
}

pub fn draw_dot(ui: &mut Ui, active: bool, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    if active {
        let t = ui.input(|i| i.time) as f32;
        let pulse = (t * 2.2).sin() * 0.5 + 0.5;
        ui.painter().circle_filled(
            rect.center(),
            4.0 + pulse * 2.0,
            color.gamma_multiply(0.18),
        );
        ui.painter().circle_filled(rect.center(), 3.0, color);
    } else {
        ui.painter().circle_filled(rect.center(), 3.0, ui.visuals().widgets.noninteractive.fg_stroke.color);
    }
}

pub fn modal_frame(ui: &mut Ui, title: &str, width: f32, add: impl FnOnce(&mut Ui)) {
    let mut open = true;
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .fixed_size([width, 0.0])
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(
            egui::Frame::new()
                .fill(ui.visuals().window_fill)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(18.0)
                .shadow(ui.visuals().window_shadow),
        )
        .show(ui.ctx(), |ui| {
            ui.set_width(width - 36.0);
            add(ui);
        });
}
