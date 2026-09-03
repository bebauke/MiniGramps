//! Rahmenlose Titelleiste: Projekt-Aktionen links, draggable Flaeche und
//! native Fensteraktionen rechts.
//!
//! Verdrahtung:
//! - "Projekt öffnen" zählt Treffer (`crate::import::discover_projects`) und
//!   öffnet den Öffnen-Dialog (`dialogs::show_open`).
//! - "Projekt speichern" → `MiniGramps::save`.
//! - Zahnrad toggelt `MiniGramps::show_settings` → `dialogs::show_settings`.

use eframe::egui::{self, Color32};

use crate::import::discover_projects;
use crate::ui::{
    ICON_CLOSE, ICON_MAXIMIZE, ICON_MINIMIZE, ICON_OPEN, ICON_SAVE, ICON_SETTINGS, MiniGramps,
    icon_button_big, panels::palette, whitened_logo,
};

pub fn show(app: &mut MiniGramps, ctx: &egui::Context) {
    let colors = palette(app.dark_mode);
    egui::TopBottomPanel::top("header")
        .exact_height(58.0)
        .frame(egui::Frame::new().fill(colors.header).inner_margin(6))
        .show(ctx, |ui| {
            // Zuerst als Hintergrund registrieren; spaeter angelegte Buttons
            // erhalten Vorrang und starten keinen Fenster-Drag.
            let title_response = ui.interact(
                ui.max_rect(),
                ui.id().with("window-drag-area"),
                egui::Sense::click_and_drag(),
            );
            let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
            if title_response.double_clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            } else if title_response.drag_started_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            ui.columns(3, |columns| {
                columns[0].with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if icon_button_big(ui, ICON_OPEN, "open", "Projekt öffnen").clicked() {
                        let found = discover_projects(&app.library).len();
                        app.log(format!("Projektsuche: {found} Treffer"));
                        app.show_open = true;
                    }
                    let logo = egui::Image::from_bytes(
                        "bytes://minigramps-project-logo.svg",
                        whitened_logo(),
                    )
                    .fit_to_exact_size(egui::Vec2::splat(25.0))
                    .tint(ui.visuals().text_color());
                    if ui
                        .add(egui::Button::image(logo).min_size(egui::Vec2::new(40.0, 36.0)))
                        .on_hover_text("MiniGramps-Projekt")
                        .clicked()
                    {
                        app.show_project = !app.show_project;
                    }
                    if icon_button_big(ui, ICON_SAVE, "save", "Projekt speichern").clicked() {
                        app.save();
                    }
                    // Projektname hinter dem Speichern-Button (etwas größer
                    // als Kleinschrift, damit er als Titel erkennbar ist).
                    ui.label(
                        egui::RichText::new(&app.data.project.name)
                            .size(15.0)
                            .color(colors.accent_dim),
                    );
                });
                columns[1].vertical_centered(|ui| {
                    // Optische Vertikal-Zentrierung: die zweizeilige Marke
                    // sitzt sonst zu hoch (Zeilenabstand der zweiten Zeile).
                    ui.add_space(4.0);
                    ui.heading(
                        egui::RichText::new("mini gramps")
                            .color(colors.brand)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new("FAMILIENARCHIV")
                            .small()
                            .color(colors.accent_dim),
                    );
                });
                columns[2].with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if window_button(ui, ICON_CLOSE, "window-close", "Schliessen", true).clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if window_button(
                        ui,
                        ICON_MAXIMIZE,
                        "window-maximize",
                        if maximized {
                            "Wiederherstellen"
                        } else {
                            "Maximieren"
                        },
                        false,
                    )
                    .clicked()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    }
                    if window_button(ui, ICON_MINIMIZE, "window-minimize", "Minimieren", false)
                        .clicked()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                    if icon_button_big(ui, ICON_SETTINGS, "settings", "Einstellungen").clicked() {
                        app.show_settings = !app.show_settings;
                    }
                    // Status ABSCHNEIDEN statt überlaufen: langer Pfad kollidiert
                    // sonst mit dem zentrierten Titel.
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&app.status)
                                .small()
                                .color(Color32::LIGHT_GRAY),
                        )
                        .truncate(),
                    );
                });
            });
        });
}

fn window_button(
    ui: &mut egui::Ui,
    bytes: &'static [u8],
    id: &'static str,
    tooltip: &str,
    close: bool,
) -> egui::Response {
    let image =
        egui::Image::from_bytes(format!("bytes://{id}.svg"), crate::ui::whitened_svg(bytes))
            .fit_to_exact_size(egui::Vec2::splat(15.0))
            .tint(ui.visuals().text_color());
    let response = ui.add(egui::Button::image(image).min_size(egui::Vec2::new(38.0, 34.0)));
    if close && response.hovered() {
        ui.painter().rect_filled(
            response.rect,
            ui.visuals().widgets.hovered.corner_radius,
            Color32::from_rgb(190, 45, 45),
        );
        // Nach dem roten Hintergrund das Symbol erneut zeichnen.
        let icon = egui::Image::from_bytes(
            format!("bytes://{id}-hover.svg"),
            crate::ui::whitened_svg(bytes),
        )
        .fit_to_exact_size(egui::Vec2::splat(15.0))
        .tint(Color32::WHITE);
        icon.paint_at(ui, response.rect.shrink2(egui::Vec2::new(11.5, 9.5)));
    }
    response.on_hover_text(tooltip)
}
