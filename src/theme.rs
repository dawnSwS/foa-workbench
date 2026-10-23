use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke};

pub const BACKGROUND: Color32 = Color32::from_rgb(242, 245, 248);
pub const PANEL: Color32 = Color32::WHITE;
pub const CARD: Color32 = Color32::from_rgb(247, 249, 251);
pub const SIDEBAR: Color32 = Color32::from_rgb(235, 240, 245);
pub const ROW_ALT: Color32 = Color32::from_rgb(247, 249, 251);
pub const BORDER: Color32 = Color32::from_rgb(220, 228, 235);
pub const ACCENT: Color32 = Color32::from_rgb(5, 115, 117);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(226, 243, 240);
pub const ON_ACCENT: Color32 = Color32::WHITE;
pub const TEXT: Color32 = Color32::from_rgb(27, 44, 62);
pub const MUTED: Color32 = Color32::from_rgb(96, 113, 130);
pub const AMBER: Color32 = Color32::from_rgb(151, 91, 18);
pub const DANGER: Color32 = Color32::from_rgb(179, 54, 63);
pub const NAV_BG: Color32 = Color32::from_rgb(19, 36, 53);
pub const NAV_TEXT: Color32 = Color32::from_rgb(239, 246, 252);
pub const NAV_MUTED: Color32 = Color32::from_rgb(161, 181, 199);
pub const NAV_SELECTED: Color32 = Color32::from_rgb(37, 66, 82);

pub fn configure(context: &egui::Context) {
    context.set_theme(egui::Theme::Light);
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".into(),
        egui::FontData::from_static(include_bytes!("assets/NotoSansCJKsc-Regular.otf")).into(),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "cjk".into());
    // Keep Latin field names monospaced; use the embedded font for Chinese fallback.
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .push("cjk".into());
    context.set_fonts(fonts);

    let mut style = egui::Style {
        visuals: egui::Visuals::light(),
        ..Default::default()
    };
    configure_visuals(&mut style.visuals);
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size = egui::vec2(36.0, 34.0);
    style.spacing.icon_width = 18.0;
    style.spacing.icon_width_inner = 10.0;
    style.spacing.icon_spacing = 8.0;
    style.spacing.text_edit_width = 280.0;
    style.spacing.window_margin = egui::Margin::same(20);
    style.spacing.menu_margin = egui::Margin::same(8);
    for (text_style, font) in [
        (egui::TextStyle::Heading, FontId::proportional(24.0)),
        (egui::TextStyle::Body, FontId::proportional(15.0)),
        (egui::TextStyle::Button, FontId::proportional(15.0)),
        (egui::TextStyle::Small, FontId::proportional(12.0)),
        (egui::TextStyle::Monospace, FontId::monospace(13.0)),
    ] {
        style.text_styles.insert(text_style, font);
    }
    context.set_global_style(style);
}

fn configure_visuals(visuals: &mut egui::Visuals) {
    visuals.override_text_color = Some(TEXT);
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = PANEL;
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.window_corner_radius = egui::CornerRadius::same(8);
    visuals.menu_corner_radius = egui::CornerRadius::same(8);
    visuals.extreme_bg_color = PANEL;
    visuals.faint_bg_color = ROW_ALT;
    visuals.code_bg_color = CARD;
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = AMBER;
    visuals.error_fg_color = DANGER;
    visuals.selection.bg_fill = ACCENT_SOFT;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.text_cursor.stroke = Stroke::new(2.0, ACCENT);
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 20,
        spread: 0,
        color: Color32::from_black_alpha(24),
    };
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(18),
    };
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(8);
        widget.bg_fill = PANEL;
        widget.weak_bg_fill = PANEL;
        widget.bg_stroke = Stroke::new(1.0, BORDER);
        widget.fg_stroke = Stroke::new(1.5, TEXT);
        widget.expansion = 0.0;
    }
    // egui derives disabled colors by fading towards this quiet surface.
    visuals.widgets.noninteractive.weak_bg_fill = SIDEBAR;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(234, 244, 245);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(234, 244, 245);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.hovered.expansion = 1.0;
    // The active style is also used for keyboard focus, including text inputs.
    visuals.widgets.active.bg_fill = ACCENT_SOFT;
    visuals.widgets.active.weak_bg_fill = ACCENT_SOFT;
    visuals.widgets.active.bg_stroke = Stroke::new(2.0, ACCENT);
    visuals.widgets.active.expansion = 1.0;
    visuals.widgets.open.bg_fill = ACCENT_SOFT;
    visuals.widgets.open.weak_bg_fill = ACCENT_SOFT;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, ACCENT);
}

