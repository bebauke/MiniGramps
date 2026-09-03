//! Dialogfenster: Einstellungen, Projekt öffnen, Person-Editor, Foto-Intent.
//!
//! Verdrahtung:
//! - `show_settings`  → Design/Generationenlimit (`max_generations`, genutzt
//!   von `tree::draw_tree`) und Speicherort (`MiniGramps::change_library`).
//! - `show_open`      → Projektliste (`crate::import::discover_projects`),
//!   Suchordner (`MiniGramps::project_locations`), manuelles Laden.
//! - `show_editor`    → Modal zum Anlegen/Bearbeiten/Löschen; schreibt in
//!   `MiniGramps::draft`/`data.people`, Fotos via `media::import_media_file`.
//! - `show_image_intent` → fragt nach Drag-and-drop, ob das Foto Profilbild
//!   oder Galeriebild wird (kopiert nach `media/`).
//!
//! Alle Fenster sind fix (`movable(false)`) und mit kleinem Titel.

use eframe::egui::{self, Color32};
use rfd::FileDialog;

use crate::import::{discover_projects, project_display_name};
use crate::media::import_media_file;
use crate::model::Gender;
use crate::ui::{ICON_EXPORT, MiniGramps, icon_button, panels::palette, window_title};

pub fn show_project(app: &mut MiniGramps, ctx: &egui::Context) {
    if !app.show_project {
        return;
    }
    let mut open = true;
    let section = palette(app.dark_mode).section;
    egui::Window::new(window_title("MiniGramps-Projekt"))
        .open(&mut open)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::LEFT_TOP, [52.0, 60.0])
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("PROJEKT")
                    .small()
                    .strong()
                    .color(section),
            );
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.add(
                    egui::TextEdit::singleline(&mut app.data.project.name)
                        .desired_width(320.0),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Datenformat");
                ui.monospace(format!("Version {}", app.data.project.format_version));
            });
            ui.label(
                app.current_data_path
                    .as_deref()
                    .unwrap_or(&app.library)
                    .display()
                    .to_string(),
            );
            ui.separator();
            ui.label(
                egui::RichText::new("VERSIONSVERLAUF")
                    .small()
                    .strong()
                    .color(section),
            );
            ui.label("Noch keine Projektstaende. Versionierte Snapshots werden mit dem .mfg-Paketformat aktiviert.");
            ui.separator();
            if icon_button(ui, ICON_EXPORT, "project-export", "Exportieren...").clicked() {
                app.show_export = true;
            }
        });
    app.show_project = app.show_project && open;
}

pub fn show_export(app: &mut MiniGramps, ctx: &egui::Context) {
    if !app.show_export {
        return;
    }
    let mut open = true;
    egui::Window::new(window_title("Projekt exportieren"))
        .open(&mut open)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(430.0)
        .show(ctx, |ui| {
            ui.label("Exportformat auswaehlen");
            ui.separator();
            ui.add_enabled(false, egui::Button::new("Gramps XML (.gramps)"));
            ui.add_enabled(false, egui::Button::new("Gramps-Paket mit Medien (.gpkg)"));
            ui.add_enabled(false, egui::Button::new("GEDCOM (.ged)"));
            ui.add_enabled(false, egui::Button::new("MiniGramps Full (.mfg)"));
            ui.add_enabled(false, egui::Button::new("MiniGramps Mini (.mmg)"));
            ui.separator();
            ui.small("Die Exportmodule werden im naechsten Schritt implementiert.");
        });
    app.show_export = app.show_export && open;
}

pub fn show_settings(app: &mut MiniGramps, ctx: &egui::Context) {
    if !app.show_settings {
        return;
    }
    let mut open = true;
    let section = palette(app.dark_mode).section;
    egui::Window::new(window_title("Einstellungen"))
        .open(&mut open)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::RIGHT_TOP, [-12.0, 54.0])
        .default_width(340.0)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("DARSTELLUNG")
                    .small()
                    .strong()
                    .color(section),
            );
            ui.horizontal(|ui| {
                ui.label("Design");
                ui.selectable_value(&mut app.dark_mode, true, "Dunkel");
                ui.selectable_value(&mut app.dark_mode, false, "Hell");
            });
            ui.horizontal(|ui| {
                ui.label("Generationen");
                for limit in [3, 5, 7] {
                    ui.selectable_value(&mut app.max_generations, limit, format!("{limit}"));
                }
                ui.selectable_value(&mut app.max_generations, 0, "Alle");
            });
            ui.separator();
            ui.label(
                egui::RichText::new("SPEICHERORT")
                    .small()
                    .strong()
                    .color(section),
            );
            // Der Speicherort ist fix (App-Datenordner) und wird nur
            // angezeigt — eine Änderung ist bewusst nicht vorgesehen.
            ui.label(
                egui::RichText::new(app.library.display().to_string())
                    .small()
                    .monospace(),
            );
        });
    app.show_settings = open;
}

