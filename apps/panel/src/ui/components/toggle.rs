use eframe::egui;

pub fn toggle(on: &mut bool) -> impl egui::Widget + '_ {
    move |ui: &mut egui::Ui| {
        let desired_size = ui.spacing().interact_size.y * egui::vec2(2.0, 1.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
        if response.clicked() {
            *on = !*on;
        }
        response
            .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *on, ""));

        if ui.is_rect_visible(rect) {
            let how_on = ui.ctx().animate_bool_with_time(response.id, *on, 0.1);
            let visuals = ui.style().interact_selectable(&response, *on);
            let rect_expanded = rect.expand(visuals.expansion);
            let radius = 0.5 * rect_expanded.height();
            ui.painter().rect(
                rect_expanded,
                radius,
                visuals.bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
            let circle_x = egui::lerp(
                (rect_expanded.min.x + radius)..=(rect_expanded.max.x - radius),
                how_on,
            );
            let center = egui::pos2(circle_x, rect_expanded.center().y);
            ui.painter()
                .circle(center, 0.75 * radius, visuals.bg_fill, visuals.bg_stroke);
        }

        response
    }
}
