//! Seitenleisten: Personenliste links, Profilansicht rechts.
//!
//! Verdrahtung:
//! - Links: Gruppen nach Nachnamen (Sortierung via `MiniGramps::group_by_count`,
//!   Zahnrad oben rechts). Klick öffnet zur Ansicht (`selected`); die
//!   Referenzperson wird bewusst NICHT hier gesetzt (dafür "Als Referenz
//!   setzen" oder Baum-Aktionen).
//! - Rechts: Ansicht/Bearbeitung (`selected`/`draft`/`inline_edit`), Stift-
//!   bzw. Disketten-Button, "Als Referenz setzen" (`reference`), Beziehungs-
//!   kategorien mit `+`-Picker (`picker::suggestions`, nur Bearbeitungsmodus)
//!   und die Galerie mit Drag-and-drop (`pending_image` → Intent-Dialog).
//! - Fotos laufen über `crate::media` (avatar_ui, import_media_file).

use eframe::egui::{self, Color32, Sense, Stroke, Vec2};
use rfd::FileDialog;

use crate::media::{avatar_ui, import_media_file};
use crate::model::{ChildRelation, Gender, Person, person};
use crate::ui::{
    ICON_ADD_PERSON, ICON_CHILD, ICON_EDIT, ICON_PARENT, ICON_PARTNER, ICON_REFERENCE, ICON_SAVE,
    ICON_SETTINGS, ICON_SIBLING, MiniGramps, icon_button, icon_only_button, panels::palette,
    picker,
};

/// Linke Seitenleiste: Personen nach Nachnamen gruppiert.
pub fn show_left(app: &mut MiniGramps, ctx: &egui::Context) {
    let colors = palette(app.dark_mode);
    egui::SidePanel::left("people")
        .resizable(true)
        .default_width(230.0)
        .frame(egui::Frame::new().fill(colors.panel).inner_margin(10))
        .show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("PERSONEN")
                        .small()
                        .strong()
                        .color(colors.section),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_only_button(ui, ICON_SETTINGS, "group-sort")
                        .on_hover_text(if app.group_by_count {
                            "Sortierung: Anzahl (Klick für Alphabet)"
                        } else {
                            "Sortierung: Alphabet (Klick für Anzahl)"
                        })
                        .clicked()
                    {
                        app.group_by_count = !app.group_by_count;
                    }
                });
            });
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Eigene Kopien (id/name/gender), damit im Closure auch
                    // `app.set_reference` aufgerufen werden kann.
                    type PersonRow = (String, Gender, String);
                    let mut groups: Vec<(String, Vec<PersonRow>)> = Vec::new();
                    for p in &app.data.people {
                        let surname = if p.family_name.is_empty() {
                            p.display_name()
                        } else {
                            p.family_name.clone()
                        }
                        .to_lowercase();
                        match groups.iter_mut().find(|(name, _)| *name == surname) {
                            Some((_, members)) => {
                                members.push((p.id.clone(), p.gender, p.display_name()))
                            }
                            None => groups
                                .push((surname, vec![(p.id.clone(), p.gender, p.display_name())])),
                        }
                    }
                    if app.group_by_count {
                        groups.sort_by(|(a, members_a), (b, members_b)| {
                            members_b.len().cmp(&members_a.len()).then_with(|| a.cmp(b))
                        });
                    } else {
                        groups.sort_by(|(a, _), (b, _)| a.cmp(b));
                    }
                    for (surname, members) in &mut groups {
                        members.sort_by_key(|(_, _, name)| name.to_lowercase());
                        let mut chars = surname.chars();
                        let display = match chars.next() {
                            Some(first) => {
                                first.to_uppercase().collect::<String>() + chars.as_str()
                            }
                            None => String::new(),
                        };
                        ui.collapsing(format!("{display} · {}", members.len()), |ui| {
                            for (id, gender, name) in members.iter() {
                                let active = app.selected.as_deref() == Some(id.as_str());
                                if ui
                                    .selectable_label(
                                        active,
                                        format!("{}  {}", picker::gender_symbol(*gender), name),
                                    )
                                    .clicked()
                                {
                                    // Ansicht öffnen; Shift/Dreifachklick
                                    // setzt direkt die Referenzperson.
                                    app.selected = Some(id.clone());
                                    if picker::wants_reference(ui) {
                                        app.set_reference(id);
                                    }
                                }
                            }
                        });
                    }
                });
            ui.separator();
            if icon_button(ui, ICON_ADD_PERSON, "add-person", "Person hinzufügen").clicked() {
                app.editing = None;
                app.draft = person(
                    &format!("p{}", app.data.people.len() + 1),
                    "",
                    "",
                    "",
                    Gender::Unknown,
                );
                app.show_editor = true;
            }
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(format!(
                    "{} Personen · {} Familien",
                    app.data.people.len(),
                    app.data.families.len()
                ))
                .small()
                .color(crate::ui::panels::dim_text(ui)),
            );
        });
}