pub fn show_open(app: &mut MiniGramps, ctx: &egui::Context) {
    if !app.show_open {
        return;
    }
    let mut open = true;
    egui::Window::new(window_title("Projekt öffnen"))
        .open(&mut open)
        .movable(false)
        .default_width(560.0)
        .show(ctx, |ui| {
            ui.label("Gefundene Projekte in MiniGramps- und Gramps-Ordnern");
            ui.separator();
            for path in discover_projects(&app.library) {
                let selected = app
                    .selected_project
                    .as_deref()
                    .is_some_and(|current| current == path.as_path());
                if ui
                    .selectable_label(selected, project_display_name(&path))
                    .clicked()
                {
                    app.selected_project = Some(path.clone());
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.add_enabled_ui(app.selected_project.is_some(), |ui| {
                    if ui.button("Laden").clicked()
                        && let Some(path) = app.selected_project.clone()
                    {
                        app.load_path(&path);
                    }
                });
                if ui.button("Import...").clicked() {
                    app.import_dialog();
                }
            });
            ui.separator();
            ui.label(
                egui::RichText::new("SERVER")
                    .small()
                    .strong()
                    .color(palette(app.dark_mode).section),
            );
            ui.add(
                egui::TextEdit::singleline(&mut app.server_url)
                    .hint_text("https://host/api/v1 (IP/Domain)")
                    .desired_width(360.0),
            );
            ui.add(
                egui::TextEdit::singleline(&mut app.server_token)
                    .hint_text("Login/Token")
                    .password(true),
            );
            if ui.button("Vom Server laden").clicked() && !app.server_url.trim().is_empty() {
                let url = app.server_url.trim().to_string();
                let token = app.server_token.clone();
                app.load_from_server(&url, &token);
            }
        });
    app.show_open = app.show_open && open;
}

/// Bestätigung vor dem Schließen bei laufender Bearbeitung: Speichern,
/// verwerfen oder abbrechen.
pub fn show_close_confirm(app: &mut MiniGramps, ctx: &egui::Context) {
    if !app.pending_close {
        return;
    }
    let mut open = true;
    egui::Window::new(window_title("Ungespeicherte Änderungen"))
        .open(&mut open)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(360.0)
        .show(ctx, |ui| {
            ui.label("Es gibt ungespeicherte Änderungen. Vor dem Schließen speichern?");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Speichern und schließen").clicked() {
                    app.save();
                    app.pending_close = false;
                    app.inline_edit = false;
                    app.show_editor = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui.button("Verwerfen").clicked() {
                    app.pending_close = false;
                    app.inline_edit = false;
                    app.show_editor = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui.button("Abbrechen").clicked() {
                    app.pending_close = false;
                }
            });
        });
    if !open {
        app.pending_close = false;
    }
}

