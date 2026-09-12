//! Dialogfenster: Einstellungen, Projekt öffnen, Person-Editor, Foto-Intent.
//!
//! Verdrahtung:
//! - `show_settings`  → Design/Generationenlimit (`max_generations`, genutzt
//!   von `tree::draw_tree`) und Speicherort (`MiniGramps::change_library`).
//! - `show_open`      → Projektliste (`crate::import::discover_projects`),
//!   Suchordner (`MiniGramps::project_locations`), manuelles Laden.
//! - `show_image_intent` → fragt nach Drag-and-drop, ob das Foto Profilbild
//!   oder Galeriebild wird (kopiert nach `media/`).
//!
//! Personen werden **ausschließlich in der rechten Seitenleiste** bearbeitet
//! (kein Modal). Alle Fenster hier sind fix (`movable(false)`) und mit
//! kleinem Titel.

use eframe::egui;

use crate::media::clear_person_photo_cache;
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use crate::media::import_media_file_async;
use crate::model::{Gender, person};
use crate::ui::{
    CardLayout, ICON_CHEVRON_LEFT, ICON_CHEVRON_RIGHT, ICON_EXPORT, ICON_EXTERNAL_LINK,
    ICON_TRASH, MiniGramps, icon_button, icon_only_button, panels::palette, window_title,
};

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
                let name_before = app.data.project.name.clone();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut app.data.project.name)
                        .desired_width(320.0),
                );
                if response.gained_focus() {
                    app.project_name_before_edit = Some(name_before);
                }
                if response.lost_focus() {
                    if let Some(previous_name) = app.project_name_before_edit.take() {
                        if previous_name != app.data.project.name {
                            let new_name = app.data.project.name.clone();
                            app.data.project.name = previous_name;
                            app.snapshot(format!("Projektname ändern: {new_name}"));
                            app.data.project.name = new_name;
                        }
                    }
                }
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
            ui.horizontal(|ui| {
                if ui.button("Datei anhängen & vergleichen…").on_hover_text(
                    "Datei ans aktuelle Projekt anhängen (IDs werden frisch vergeben) \
                     und Duplikate anhand von Vornamen, Geburtsdatum und \
                     Verwandtschaft erkennen.",
                ).clicked() {
                    app.import_append_dialog();
                }
                let open_hits = app
                    .merge_review
                    .as_ref()
                    .map(|review| review.candidates.len())
                    .unwrap_or(0);
                if open_hits > 0 && ui.button(format!("Treffer prüfen ({open_hits})")).clicked() {
                    app.show_merge_review = true;
                }
            });
        });
    app.show_project = app.show_project && open;
}