/// Rechte Seitenleiste: Ansicht/Bearbeitung der ausgewählten Person.
pub fn show_right(app: &mut MiniGramps, ctx: &egui::Context) {
    let colors = palette(app.dark_mode);
    egui::SidePanel::right("details")
        .default_width(250.0)
        .frame(egui::Frame::new().fill(colors.panel).inner_margin(10))
        .show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("PERSON")
                        .small()
                        .strong()
                        .color(colors.section),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Stift (Bearbeiten starten) / Diskette (speichern).
                    let icon = if app.inline_edit {
                        ICON_SAVE
                    } else {
                        ICON_EDIT
                    };
                    // Als-Referenz-Icon (Stern) neben dem Stift, nur wenn die
                    // angezeigte Person nicht bereits Referenz ist.
                    if app.reference.as_deref() != app.selected.as_deref() {
                        if icon_only_button(ui, ICON_REFERENCE, "set-reference")
                            .on_hover_text("Als Referenz setzen")
                            .clicked()
                        {
                            if let Some(id) = app.selected.clone() {
                                app.set_reference(&id);
                            }
                        }
                    }
                    if icon_only_button(ui, icon, "profile-edit").clicked() {
                        if app.inline_edit {
                            if let Some(person) = app
                                .data
                                .people
                                .iter_mut()
                                .find(|person| person.id == app.draft.id)
                            {
                                *person = app.draft.clone();
                            }
                            app.photo_cache.remove(&app.draft.id);
                            app.status = "Profil gespeichert".into();
                            // Auch auf die Festplatte schreiben — sonst sind
                            // Foto/Änderungen nach Neustart weg.
                            app.save();
                            app.inline_edit = false;
                            app.relation_picker = None;
                            app.relation_query.clear();
                        } else if let Some(id) = &app.selected {
                            if let Some(person) = app.data.find(id).cloned() {
                                app.draft = person;
                                app.inline_edit = true;
                            }
                        }
                    }
                });
            });
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Einheitliches Verhalten: Personenwechsel während der
                    // Inline-Bearbeitung lädt die neue Person in das
                    // Bearbeitungsformular (Familie und Felder synchron).
                    if app.inline_edit {
                        match &app.selected {
                            Some(id) if id != &app.draft.id => {
                                if let Some(person) = app.data.find(id).cloned() {
                                    app.draft = person;
                                }
                            }
                            _ => {}
                        }
                    }
                    if let Some(id) = &app.selected.clone() {
                        if let Some(p) = app.data.find(id).cloned() {
                            profile(app, ui, colors.section, &p);
                        }
                    }
                });
        });
}