pub fn show_editor(app: &mut MiniGramps, ctx: &egui::Context) {
    if !app.show_editor {
        return;
    }
    let mut open = true;
    let title = if app.editing.is_some() {
        "Profil bearbeiten"
    } else {
        "Neue Person"
    };
    egui::Window::new(window_title(title))
        .open(&mut open)
        .movable(false)
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.label("Vorname");
            ui.add(
                egui::TextEdit::singleline(&mut app.draft.given_name)
                    .hint_text("Vorname")
                    .desired_width(200.0),
            );
            ui.label("Nachname");
            ui.add(
                egui::TextEdit::singleline(&mut app.draft.family_name)
                    .hint_text("Nachname")
                    .desired_width(200.0),
            );
            ui.horizontal(|ui| {
                ui.label("Geboren");
                ui.add(
                    egui::TextEdit::singleline(&mut app.draft.birth)
                        .hint_text("Jahr")
                        .desired_width(80.0),
                );
                ui.label("Ort");
                ui.add(egui::TextEdit::singleline(&mut app.draft.birth_place).hint_text("Ort"));
                ui.label("Gestorben");
                ui.add(
                    egui::TextEdit::singleline(&mut app.draft.death)
                        .hint_text("Jahr")
                        .desired_width(80.0),
                );
                ui.label("Ort");
                ui.add(egui::TextEdit::singleline(&mut app.draft.death_place).hint_text("Ort"));
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut app.draft.gender, Gender::Female, "Weiblich");
                ui.radio_value(&mut app.draft.gender, Gender::Male, "Männlich");
                ui.radio_value(&mut app.draft.gender, Gender::Unknown, "Unbekannt");
            });
            ui.horizontal(|ui| {
                if ui.button("Foto auswählen").clicked() {
                    if let Some(path) = FileDialog::new()
                        .add_filter("Bilder", &["png", "jpg", "jpeg", "webp"])
                        .pick_file()
                    {
                        if let Some(relative) = import_media_file(&app.library, &path) {
                            app.draft.photo = Some(relative);
                        }
                    }
                }
                if app.draft.photo.is_some() && ui.button("Foto entfernen").clicked() {
                    app.draft.photo = None;
                }
            });
            ui.label("Notizen");
            ui.add(
                egui::TextEdit::multiline(&mut app.draft.notes)
                    .hint_text("Notizen")
                    .desired_rows(3),
            );
            ui.label("Quelle");
            ui.add(egui::TextEdit::singleline(&mut app.draft.source).hint_text("Quelle"));
            ui.separator();
            if ui.button("Speichern").clicked() {
                let id = app.draft.id.clone();
                if let Some(existing) = &app.editing {
                    if let Some(person) = app
                        .data
                        .people
                        .iter_mut()
                        .find(|person| person.id == *existing)
                    {
                        *person = app.draft.clone();
                    }
                } else {
                    app.data.people.push(app.draft.clone());
                }
                app.photo_cache.remove(&id);
                app.selected = Some(id);
                app.status = "Profil gespeichert".into();
                app.show_editor = false;
            }
            if let Some(id) = app.editing.clone() {
                if ui
                    .button(egui::RichText::new("Person löschen").color(Color32::LIGHT_RED))
                    .clicked()
                {
                    app.data.people.retain(|person| person.id != id);
                    app.data.families.iter_mut().for_each(|family| {
                        if family.parent_a.as_deref() == Some(&id) {
                            family.parent_a = None;
                        }
                        if family.parent_b.as_deref() == Some(&id) {
                            family.parent_b = None;
                        }
                        family.children.retain(|child| child != &id);
                    });
                    app.selected = app.data.people.first().map(|person| person.id.clone());
                    app.show_editor = false;
                }
            }
        });
    app.show_editor = open;
}

pub fn show_image_intent(app: &mut MiniGramps, ctx: &egui::Context) {
    let Some(path) = app.pending_image.clone() else {
        return;
    };
    egui::Window::new(window_title("Foto hinzufügen"))
        .movable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(path.display().to_string());
            ui.label("Wofür soll dieses Foto verwendet werden?");
            if ui.button("Als Profilbild verwenden").clicked() {
                if let Some(id) = &app.selected {
                    if let Some(relative) = import_media_file(&app.library, &path) {
                        if let Some(person) =
                            app.data.people.iter_mut().find(|person| person.id == *id)
                        {
                            person.photo = Some(relative);
                        }
                        app.photo_cache.remove(id);
                    }
                }
                app.pending_image = None;
            }
            if ui.button("Zur Galerie hinzufügen").clicked() {
                if let Some(id) = &app.selected {
                    if let Some(relative) = import_media_file(&app.library, &path) {
                        if let Some(person) =
                            app.data.people.iter_mut().find(|person| person.id == *id)
                        {
                            person.gallery.push(relative);
                        }
                    }
                }
                app.pending_image = None;
            }
            if ui.button("Abbrechen").clicked() {
                app.pending_image = None;
            }
        });
}