pub fn panel() -> egui::Frame {
    egui::Frame::NONE
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(10)
        .inner_margin(16)
}

pub fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text).color(MUTED)
}

pub fn primary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).color(ON_ACCENT).size(15.0))
        .fill(ACCENT)
        .corner_radius(8)
        .min_size(egui::vec2(0.0, 30.0))
}

pub fn compact_controls(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
    ui.spacing_mut().button_padding = egui::vec2(10.0, 4.0);
    ui.spacing_mut().interact_size = egui::vec2(28.0, 28.0);
}

pub fn badge(ui: &mut egui::Ui, text: &str) {
    status_badge(ui, text, ACCENT);
}

pub fn status_badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::NONE
        .fill(color.gamma_multiply(0.09))
        .corner_radius(5)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color).size(12.0));
        });
}

pub fn section_heading(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).size(25.0).strong());
    if !subtitle.is_empty() {
        ui.label(muted(subtitle).size(13.0));
    }
    ui.add_space(8.0);
}

pub fn stat(ui: &mut egui::Ui, label: &str, value: &str, caption: &str) {
    panel().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(muted(label).size(12.0));
        ui.label(RichText::new(value).size(28.0).strong());
        if !caption.is_empty() {
            ui.label(muted(caption).size(12.0));
        }
    });
}

pub fn nav_item(ui: &mut egui::Ui, selected: bool, title: &str, caption: &str) -> egui::Response {
    let height = if caption.is_empty() { 40.0 } else { 58.0 };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, title)
    });
    let painter = ui.painter();
    let active = selected || response.hovered() || response.has_focus();
    if active {
        painter.rect_filled(rect, 7, NAV_SELECTED);
    }
    if selected {
        painter.rect_filled(
            egui::Rect::from_min_size(
                rect.min + egui::vec2(0.0, 10.0),
                egui::vec2(3.0, height - 20.0),
            ),
            2,
            Color32::from_rgb(92, 206, 189),
        );
    }
    let title_y = if caption.is_empty() { 11.0 } else { 9.0 };
    painter.text(
        rect.min + egui::vec2(14.0, title_y),
        egui::Align2::LEFT_TOP,
        title,
        FontId::proportional(15.0),
        if active { NAV_TEXT } else { NAV_MUTED },
    );
    if !caption.is_empty() {
        painter.text(
            rect.min + egui::vec2(14.0, 33.0),
            egui::Align2::LEFT_TOP,
            caption,
            FontId::proportional(11.0),
            NAV_MUTED,
        );
    }
    response
}

pub fn emblem(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 8, ACCENT);
    let center = rect.center();
    // A simple archive mark stays legible at desktop toolbar size.
    painter.rect_stroke(
        egui::Rect::from_center_size(center, egui::vec2(20.0, 22.0)),
        3,
        Stroke::new(1.5, ON_ACCENT),
        egui::StrokeKind::Middle,
    );
    painter.line_segment(
        [
            center + egui::vec2(-9.0, -4.0),
            center + egui::vec2(9.0, -4.0),
        ],
        Stroke::new(1.5, ON_ACCENT),
    );
    painter.line_segment(
        [
            center + egui::vec2(-3.0, 2.0),
            center + egui::vec2(3.0, 2.0),
        ],
        Stroke::new(1.5, ON_ACCENT),
    );
}