/// Profilinhalt (Avatar, Daten, Familie, Galerie) der angezeigten Person.
fn profile(app: &mut MiniGramps, ui: &mut egui::Ui, section_accent: Color32, p: &Person) {
    let parents: Vec<_> = app.data.parents_of(&p.id).into_iter().cloned().collect();
    let siblings: Vec<_> = app.data.siblings_of(&p.id).into_iter().cloned().collect();
    let children: Vec<_> = app.data.children_of(&p.id).into_iter().cloned().collect();
    let partners: Vec<_> = app.data.partners_of(&p.id).into_iter().cloned().collect();
    ui.add_space(18.0);
    if app.inline_edit {
        // Bearbeitungsmodus: Avatar-Klick wählt ein neues Foto (wird nach
        // media/ kopiert, siehe `media::import_media_file`); Ziehen auf dem
        // Avatar verschiebt den Bildausschnitt.
        let avatar_response = avatar_ui(ui, &app.draft, &mut app.photo_cache, &app.library, 64.0);
        if avatar_response.dragged() {
            let delta = avatar_response.drag_delta();
            let crop = app
                .draft
                .photo_crop
                .get_or_insert_with(crate::model::PhotoCrop::default);
            crop.x = (crop.x - delta.x / 32.0).clamp(-1.0, 1.0);
            crop.y = (crop.y - delta.y / 32.0).clamp(-1.0, 1.0);
        }
        if avatar_response.clicked() {
            if let Some(path) = FileDialog::new()
                .add_filter("Bilder", &["png", "jpg", "jpeg", "webp"])
                .pick_file()
            {
                if let Some(relative) = import_media_file(&app.library, &path) {
                    app.draft.photo = Some(relative);
                }
            }
        }
        // Bildausschnitt: Zoom regeln, Reset stellt das volle Bild ein.
        ui.horizontal(|ui| {
            ui.label("Ausschnitt");
            let crop = app
                .draft
                .photo_crop
                .get_or_insert_with(crate::model::PhotoCrop::default);
            ui.add(
                egui::Slider::new(&mut crop.zoom, 1.0..=4.0)
                    .show_value(false)
                    .text("Zoom"),
            );
            if ui.small_button("Reset").clicked() {
                app.draft.photo_crop = None;
            }
        });
        // Größerer Abstand zwischen Profilbild und Namensfeldern.
        ui.add_space(8.0);
        ui.add(egui::TextEdit::singleline(&mut app.draft.given_name).hint_text("Vorname"));
        ui.add(egui::TextEdit::singleline(&mut app.draft.family_name).hint_text("Nachname"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut app.draft.gender, Gender::Female, "W");
            ui.radio_value(&mut app.draft.gender, Gender::Male, "M");
            ui.radio_value(&mut app.draft.gender, Gender::Unknown, "?");
        });
    } else {
        avatar_ui(ui, p, &mut app.photo_cache, &app.library, 64.0);
        // Größerer Abstand zwischen Profilbild und Name.
        ui.add_space(8.0);
        ui.heading(p.display_name());
        ui.label(picker::gender_label(p.gender));
    }
    ui.add_space(12.0);
    if app.inline_edit {
        picker::editable_info_row(
            ui,
            "GEBOREN",
            &mut app.draft.birth,
            &mut app.draft.birth_place,
        );
        picker::editable_info_row(
            ui,
            "GESTORBEN",
            &mut app.draft.death,
            &mut app.draft.death_place,
        );
        ui.label("QUELLE");
        ui.add(egui::TextEdit::singleline(&mut app.draft.source).hint_text("Quelle"));
        ui.label("NOTIZEN");
        ui.add(
            egui::TextEdit::multiline(&mut app.draft.notes)
                .hint_text("Notizen")
                .desired_rows(2),
        );
    } else {
        picker::info_row(
            ui,
            "GEBOREN",
            &picker::dated_place(&p.birth, &p.birth_place),
        );
        picker::info_row(
            ui,
            "GESTORBEN",
            &picker::dated_place(&p.death, &p.death_place),
        );
    }
    if !p.source.is_empty() {
        picker::info_row(ui, "QUELLE", &p.source);
    }
    if !p.notes.is_empty() {
        ui.label(
            egui::RichText::new("NOTIZEN")
                .small()
                .color(crate::ui::panels::dim_text(ui)),
        );
        ui.label(&p.notes);
    }
    ui.separator();
    ui.label(
        egui::RichText::new("FAMILIE")
            .small()
            .strong()
            .color(section_accent),
    );
    ui.add_space(5.0);
    relation_section(
        app,
        ui,
        "PARTNER",
        crate::ui::tree::RelationKind::Partner,
        &partners,
        ICON_PARTNER,
    );
    relation_section(
        app,
        ui,
        "ELTERN",
        crate::ui::tree::RelationKind::Parent,
        &parents,
        ICON_PARENT,
    );
    relation_section(
        app,
        ui,
        "GESCHWISTER",
        crate::ui::tree::RelationKind::Sibling,
        &siblings,
        ICON_SIBLING,
    );
    // Kinder zeigen ihre Beziehungsart (adoptiert usw.) mit an.
    ui.add_space(7.0);
    picker::relation_header(
        ui,
        "KINDER",
        crate::ui::tree::RelationKind::Child,
        &mut app.relation_picker,
        app.inline_edit,
    );
    if children.is_empty() {
        ui.label(
            egui::RichText::new("Nicht hinterlegt")
                .italics()
                .color(crate::ui::panels::dim_text(ui)),
        );
    }
    for child in children {
        let relation = app.data.relation_of_child(&p.id, &child.id);
        let mut display = child.clone();
        if relation != ChildRelation::Birth {
            display.name = format!("{} · {}", display.display_name(), relation.label());
        }
        ui.horizontal(|ui| {
            if app.inline_edit {
                let clicked = picker::relationship_row(
                    ui,
                    ICON_CHILD,
                    "child",
                    &display,
                    &mut app.photo_cache,
                    &app.library,
                );
                if clicked {
                    app.relation_editor = match app.relation_editor.take() {
                        Some((crate::ui::tree::RelationKind::Child, open_id))
                            if open_id == child.id =>
                        {
                            None
                        }
                        _ => Some((crate::ui::tree::RelationKind::Child, child.id.clone())),
                    };
                }
                if ui.small_button("✕").clicked() {
                    app.data.unlink_child(&p.id, &child.id);
                    app.relation_editor = None;
                }
            } else if picker::relationship_row(
                ui,
                ICON_CHILD,
                "child",
                &display,
                &mut app.photo_cache,
                &app.library,
            ) {
                let child_id = child.id.clone();
                app.selected = Some(child_id.clone());
                if picker::wants_reference(ui) {
                    app.set_reference(&child_id);
                }
            }
        });
        if app
            .relation_editor
            .as_ref()
            .is_some_and(|(open_kind, open_id)| {
                *open_kind == crate::ui::tree::RelationKind::Child && open_id == child.id.as_str()
            })
        {
            picker::relation_options(
                app,
                ui,
                crate::ui::tree::RelationKind::Child,
                &p.id,
                &child.id,
            );
        }
    }
    if app.relation_picker == Some(crate::ui::tree::RelationKind::Child) {
        // Standardauswahl beim Öffnen: erster Partner als weiteres Elternteil.
        if app.pending_child_for.as_deref() != Some(&p.id) {
            app.pending_child_for = Some(p.id.clone());
            app.pending_child_partner = app
                .data
                .partners_of(&p.id)
                .first()
                .map(|partner| partner.id.clone());
            app.pending_child_relation = ChildRelation::Birth;
        }
        let partners: Vec<_> = app.data.partners_of(&p.id).into_iter().cloned().collect();
        if let Some(id) = &app.pending_child_partner {
            if !partners.iter().any(|partner| &partner.id == id) {
                app.pending_child_partner = None;
            }
        }
        picker::suggestions(app, ui, crate::ui::tree::RelationKind::Child, &p.id);
    }
    ui.separator();
    gallery(app, ui, section_accent, p);
}

