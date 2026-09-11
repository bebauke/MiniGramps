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
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use rfd::FileDialog;
use std::collections::HashMap;

use crate::media::{
    avatar_ui_live, avatar_ui_preview, clear_person_photo_cache, gallery_thumbnail_ui,
};
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
use crate::media::import_media_file_async;
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
                    let sort_button = icon_only_button(ui, ICON_SETTINGS, "group-sort")
                        .on_hover_text("Sortierung der Personenliste");
                    egui::Popup::menu(&sort_button).show(|ui| {
                        ui.label(egui::RichText::new("Sortierung").strong());
                        ui.separator();
                        if ui
                            .selectable_label(app.group_by_count, "Gruppengröße (Anzahl)")
                            .clicked()
                        {
                            app.group_by_count = true;
                            app.people_groups_dirty = true;
                            ui.close();
                        }
                        if ui
                            .selectable_label(!app.group_by_count, "Alphabetisch")
                            .clicked()
                        {
                            app.group_by_count = false;
                            app.people_groups_dirty = true;
                            ui.close();
                        }
                    });
                });
            });
            ui.add_space(2.0);
            ui.add(
                egui::TextEdit::singleline(&mut app.people_filter)
                    .hint_text("Person suchen…")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if app.people_groups_dirty {
                        let mut grouped: HashMap<String, Vec<(String, Gender, String)>> =
                            HashMap::new();
                        for person in &app.data.people {
                            let surname = if person.family_name.is_empty() {
                                person.display_name()
                            } else {
                                person.family_name.clone()
                            }
                            .to_lowercase();
                            grouped.entry(surname).or_default().push((
                                person.id.clone(),
                                person.gender,
                                person.display_name(),
                            ));
                        }
                        let mut groups: Vec<_> = grouped.into_iter().collect();
                        for (_, members) in &mut groups {
                            members.sort_by_key(|(_, _, name)| name.to_lowercase());
                        }
                        if app.group_by_count {
                            groups.sort_by(|(a, members_a), (b, members_b)| {
                                members_b.len().cmp(&members_a.len()).then_with(|| a.cmp(b))
                            });
                        } else {
                            groups.sort_by(|(a, _), (b, _)| a.cmp(b));
                        }
                        app.people_groups = groups;
                        app.people_groups_dirty = false;
                    }
                    let selected = app.selected.as_deref();
                    let filter = app.people_filter.trim().to_lowercase();
                    let filtering = !filter.is_empty();
                    let mut requested_selection = None;
                    for (surname, members) in &app.people_groups {
                        // Bei aktivem Filter: nur passende Personen; leere
                        // Gruppen ausblenden. Alle Treffer-Gruppen aufgeklappt.
                        let shown: Vec<&(String, Gender, String)> = if filtering {
                            members
                                .iter()
                                .filter(|(_, _, name)| name.to_lowercase().contains(&filter))
                                .collect()
                        } else {
                            members.iter().collect()
                        };
                        if filtering && shown.is_empty() {
                            continue;
                        }
                        let mut chars = surname.chars();
                        let display = match chars.next() {
                            Some(first) => {
                                first.to_uppercase().collect::<String>() + chars.as_str()
                            }
                            None => String::new(),
                        };
                        let count = shown.len();
                        let mut header = egui::CollapsingHeader::new(format!("{display} · {count}"))
                            .id_salt(surname.as_str());
                        if filtering {
                            header = header.open(Some(true));
                        }
                        header.show(ui, |ui| {
                            // Namen nicht umbrechen lassen, sondern in „…“
                            // übergehen lassen, damit lange Namen die Liste
                            // nicht stauchen.
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                            for entry in &shown {
                                let (id, gender, name) = *entry;
                                let active = selected == Some(id.as_str());
                                if ui
                                    .selectable_label(
                                        active,
                                        format!("{}  {}", picker::gender_symbol(*gender), name),
                                    )
                                    .clicked()
                                {
                                    // Ansicht öffnen; Shift/Dreifachklick
                                    // setzt direkt die Referenzperson.
                                    requested_selection =
                                        Some((id.clone(), picker::wants_reference(ui)));
                                }
                            }
                        });
                    }
                    if let Some((id, set_reference)) = requested_selection {
                        app.request_select(&id, set_reference);
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
                section_title(ui, "PERSON", colors.section, app);
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
                            app.commit_draft();
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
    {
        if app.inline_edit {
            ui.horizontal_top(|ui| {
                // Links das Bild, rechts Foto-Aktionen und Ausschnitt.
                let avatar_response =
                    avatar_ui_live(ui, &app.draft, &mut app.photo_cache, &app.library, 64.0);
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
                    #[cfg(any(target_arch = "wasm32", target_os = "android"))]
                    {
                        app.status = "Fotoauswahl ist auf diesem Ziel noch nicht implementiert".into();
                    }

                    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
                    {
                    if let Some(path) = FileDialog::new()
                        .add_filter("Bilder", &["png", "jpg", "jpeg", "webp"])
                        .pick_file()
                    {
                        if let Some(relative) =
                            import_media_file_async(ui.ctx(), &app.library, &path)
                        {
                            app.draft.photo = Some(relative.clone());
                            if !app.draft.gallery.iter().any(|entry| entry == &relative) {
                                app.draft.gallery.push(relative);
                            }
                            clear_person_photo_cache(&mut app.photo_cache, &app.draft.id);
                        }
                    }
                    }
                }
                ui.add_space(12.0);
                ui.vertical(|ui| {
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
                    ui.horizontal(|ui| {
                        if ui.button("Foto löschen").clicked() {
                            app.draft.photo = None;
                            app.draft.photo_crop = None;
                            clear_person_photo_cache(&mut app.photo_cache, &app.draft.id);
                        }
                    });
                });
            });
            ui.add_space(8.0);
            ui.add(egui::TextEdit::singleline(&mut app.draft.given_name).hint_text("Vorname"));
            ui.add(egui::TextEdit::singleline(&mut app.draft.family_name).hint_text("Nachname"));
            ui.horizontal(|ui| {
                ui.radio_value(&mut app.draft.gender, Gender::Female, "W");
                ui.radio_value(&mut app.draft.gender, Gender::Male, "M");
                ui.radio_value(&mut app.draft.gender, Gender::Unknown, "?");
            });
        } else {
            avatar_ui_preview(ui, p, &mut app.photo_cache, &app.library, 64.0);
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
    } // Ende PERSON
    if !is_hidden(app, "NAMEN") {
        ui.separator();
        section_title(ui, "NAMEN", section_accent, app);
        if !app.collapsed_sections.contains("NAMEN") {
            let mut name_entries: Vec<(String, String)> = Vec::new();
            if !p.title.is_empty() {
                name_entries.push(("Titel".into(), p.title.clone()));
            }
            if !p.nick_name.is_empty() {
                name_entries.push(("Spitzname".into(), p.nick_name.clone()));
            }
            if !p.call_name.is_empty() {
                name_entries.push(("Rufname".into(), p.call_name.clone()));
            }
            if !p.suffix.is_empty() {
                name_entries.push(("Namenszusatz".into(), p.suffix.clone()));
            }
            if !p.name_prefix.is_empty() {
                name_entries.push(("Namenspräfix".into(), p.name_prefix.clone()));
            }
            if !p.surname_prefix.is_empty() {
                name_entries.push(("Namenspräfix".into(), p.surname_prefix.clone()));
            }
            if !p.name_type.is_empty() {
                name_entries.push(("Namensart".into(), p.name_type.clone()));
            }
            if !p.name_origin.is_empty() {
                name_entries.push(("Herkunft".into(), p.name_origin.clone()));
            }
            if name_entries.is_empty() {
                ui.label(
                    egui::RichText::new("Keine weiteren Namen")
                        .italics()
                        .color(crate::ui::panels::dim_text(ui)),
                );
            } else {
                for (label, value) in name_entries {
                    picker::info_row(ui, &label, &value);
                }
            }
        }
    }
    if !is_hidden(app, "EREIGNISSE") {
        ui.separator();
        section_title(ui, "EREIGNISSE", section_accent, app);
        if !app.collapsed_sections.contains("EREIGNISSE") {
            let mut has_any = false;

            // 1. Geburt (Birth)
            if !p.birth.is_empty() || !p.birth_place.is_empty() {
                picker::info_row(ui, "Geburt", &picker::dated_place(&p.birth, &p.birth_place));
                has_any = true;
            }

            // 2. Tod (Death)
            if !p.death.is_empty() || !p.death_place.is_empty() {
                picker::info_row(ui, "Tod", &picker::dated_place(&p.death, &p.death_place));
                has_any = true;
            }

            // 3. Andere Ereignisse (die nicht Geburt/Tod sind, um Duplikate zu vermeiden!)
            for event in &p.events {
                if event.kind != crate::model::EventKind::Birth
                    && event.kind != crate::model::EventKind::Death
                {
                    let value = picker::dated_place(&event.date, &event.place);
                    picker::info_row(ui, event.kind.label(), &value);
                    has_any = true;
                }
            }

            // 4. Heirat / Scheidung von Partnern (falls nicht schon gelistet)
            for partner in &partners {
                for event in &partner.events {
                    if event.kind == crate::model::EventKind::Marriage
                        || event.kind == crate::model::EventKind::Divorce
                    {
                        let value = picker::dated_place(&event.date, &event.place);
                        picker::info_row(ui, event.kind.label(), &value);
                        has_any = true;
                    }
                }
            }

            if !has_any {
                ui.label(
                    egui::RichText::new("Keine Ereignisse")
                        .italics()
                        .color(crate::ui::panels::dim_text(ui)),
                );
            }
        }
    }
    if !is_hidden(app, "FAMILIE") {
        ui.separator();
        section_title(ui, "FAMILIE", section_accent, app);
        if !app.collapsed_sections.contains("FAMILIE") {
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
            for child in &children {
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
                            app.snapshot(format!(
                                "Kind-Verknüpfung lösen: {}",
                                child.display_name()
                            ));
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
                        app.request_select(&child_id, picker::wants_reference(ui));
                    }
                });
                if app
                    .relation_editor
                    .as_ref()
                    .is_some_and(|(open_kind, open_id)| {
                        *open_kind == crate::ui::tree::RelationKind::Child
                            && open_id == child.id.as_str()
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

            // Sonstige (falls vorhanden und nicht familiär abgedeckt)
            let sonstige: Vec<&Person> = Vec::new();
            // (Muster-Prüfung: Falls wir künftig sonstige Verknüpfungen haben, listen wir sie hier)
            if !sonstige.is_empty() {
                ui.add_space(5.0);
                ui.label(
                    egui::RichText::new("SONSTIGE")
                        .small()
                        .color(crate::ui::panels::dim_text(ui)),
                );
                for entry in sonstige {
                    picker::relationship_row(
                        ui,
                        ICON_SIBLING,
                        "sonstige",
                        entry,
                        &mut app.photo_cache,
                        &app.library,
                    );
                }
            }
        }
    }
    if !is_hidden(app, "GALERIE") {
        ui.separator();
        section_title(ui, "GALERIE", section_accent, app);
        if !app.collapsed_sections.contains("GALERIE") {
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
                    // Proaktiv 720p-Kopie im Hintergrund generieren, damit das Vollbild beim ersten Klick absolut verzögerungsfrei öffnet!
                    let _ =
                        crate::media::ensure_large_thumb(ui.ctx(), &app.library, &gallery_photo);
                    if gallery_thumbnail_ui(
                        ui,
                        &gallery_photo,
                        &mut app.photo_cache,
                        &app.library,
                        Vec2::new(54.0, 42.0),
                    )
                    .clicked()
                    {
                        app.lightbox_image = Some(path.clone());
                    }
                }
            });
        }
    }
}