/// Duplikat-Review nach angehängtem Import: pro Treffer selektiv
/// zusammenführen (Checkbox) oder endgültig als Kein-Match ablehnen.
pub fn show_merge_review(app: &mut MiniGramps, ctx: &egui::Context) {
    if !app.show_merge_review || app.merge_review.is_none() {
        return;
    }
    let mut open = true;
    egui::Window::new(window_title("Duplikate prüfen"))
        .open(&mut open)
        .movable(false)
        .resizable(true)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(560.0)
        .show(ctx, |ui| {
            let count = app
                .merge_review
                .as_ref()
                .map(|review| review.candidates.len())
                .unwrap_or(0);
            ui.label(format!(
                "{count} mögliche Duplikate — pro Treffer zusammenführen oder ablehnen."
            ));
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for index in 0..count {
                        let (drop_id, label) = {
                            let Some((candidate, _)) = app
                                .merge_review
                                .as_ref()
                                .and_then(|review| review.candidates.get(index))
                            else {
                                continue;
                            };
                            let keep = app.data.find(&candidate.keep_id);
                            let drop = app.data.find(&candidate.drop_id);
                            let label = format!(
                                "{} ↔ {} — Vorname {:.0} %, Verwandt {:.0} %{}{}",
                                keep.map(|person| person.display_name())
                                    .unwrap_or_else(|| candidate.keep_id.clone()),
                                drop.map(|person| person.display_name())
                                    .unwrap_or_else(|| candidate.drop_id.clone()),
                                candidate.name_score * 100.0,
                                candidate.kin_score * 100.0,
                                if candidate.birth_match {
                                    ", Geburt gleich"
                                } else {
                                    ""
                                },
                                if keep.map(|person| person.birth.clone()).unwrap_or_default()
                                    != drop.map(|person| person.birth.clone()).unwrap_or_default()
                                {
                                    " (Datum nur einseitig)"
                                } else {
                                    ""
                                },
                            );
                            (candidate.drop_id.clone(), label)
                        };
                        ui.horizontal(|ui| {
                            let mut checked = app
                                .merge_review
                                .as_ref()
                                .map(|review| review.candidates[index].1)
                                .unwrap_or(false);
                            ui.checkbox(&mut checked, label);
                            if let Some(review) = app.merge_review.as_mut() {
                                review.candidates[index].1 = checked;
                            }
                            if ui.small_button("Kein Match").clicked() {
                                app.reject_merge_candidate(&drop_id);
                            }
                        });
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                let selected = app
                    .merge_review
                    .as_ref()
                    .map(|review| review.candidates.iter().filter(|(_, c)| *c).count())
                    .unwrap_or(0);
                if ui
                    .button(format!("Ausgewählte zusammenführen ({selected})"))
                    .clicked()
                {
                    app.apply_merge_review();
                }
                if ui.button("Alle behalten & schließen").clicked() {
                    app.merge_review = None;
                    app.show_merge_review = false;
                }
            });
        });
    if !open {
        app.show_merge_review = false;
    }
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
            // Einstellungsänderungen behalten den aktuellen Ausschnitt (pan/zoom);
            // neu zentriert wird nur beim Wechsel der Referenzperson.
            ui.horizontal(|ui| {
                ui.label("Kartenlayout");
                ui.selectable_value(&mut app.card_layout, CardLayout::Compact, "Kompakt");
                ui.selectable_value(&mut app.card_layout, CardLayout::Portrait, "Großes Foto");
            });
            ui.horizontal(|ui| {
                ui.label("Breite Kompakt");
                ui.add(
                    egui::DragValue::new(&mut app.compact_card_width)
                        .range(120.0..=400.0)
                        .suffix(" px"),
                )
                .on_hover_text("Fixe Kartenbreite im Kompakt-Layout (zu lange Namen enden mit …).");
            });
            ui.horizontal(|ui| {
                ui.label("Breite Groß");
                ui.add(
                    egui::DragValue::new(&mut app.portrait_card_width)
                        .range(120.0..=400.0)
                        .suffix(" px"),
                )
                .on_hover_text("Fixe Kartenbreite im Layout „Großes Foto“ (zu lange Namen enden mit …).");
            });
            ui.horizontal(|ui| {
                ui.label("Symbol Geburt");
                ui.add(
                    egui::TextEdit::singleline(&mut app.birth_symbol).desired_width(36.0),
                )
                .on_hover_text("Zeichen vor dem Geburtsdatum auf den Baumkarten (z. B. Elhaz-Rune ᛉ).");
                ui.label("Symbol Tod");
                ui.add(
                    egui::TextEdit::singleline(&mut app.death_symbol).desired_width(36.0),
                )
                .on_hover_text("Zeichen vor dem Sterbedatum auf den Baumkarten (z. B. Elhaz-Rune ᛦ).");
            });
            ui.horizontal(|ui| {
                ui.label("Generationen");
                for limit in [3, 5, 7] {
                    ui.selectable_value(&mut app.max_generations, limit, format!("{limit}"));
                }
                ui.selectable_value(&mut app.max_generations, 0, "Automatisch");
            });
            if app.max_generations == 0 {
                ui.horizontal(|ui| {
                    ui.label("Startlimit");
                    ui.add(
                        egui::DragValue::new(&mut app.tree_initial_person_limit)
                            .range(1..=10_000)
                            .suffix(" Personen"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Ladeschritt");
                    ui.add(
                        egui::DragValue::new(&mut app.tree_load_step)
                            .range(1..=10_000)
                            .suffix(" Personen"),
                    );
                });
            }
            ui.horizontal(|ui| {
                ui.label("Baum-Abstand");
                ui.add(
                    egui::Slider::new(&mut app.layout_gap, 5.0..=150.0)
                        .step_by(5.0)
                        .show_value(true),
                )
                .on_hover_text(
                    "Default-Abstand zwischen den Karten im automatischen Layout \
                     (bei Platzmangel rücken Karten näher zusammen).",
                );
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
            if app.project_list_cache.is_empty() {
                app.refresh_project_list();
            }
            let list_paths = app.project_list_cache.clone();
            let list_names = app.project_list_names.clone();
            for (path, name) in list_paths.iter().zip(list_names.iter()) {
                let selected = app
                    .selected_project
                    .as_deref()
                    .is_some_and(|current| current == path.as_path());
                if ui.selectable_label(selected, name).clicked() {
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
                if ui.button("Neues Projekt").clicked() {
                    app.new_project();
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
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui.button("Verwerfen").clicked() {
                    app.pending_close = false;
                    app.inline_edit = false;
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

pub fn show_pending_select_confirm(app: &mut MiniGramps, ctx: &egui::Context) {
    if app.pending_select.is_none() {
        return;
    }
    let mut open = true;
    egui::Window::new(window_title("Ungespeicherte Änderungen"))
        .open(&mut open)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(400.0)
        .show(ctx, |ui| {
            let name = app
                .data
                .find(&app.draft.id)
                .map(|person| person.display_name())
                .unwrap_or_default();
            ui.label(format!(
                "Es gibt ungespeicherte Änderungen an „{name}“. Vor dem Wechsel speichern?"
            ));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Speichern und wechseln").clicked() {
                    app.commit_draft();
                    app.inline_edit = false;
                    app.status = "Profil gespeichert".into();
                    app.save();
                    let ctx = ui.ctx().clone();
                    app.apply_pending_select(&ctx);
                }
                if ui.button("Verwerfen und wechseln").clicked() {
                    app.inline_edit = false;
                    app.relation_picker = None;
                    app.status = "Änderungen verworfen".into();
                    let ctx = ui.ctx().clone();
                    app.apply_pending_select(&ctx);
                }
                if ui.button("Abbrechen").clicked() {
                    app.pending_select = None;
                }
            });
        });
    if !open {
        app.pending_select = None;
    }
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
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
            let selected_id = app.selected.clone();
            if ui.button("Als Profilbild verwenden").clicked() {
                if let Some(id) = &selected_id {
                    if let Some(relative) = import_media_file_async(ui.ctx(), &app.library, &path) {
                        let name = app
                            .data
                            .find(id)
                            .map(|person| person.display_name())
                            .unwrap_or_else(|| id.clone());
                        app.snapshot(format!("Profilbild ändern: {name}"));
                        if let Some(person) =
                            app.data.people.iter_mut().find(|person| person.id == *id)
                        {
                            person.photo = Some(relative.clone());
                            if !person.gallery.iter().any(|entry| entry == &relative) {
                                person.gallery.push(relative);
                            }
                        }
                        clear_person_photo_cache(&mut app.photo_cache, id);
                    }
                }
                app.pending_image = None;
            }
            if ui.button("Zur Galerie hinzufügen").clicked() {
                if let Some(id) = &selected_id {
                    if let Some(relative) = import_media_file_async(ui.ctx(), &app.library, &path) {
                        let name = app
                            .data
                            .find(id)
                            .map(|person| person.display_name())
                            .unwrap_or_else(|| id.clone());
                        app.snapshot(format!("Galeriebild hinzufügen: {name}"));
                        if let Some(person) =
                            app.data.people.iter_mut().find(|person| person.id == *id)
                        {
                            person.gallery.push(relative);
                        }
                    }
                }
                app.pending_image = None;
            }
            if ui.button("Profilbild entfernen").clicked() {
                if let Some(id) = &selected_id {
                    let name = app
                        .data
                        .find(id)
                        .map(|person| person.display_name())
                        .unwrap_or_else(|| id.clone());
                    app.snapshot(format!("Profilbild entfernen: {name}"));
                    if let Some(person) = app.data.people.iter_mut().find(|person| person.id == *id)
                    {
                        person.photo = None;
                        person.photo_crop = None;
                    }
                    clear_person_photo_cache(&mut app.photo_cache, id);
                }
                app.pending_image = None;
            }
            if ui.button("Abbrechen").clicked() {
                app.pending_image = None;
            }
        });
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub fn show_image_intent(app: &mut MiniGramps, _ctx: &egui::Context) {
    if app.pending_image.is_some() {
        app.status = "Foto-Drag-and-drop ist auf diesem Ziel noch nicht implementiert".into();
        app.pending_image = None;
    }
}

/// Ein Foto aus dem Wähler übernehmen: als Profilbild oder (Galerie-Modus)
/// nur in die Galerie legen.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn choose_project_photo(app: &mut MiniGramps, ui: &mut egui::Ui, relative: &str) {
    let mut preview = app.draft.clone();
    preview.photo = Some(relative.to_string());
    if crate::media::gallery_thumbnail_ui(
        ui,
        &preview,
        &mut app.photo_cache,
        &app.library,
        egui::Vec2::new(54.0, 42.0),
    )
    .on_hover_text(relative)
    .clicked()
    {
        if app.photo_chooser_gallery && !app.inline_edit {
            if let Some(id) = app.selected.clone() {
                app.add_gallery_photo_to_person(&id, relative.to_string());
            }
        } else if app.photo_chooser_gallery {
            app.add_gallery_photo(relative.to_string());
            app.status = "Foto in Galerie gelegt".into();
        } else {
            app.set_draft_photo(relative.to_string());
            app.status = "Foto aus Projekt übernommen".into();
        }
        app.photo_chooser = None;
    }
}

/// Foto-Auswahl in Abschnitten: eigene Fotos, nahe Familie (Eltern,
/// Geschwister, Kinder), Großfamilie (angezeigte Karten). Das gewählte Bild
/// landet als Profilbild im Entwurf — oder nur in der Galerie (Modus der
/// Ablagefläche, auch ohne Bearbeitungsmodus).
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn show_photo_chooser(app: &mut MiniGramps, ctx: &egui::Context) {
    let Some(all_images) = app.photo_chooser.clone() else {
        return;
    };
    // Drei Kategorien stehen immer vollständig da: eigene Fotos, nahe
    // Familie (Eltern, Geschwister, Kinder), Großfamilie (übrige Verwandte).
    // Alles darüber hinaus (nicht zuordenbar) bleibt eingeklappt. Ohne
    // Bearbeitungsmodus zählt die Galerie der gewählten Person.
    let draft_id = if app.inline_edit {
        app.draft.id.clone()
    } else {
        app.selected.clone().unwrap_or_default()
    };
    let own: Vec<String> = if app.inline_edit {
        app.draft.gallery.clone()
    } else {
        app.data
            .find(&draft_id)
            .map(|person| person.gallery.clone())
            .unwrap_or_default()
    };
    let mut seen: std::collections::HashSet<String> = own.iter().cloned().collect();
    let mut close_people: Vec<crate::model::Person> = Vec::new();
    close_people.extend(app.data.parents_of(&draft_id).into_iter().cloned());
    close_people.extend(app.data.siblings_of(&draft_id).into_iter().cloned());
    close_people.extend(app.data.children_of(&draft_id).into_iter().cloned());
    let close_ids: std::collections::HashSet<String> =
        close_people.iter().map(|person| person.id.clone()).collect();
    let mut close_images: Vec<String> = Vec::new();
    for person in &close_people {
        for path in person.photo.iter().chain(person.gallery.iter()) {
            if seen.insert(path.clone()) {
                close_images.push(path.clone());
            }
        }
    }
    // Großfamilie = alle gerade im Stammbaum angezeigten Personen (in
    // Zeichenreihenfolge, ohne Selbst und nahe Familie).
    let mut extended_ids: Vec<String> = Vec::new();
    {
        let mut shown_ids: std::collections::HashSet<String> =
            std::collections::HashSet::from([draft_id.clone()]);
        shown_ids.extend(close_ids.iter().cloned());
        for (id, _) in &app.tree_card_rects {
            if shown_ids.insert(id.clone()) {
                extended_ids.push(id.clone());
            }
        }
    }
    let mut extended_images: Vec<String> = Vec::new();
    for id in &extended_ids {
        if let Some(person) = app.data.find(id) {
            for path in person.photo.iter().chain(person.gallery.iter()) {
                if seen.insert(path.clone()) {
                    extended_images.push(path.clone());
                }
            }
        }
    }
    let rest_images: Vec<String> = all_images
        .into_iter()
        .filter(|path| !seen.contains(path))
        .collect();
    let mut open = true;
    egui::Window::new(window_title("Foto wählen"))
        .open(&mut open)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(420.0)
        .show(ctx, |ui| {
            ui.label("Fotos dieser Person:");
            if own.is_empty() {
                ui.label("Keine eigenen Fotos hinterlegt.");
            } else {
                ui.horizontal_wrapped(|ui| {
                    for relative in &own {
                        choose_project_photo(app, ui, relative);
                    }
                });
            }
            egui::CollapsingHeader::new(format!("Nahe Familie · {}", close_images.len()))
                .default_open(true)
                .show(ui, |ui| {
                    if close_images.is_empty() {
                        ui.label("Keine Fotos bei Eltern, Geschwistern und Kindern.");
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            for relative in &close_images {
                                choose_project_photo(app, ui, relative);
                            }
                        });
                    }
                });
            ui.label(format!("Großfamilie · {}:", extended_images.len()));
            if extended_images.is_empty() {
                ui.label("Keine Fotos bei der übrigen Verwandtschaft.");
            } else {
                ui.horizontal_wrapped(|ui| {
                    for relative in &extended_images {
                        choose_project_photo(app, ui, relative);
                    }
                });
            }
            if !rest_images.is_empty() {
                egui::CollapsingHeader::new(format!(
                    "Übrige Fotos · {}",
                    rest_images.len()
                ))
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for relative in &rest_images {
                            choose_project_photo(app, ui, relative);
                        }
                    });
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                // Galerie-Modus (Ablagefläche): nur in die Galerie legen,
                // sonst als Profilbild übernehmen. Ohne Bearbeitungsmodus
                // geht es direkt ans Datenobjekt (mit Undo-Snapshot).
                let take_photo = |app: &mut MiniGramps, relative: String| {
                    if app.photo_chooser_gallery && !app.inline_edit {
                        if let Some(id) = app.selected.clone() {
                            app.add_gallery_photo_to_person(&id, relative);
                        }
                    } else if app.photo_chooser_gallery {
                        app.add_gallery_photo(relative);
                        app.status = "Foto in Galerie gelegt".into();
                    } else {
                        app.set_draft_photo(relative);
                    }
                    app.photo_chooser = None;
                };
                if ui.button("Neue Datei…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Bilder", &["png", "jpg", "jpeg", "webp"])
                        .pick_file()
                    {
                        if let Some(relative) = import_media_file_async(
                            ui.ctx(),
                            &app.library,
                            &path,
                        ) {
                            take_photo(app, relative);
                        }
                    }
                }
                if ui.button("Aus Zwischenablage").clicked() {
                    // Zuerst Bitmap, sonst kopierte Bilddatei (Explorer).
                    if let Some(png) = crate::media::read_clipboard_png() {
                        match crate::media::import_image_bytes(&app.library, &png) {
                            Some(relative) => take_photo(app, relative),
                            None => {
                                app.status = "Zwischenablage-Bild konnte nicht gespeichert werden".into();
                            }
                        }
                    } else if let Some(path) = crate::media::read_clipboard_image_file() {
                        if let Some(relative) = import_media_file_async(
                            ui.ctx(),
                            &app.library,
                            &path,
                        ) {
                            take_photo(app, relative);
                        } else {
                            app.status = "Datei aus Zwischenablage konnte nicht übernommen werden".into();
                        }
                    } else {
                        app.status = "Kein Bild in der Zwischenablage".into();
                    }
                }
                if ui.button("Abbrechen").clicked() {
                    app.photo_chooser = None;
                }
            });
        });
    if !open {
        app.photo_chooser = None;
    }
}