/// Eine Beziehungskategorie: Einträge + optionaler Picker (nur Bearbeitung).
fn relation_section(
    app: &mut MiniGramps,
    ui: &mut egui::Ui,
    label: &str,
    kind: crate::ui::tree::RelationKind,
    entries: &[Person],
    icon_bytes: &'static [u8],
) {
    picker::relation_header(ui, label, kind, &mut app.relation_picker, app.inline_edit);
    if entries.is_empty() {
        ui.label(
            egui::RichText::new("Nicht hinterlegt")
                .italics()
                .color(crate::ui::panels::dim_text(ui)),
        );
    }
    for entry in entries {
        // Zeile + (im Bearbeitungsmodus) Löschen-Knopf; Klick auf den Namen
        // öffnet dann den Beziehungseditor statt der Person.
        ui.horizontal(|ui| {
            if app.inline_edit {
                let clicked = picker::relationship_row(
                    ui,
                    icon_bytes,
                    label.to_lowercase().as_str(),
                    entry,
                    &mut app.photo_cache,
                    &app.library,
                );
                if clicked && kind != crate::ui::tree::RelationKind::Sibling {
                    // Geschwister haben keine Beziehungsoptionen -> nicht aufklappbar.
                    app.relation_editor = match app.relation_editor.take() {
                        Some((open_kind, open_id)) if open_kind == kind && open_id == entry.id => {
                            None
                        }
                        _ => Some((kind, entry.id.clone())),
                    };
                }
                if ui.small_button("✕").clicked() {
                    match kind {
                        crate::ui::tree::RelationKind::Partner => app
                            .data
                            .unlink_partner(&app.selected.clone().unwrap_or_default(), &entry.id),
                        crate::ui::tree::RelationKind::Parent => app
                            .data
                            .unlink_parent(&app.selected.clone().unwrap_or_default(), &entry.id),
                        crate::ui::tree::RelationKind::Sibling => app
                            .data
                            .unlink_sibling(&app.selected.clone().unwrap_or_default(), &entry.id),
                        crate::ui::tree::RelationKind::Child => app
                            .data
                            .unlink_child(&app.selected.clone().unwrap_or_default(), &entry.id),
                    }
                    app.relation_editor = None;
                }
            } else {
                if picker::relationship_row(
                    ui,
                    icon_bytes,
                    label.to_lowercase().as_str(),
                    entry,
                    &mut app.photo_cache,
                    &app.library,
                ) {
                    app.selected = Some(entry.id.clone());
                    if picker::wants_reference(ui) {
                        app.set_reference(&entry.id);
                    }
                }
            }
        });
        if app
            .relation_editor
            .as_ref()
            .is_some_and(|(open_kind, open_id)| *open_kind == kind && open_id == entry.id.as_str())
        {
            picker::relation_options(
                app,
                ui,
                kind,
                &app.selected.clone().unwrap_or_default(),
                &entry.id,
            );
        }
    }
    if app.relation_picker == Some(kind) {
        picker::suggestions(app, ui, kind, &app.selected.clone().unwrap_or_default());
    }
    ui.add_space(7.0);
}

