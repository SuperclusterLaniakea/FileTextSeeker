use std::sync::Arc;

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, TextStyle};

pub const BG: Color32 = Color32::from_rgb(13, 18, 24);
pub const SIDEBAR: Color32 = Color32::from_rgb(17, 24, 32);
pub const PANEL: Color32 = Color32::from_rgb(22, 31, 41);
pub const PANEL_HI: Color32 = Color32::from_rgb(28, 39, 51);
pub const BORDER: Color32 = Color32::from_rgb(42, 56, 70);
pub const TEXT: Color32 = Color32::from_rgb(226, 235, 241);
pub const TEXT_DIM: Color32 = Color32::from_rgb(138, 156, 170);
pub const ACCENT: Color32 = Color32::from_rgb(56, 202, 179);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(38, 133, 121);
pub const BLUE: Color32 = Color32::from_rgb(91, 156, 255);
pub const WARN: Color32 = Color32::from_rgb(245, 184, 78);
pub const DANGER: Color32 = Color32::from_rgb(238, 108, 102);
pub const SUCCESS: Color32 = Color32::from_rgb(102, 211, 143);

pub const LIGHT_BG: Color32 = Color32::from_rgb(246, 248, 250);
pub const LIGHT_SIDEBAR: Color32 = Color32::from_rgb(238, 242, 245);
pub const LIGHT_PANEL: Color32 = Color32::from_rgb(255, 255, 255);
pub const LIGHT_PANEL_HI: Color32 = Color32::from_rgb(245, 248, 250);
pub const LIGHT_BORDER: Color32 = Color32::from_rgb(219, 227, 234);
pub const LIGHT_TEXT: Color32 = Color32::from_rgb(30, 41, 51);
pub const LIGHT_TEXT_DIM: Color32 = Color32::from_rgb(104, 120, 133);

pub fn text(ui: &egui::Ui) -> Color32 {
    ui.visuals().widgets.inactive.fg_stroke.color
}

pub fn dim(ui: &egui::Ui) -> Color32 {
    ui.visuals().widgets.noninteractive.fg_stroke.color
}

pub fn setup(ctx: &egui::Context, dark: bool, font_scale: f32) {
    install_fonts(ctx, font_scale);
    apply(ctx, dark);
}

fn install_fonts(ctx: &egui::Context, font_scale: f32) {
    let mut fonts = FontDefinitions::default();
    let candidates = [
        r"C:\Windows\Fonts\Deng.ttf",
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let mut data = FontData::from_owned(bytes);
            data.index = 0;
            fonts.font_data.insert("cjk".to_owned(), Arc::new(data));
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .push("cjk".to_owned());
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .push("cjk".to_owned());
            break;
        }
    }
    ctx.set_fonts(fonts);

    let scale = font_scale.clamp(0.8, 1.6);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(11.5 * scale)),
            (TextStyle::Body, FontId::proportional(14.0 * scale)),
            (TextStyle::Button, FontId::proportional(14.0 * scale)),
            (TextStyle::Heading, FontId::proportional(22.0 * scale)),
            (TextStyle::Monospace, FontId::monospace(12.5 * scale)),
        ]
        .into();
    });
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    let theme = if dark { egui::Theme::Dark } else { egui::Theme::Light };
    let mut style = (*ctx.style_of(theme)).clone();
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    let bg = if dark { BG } else { LIGHT_BG };
    let side = if dark { SIDEBAR } else { LIGHT_SIDEBAR };
    let panel = if dark { PANEL } else { LIGHT_PANEL };
    let panel_hi = if dark { PANEL_HI } else { LIGHT_PANEL_HI };
    let border = if dark { BORDER } else { LIGHT_BORDER };
    let text = if dark { TEXT } else { LIGHT_TEXT };
    let dim = if dark { TEXT_DIM } else { LIGHT_TEXT_DIM };

    v.window_fill = panel;
    v.panel_fill = bg;
    v.extreme_bg_color = if dark { Color32::from_rgb(8, 12, 17) } else { Color32::from_rgb(236, 241, 245) };
    v.faint_bg_color = if dark { Color32::from_rgb(25, 34, 44) } else { Color32::from_rgb(243, 246, 249) };
    v.override_text_color = Some(text);
    v.window_stroke = egui::Stroke::new(1.0, border);
    v.window_corner_radius = 16.into();
    v.menu_corner_radius = 10.into();
    v.popup_shadow = if dark {
        egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(120) }
    } else {
        egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(40) }
    };
    v.window_shadow = v.popup_shadow;

    v.widgets.noninteractive.bg_fill = panel;
    v.widgets.noninteractive.weak_bg_fill = panel;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, border);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, dim);

    v.widgets.inactive.bg_fill = if dark { Color32::from_rgb(30, 42, 54) } else { Color32::from_rgb(238, 243, 247) };
    v.widgets.inactive.weak_bg_fill = if dark { Color32::from_rgb(27, 38, 49) } else { Color32::from_rgb(243, 247, 250) };
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, border);
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, text);

    v.widgets.hovered.bg_fill = if dark { Color32::from_rgb(38, 54, 68) } else { Color32::from_rgb(229, 238, 244) };
    v.widgets.hovered.weak_bg_fill = v.widgets.hovered.bg_fill;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT_DIM);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, text);

    v.widgets.active.bg_fill = if dark { Color32::from_rgb(45, 63, 78) } else { Color32::from_rgb(221, 233, 240) };
    v.widgets.active.weak_bg_fill = v.widgets.active.bg_fill;
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, text);

    v.selection.bg_fill = if dark {
        Color32::from_rgba_unmultiplied(56, 202, 179, 52)
    } else {
        Color32::from_rgba_unmultiplied(38, 133, 121, 38)
    };
    v.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    v.hyperlink_color = if dark { Color32::from_rgb(106, 172, 255) } else { Color32::from_rgb(35, 104, 200) };
    v.slider_trailing_fill = true;
    v.striped = true;
    v.collapsing_header_frame = true;
    v.button_frame = true;

    // Keep translucent accent fills readable in both themes.
    let accent = if dark { ACCENT } else { ACCENT_DIM };
    v.widgets.active.bg_fill = accent.gamma_multiply(0.22);
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0, accent);
    v.widgets.inactive.bg_fill = if dark { panel_hi } else { Color32::from_rgb(243, 247, 250) };

    style.visuals = v;
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.menu_margin = 8.0.into();
    style.spacing.window_margin = 12.0.into();
    style.spacing.indent = 18.0;
    style.spacing.interact_size.y = 30.0;
    style.spacing.combo_width = 130.0;
    style.spacing.scroll = egui::style::ScrollStyle {
        bar_width: 8.0,
        floating: false,
        ..Default::default()
    };
    ctx.set_style_of(theme, style);
    ctx.set_theme(theme);
    let _ = (side, bg);
}

pub fn paint_backdrop(ctx: &egui::Context, dark: bool, screen: egui::Rect) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let base = if dark { BG } else { LIGHT_BG };
    painter.rect_filled(screen, 0.0, base);
    if dark {
        let glow = Color32::from_rgba_unmultiplied(30, 110, 115, 22);
        painter.circle_filled(
            egui::pos2(screen.right() - 130.0, screen.top() + 40.0),
            380.0,
            glow,
        );
        let glow2 = Color32::from_rgba_unmultiplied(60, 90, 180, 16);
        painter.circle_filled(
            egui::pos2(screen.left() + 260.0, screen.bottom() + 120.0),
            420.0,
            glow2,
        );
    }
}