/// Ist eine Kategorie per Rechtsklick ausgeblendet?
fn is_hidden(app: &MiniGramps, name: &str) -> bool {
    app.hidden_sections.contains(name)
}

/// Zeigt ein Kontextmenü mit allen verfügbaren Panels und Markierungen (Häkchen) für die angezeigten Panels.
fn show_panes_context_menu(ui: &mut egui::Ui, app: &mut MiniGramps) {
    ui.label(egui::RichText::new("Panels anzeigen/ausblenden").strong());
    ui.separator();

    // PERSON ist immer sichtbar und kann nicht ausgeblendet werden
    let mut person_visible = true;
    ui.add_enabled_ui(false, |ui| {
        ui.checkbox(&mut person_visible, "PERSON");
    });

    let all_panes = ["NAMEN", "EREIGNISSE", "FAMILIE", "REFERENZEN", "GALERIE"];
    for pane in all_panes {
        let visible = !is_hidden(app, pane);
        let mut check = visible;
        if ui.checkbox(&mut check, pane).clicked() {
            if check {
                app.hidden_sections.remove(pane);
            } else {
                app.hidden_sections.insert(pane.to_string());
            }
            ui.close();
        }
    }
}

/// Kategorieüberschrift mit Rechtsklick-Menü für alle Panels (wie in Gramps).
/// Ein Klick klappt die Sektion ein/aus (außer bei PERSON).
fn section_title(ui: &mut egui::Ui, name: &str, accent: Color32, app: &mut MiniGramps) {
    let hidden = is_hidden(app, name);
    if hidden && name != "PERSON" {
        return;
    }

    let collapsed = app.collapsed_sections.contains(name);
    let display_name = if name == "PERSON" {
        name.to_string()
    } else if collapsed {
        format!("> {name}")
    } else {
        format!("v {name}")
    };

    let label = egui::RichText::new(display_name)
        .small()
        .strong()
        .color(accent);

    let response = ui.add(egui::Label::new(label).sense(Sense::click()));

    // Linksklick: Einklappen togglen (PERSON lässt sich weder einklappen noch ausblenden)
    if response.clicked() && name != "PERSON" {
        if collapsed {
            app.collapsed_sections.remove(name);
        } else {
            app.collapsed_sections.insert(name.to_string());
        }
    }

    // Rechtsklick: Zeigt eine Liste aller verfügbaren Panes mit Haken
    egui::Popup::context_menu(&response).show(|ui| {
        show_panes_context_menu(ui, app);
    });
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
    let old_picker = app.relation_picker;
    picker::relation_header(ui, label, kind, &mut app.relation_picker, app.inline_edit);
    if app.relation_picker != old_picker {
        app.relation_query.clear();
        app.relation_family_name.clear();
    }
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
                    let relation = match kind {
                        crate::ui::tree::RelationKind::Partner => "Partner-Verknüpfung",
                        crate::ui::tree::RelationKind::Parent => "Eltern-Verknüpfung",
                        crate::ui::tree::RelationKind::Sibling => "Geschwister-Verknüpfung",
                        crate::ui::tree::RelationKind::Child => "Kind-Verknüpfung",
                    };
                    app.snapshot(format!("{relation} lösen: {}", entry.display_name()));
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
                    app.request_select(&entry.id, picker::wants_reference(ui));
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