/// Galerie: Drop-Zone + Miniaturen (Pseudo-IDs `gallery-<id>-<index>` für
/// den Textur-Cache, siehe `media::photo_texture`).
fn gallery(app: &mut MiniGramps, ui: &mut egui::Ui, section_accent: Color32, p: &Person) {
    ui.label(
        egui::RichText::new("GALERIE")
            .small()
            .strong()
            .color(section_accent),
    );
    let (drop_response, _) =
        ui.allocate_painter(Vec2::new(ui.available_width(), 70.0), Sense::hover());
    ui.painter().rect_stroke(
        drop_response.rect,
        6.0,
        Stroke::new(1.0, crate::ui::panels::dim_text(ui)),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        drop_response.rect.center(),
        eframe::egui::Align2::CENTER_CENTER,
        "Foto hier ablegen",
        eframe::egui::FontId::proportional(12.0),
        crate::ui::panels::dim_text(ui),
    );
    for file in ui.ctx().input(|input| input.raw.dropped_files.clone()) {
        if drop_response.hovered() {
            if let Some(path) = file.path {
                app.pending_image = Some(path);
            }
        }
    }
    ui.horizontal_wrapped(|ui| {
        for (index, path) in p.gallery.iter().enumerate() {
            let mut gallery_photo = p.clone();
            gallery_photo.id = format!("gallery-{}-{index}", p.id);
            gallery_photo.photo = Some(path.clone());
            avatar_ui(ui, &gallery_photo, &mut app.photo_cache, &app.library, 42.0);
        }
    });
}