#[cfg(any(target_arch = "wasm32", target_os = "android"))]
pub fn show_photo_chooser(app: &mut MiniGramps, _ctx: &egui::Context) {
    if app.photo_chooser.is_some() {
        app.status = "Fotoauswahl ist auf diesem Ziel noch nicht implementiert".into();
        app.photo_chooser = None;
    }
}

pub fn show_lightbox(app: &mut MiniGramps, ctx: &egui::Context) {
    let Some(path) = app.lightbox_image.clone() else {
        // Lightbox geschlossen -> GPU-Speicher für Vollbilder freigeben!
        crate::media::clear_lightbox_cache(&mut app.photo_cache, &mut app.lightbox_loading, None);
        return;
    };
    // Nur das aktuell angezeigte Vollbild im Cache behalten, andere sofort verwerfen!
    crate::media::clear_lightbox_cache(
        &mut app.photo_cache,
        &mut app.lightbox_loading,
        Some(&path),
    );

    let selected_id = app.selected.clone();
    let gallery = selected_id
        .as_deref()
        .and_then(|id| app.data.find(id))
        .map(|person| person.gallery.clone())
        .unwrap_or_default();
    let current_index = gallery.iter().position(|entry| entry == &path);
    let mut open = true;
    egui::Window::new(window_title("Galerie"))
        .open(&mut open)
        .resizable(true)
        .default_width(760.0)
        .default_height(620.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&path);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_only_button(ui, ICON_EXTERNAL_LINK, "lightbox-external")
                        .on_hover_text(
                            "Original in voller Auflösung im System-Bildbetrachter öffnen",
                        )
                        .clicked()
                    {
                        let raw = std::path::Path::new(&path);
                        let original_path = if raw.is_absolute() {
                            raw.to_path_buf()
                        } else {
                            app.library.join(raw)
                        };
                        let _ = open_in_default_viewer(&original_path);
                    }
                    if let Some(index) = current_index {
                        if icon_only_button(ui, ICON_TRASH, "lightbox-trash")
                            .on_hover_text("Dieses Bild aus der Galerie entfernen")
                            .clicked()
                        {
                            if let Some(id) = &selected_id {
                                let name = app
                                    .data
                                    .find(id)
                                    .map(|person| person.display_name())
                                    .unwrap_or_else(|| id.clone());
                                app.snapshot(format!("Galeriebild entfernen: {name}"));
                                if let Some(person) =
                                    app.data.people.iter_mut().find(|person| person.id == *id)
                                {
                                    person.gallery.retain(|entry| entry != &path);
                                }
                            }
                            app.photo_cache.remove(&format!("lightbox-{path}"));
                            app.lightbox_image = gallery
                                .get(index + 1)
                                .or_else(|| index.checked_sub(1).and_then(|prev| gallery.get(prev)))
                                .cloned();
                            app.status = "Galeriebild entfernt".into();
                            app.save();
                        }

                        let mut next_clicked = false;
                        ui.add_enabled_ui(index + 1 < gallery.len(), |ui| {
                            if icon_only_button(ui, ICON_CHEVRON_RIGHT, "lightbox-next")
                                .on_hover_text("Nächstes Bild")
                                .clicked()
                            {
                                next_clicked = true;
                            }
                        });
                        if next_clicked {
                            app.lightbox_image = Some(gallery[index + 1].clone());
                        }

                        let mut prev_clicked = false;
                        ui.add_enabled_ui(index > 0, |ui| {
                            if icon_only_button(ui, ICON_CHEVRON_LEFT, "lightbox-prev")
                                .on_hover_text("Vorheriges Bild")
                                .clicked()
                            {
                                prev_clicked = true;
                            }
                        });
                        if prev_clicked {
                            app.lightbox_image = Some(gallery[index - 1].clone());
                        }
                    }
                });
            });
            ui.separator();
            let mut image_person = person(&path, "", "", "", Gender::Unknown);
            image_person.photo = Some(path.clone());
            // Asynchron: sofort Vorschau, volles Bild im Hintergrund.
            crate::media::drain_lightbox_textures(
                &app.lightbox_rx,
                &mut app.photo_cache,
                ui.ctx(),
                &mut app.lightbox_loading,
            );
            let state = crate::media::lightbox_texture_async(
                ui.ctx(),
                &image_person,
                &mut app.photo_cache,
                &app.library,
                &mut app.lightbox_loading,
                &app.lightbox_tx,
            );
            // Infozeile unten: Auflösung, Dateigröße, EXIF-Datum, Kamera.
            let meta_line = app
                .photo_meta_cache
                .get(&path)
                .cloned()
                .or_else(|| {
                    let meta = crate::media::read_photo_meta(&app.library, &path)?;
                    app.photo_meta_cache.insert(path.clone(), meta.clone());
                    Some(meta)
                })
                .map(|meta| meta.display_line())
                .unwrap_or_default();
            let key = format!("lightbox-{path}");
            if let crate::media::LightboxState::Ready(texture) = state {
                let available = ui.available_size().max(egui::Vec2::splat(1.0));
                let tv = texture.size_vec2();
                let scale = (available.x / tv.x.max(1.0)).min(available.y / tv.y.max(1.0));
                ui.centered_and_justified(|ui| {
                    ui.add(
                        egui::Image::from_texture(texture).fit_to_exact_size(tv * scale.min(1.0)),
                    );
                });
            } else {
                // Bild lädt noch -> Ladeanimation anzeigen
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label("Hochauflösendes Bild wird im Hintergrund geladen...");
                });
                if let Some(preview) = app.photo_cache.get(&key) {
                    // Vorschau anzeigen, bis das Vollbild fertig ist.
                    let available = ui.available_size().max(egui::Vec2::splat(1.0));
                    let tv = preview.size_vec2();
                    let scale = (available.x / tv.x.max(1.0)).min(available.y / tv.y.max(1.0));
                    ui.centered_and_justified(|ui| {
                        ui.add(
                            egui::Image::from_texture(preview)
                                .fit_to_exact_size(tv * scale.min(1.0)),
                        );
                    });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.add(egui::Spinner::new().size(40.0));
                    });
                }
            }
            if !meta_line.is_empty() {
                ui.separator();
                ui.label(
                    egui::RichText::new(meta_line)
                        .small()
                        .color(crate::ui::panels::dim_text(ui)),
                );
            }
        });
    if !open {
        app.lightbox_image = None;
    }
}

/// Öffnet einen Pfad im Standard-Bildbetrachter des Betriebssystems.
fn open_in_default_viewer(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
    }
    Ok(())
}
