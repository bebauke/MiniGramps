//! Beziehungspicker und Profil-Widgets.
//!
//! Verdrahtung:
//! - `suggestions` wird von `sidebar::show_right` gerufen, sobald eine
//!   Kategorie per `+` (`relation_header`, nur Bearbeitungsmodus) geöffnet
//!   ist. Suchfeld mit max. 5 Vorschlägen; beim Kind zusätzlich Inline-
//!   Selektoren für Partner (`pending_child_partner`) und Beziehungsart
//!   (`pending_child_relation`). Verknüpfen läuft über die `model::TreeData`-
//!   Methoden `link_partner`, `link_child`, `link_child_to`.
//! - `relationship_row` / `info_row` / `editable_info_row` / `dated_place` /
//!   `gender_*` sind die kleinen Profil-Bausteine der rechten Seitenleiste.

use std::collections::HashMap;

use eframe::egui::{self, TextureHandle};

use crate::media::avatar_ui_preview;
use crate::model::{ChildRelation, Gender, Person, person};
use crate::ui::tree::RelationKind;
use crate::ui::{ICON_CLOSE, MiniGramps, icon, icon_only_button};

/// Kategorieüberschrift mit `+`-Schalter (nur Bearbeitungsmodus).
pub fn relation_header(
    ui: &mut egui::Ui,
    label: &str,
    kind: RelationKind,
    active: &mut Option<RelationKind>,
    editable: bool,
) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .small()
                .color(crate::ui::panels::dim_text(ui)),
        );
        if editable && ui.small_button("+").clicked() {
            *active = if *active == Some(kind) {
                None
            } else {
                Some(kind)
            };
        }
    });
}

/// Shift gehalten? → Name direkt als Referenz setzen (Listen, Zeilen).
pub fn wants_reference(ui: &egui::Ui) -> bool {
    ui.ctx().input(|i| i.modifiers.shift)
}

/// Beziehungszeile: Avatar + Icon + klickbarer Name (öffnet zur Ansicht).
pub fn relationship_row(
    ui: &mut egui::Ui,
    icon_bytes: &'static [u8],
    icon_id: &str,
    person: &Person,
    cache: &mut HashMap<String, TextureHandle>,
    media_base: &std::path::Path,
) -> bool {
    let mut selected = false;
    ui.horizontal(|ui| {
        avatar_ui_preview(ui, person, cache, media_base, 27.0);
        icon(ui, icon_bytes, icon_id);
        selected = ui.selectable_label(false, person.display_name()).clicked();
    });
    selected
}

/// Suchfeld + Vorschläge der offenen Beziehungskategorie.
pub fn suggestions(app: &mut MiniGramps, ui: &mut egui::Ui, kind: RelationKind, selected_id: &str) {
    if kind == RelationKind::Child {
        let partners: Vec<_> = app
            .data
            .partners_of(selected_id)
            .into_iter()
            .cloned()
            .collect();
        ui.horizontal(|ui| {
            ui.label("Elternteil");
            let selected_text = app
                .pending_child_partner
                .as_deref()
                .and_then(|id| partners.iter().find(|person| person.id == id))
                .map(|person| person.display_name())
                .unwrap_or_else(|| "Ohne Partner".into());
            egui::ComboBox::from_id_salt("child-partner-inline")
                .selected_text(selected_text)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut app.pending_child_partner, None, "Ohne Partner");
                    for partner in &partners {
                        ui.selectable_value(
                            &mut app.pending_child_partner,
                            Some(partner.id.clone()),
                            partner.display_name(),
                        );
                    }
                });
            ui.separator();
            ui.label("Beziehung");
            egui::ComboBox::from_id_salt("child-relation-inline")
                .selected_text(app.pending_child_relation.label())
                .show_ui(ui, |ui| {
                    for (relation, label) in [
                        (ChildRelation::Birth, "Leiblich"),
                        (ChildRelation::Adopted, "Adoptiert"),
                        (ChildRelation::Step, "Stiefkind"),
                        (ChildRelation::Foster, "Pflegekind"),
                    ] {
                        ui.selectable_value(&mut app.pending_child_relation, relation, label);
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Geburt");
            ui.add(
                egui::TextEdit::singleline(&mut app.pending_child_birth)
                    .hint_text("Datum")
                    .desired_width(90.0),
            );
            ui.label("Ort");
            ui.add(
                egui::TextEdit::singleline(&mut app.pending_child_birth_place)
                    .hint_text("Ort"),
            );
        });
    }
    if app.relation_family_name.is_empty() {
        if let Some(p) = app.data.find(selected_id) {
            let mut prefill = String::new();
            if kind == RelationKind::Child {
                if p.gender == Gender::Male {
                    prefill = p.family_name.clone();
                } else {
                    let partners = app.data.partners_of(&p.id);
                    if let Some(partner) = partners.iter().find(|partner| partner.gender == Gender::Male) {
                        prefill = partner.family_name.clone();
                    } else {
                        prefill = p.family_name.clone();
                    }
                }
            } else if kind == RelationKind::Sibling {
                let parents = app.data.parents_of(selected_id);
                if let Some(father) = parents.iter().find(|parent| parent.gender == Gender::Male) {
                    prefill = father.family_name.clone();
                } else if let Some(parent) = parents.first() {
                    prefill = parent.family_name.clone();
                } else {
                    prefill = p.family_name.clone();
                }
            }
            app.relation_family_name = prefill;
        }
    }

    ui.horizontal(|ui| {
        ui.label("Vorname:");
        ui.text_edit_singleline(&mut app.relation_query);
        ui.label("Nachname:");
        ui.text_edit_singleline(&mut app.relation_family_name);
        if ui.button("+").on_hover_text("Person anlegen und direkt verknüpfen").clicked() {
            let new_id = format!("p{}", app.data.people.len() + 1);
            let relation = match kind {
                RelationKind::Partner => "Partner",
                RelationKind::Parent => "Elternteil",
                RelationKind::Child => "Kind",
                RelationKind::Sibling => "Geschwister",
            };
            app.snapshot(format!(
                "{relation} anlegen: {} {}",
                app.relation_query, app.relation_family_name
            ));
            // Neuer Partner erhält per Default das andere Geschlecht.
            let new_gender = match kind {
                RelationKind::Partner => app
                    .data
                    .find(selected_id)
                    .map(|person| match person.gender {
                        Gender::Male => Gender::Female,
                        Gender::Female => Gender::Male,
                        Gender::Unknown => Gender::Unknown,
                    })
                    .unwrap_or(Gender::Unknown),
                _ => Gender::Unknown,
            };
            app.data.people.push(person(
                &new_id,
                &app.relation_query,
                &app.relation_family_name,
                &app.pending_child_birth,
                new_gender,
            ));
            if let Some(new_person) = app.data.people.last_mut() {
                new_person.birth_place = app.pending_child_birth_place.clone();
                new_person.ensure_standard_events();
            }
            match kind {
                RelationKind::Partner => {
                    app.data.link_partner(selected_id, &new_id);
                    app.status = format!("Partner angelegt: {} {}", app.relation_query, app.relation_family_name);
                }
                RelationKind::Parent => {
                    app.data.link_child(&new_id, selected_id);
                    app.status = format!("Elternteil angelegt: {} {}", app.relation_query, app.relation_family_name);
                }
                RelationKind::Child => {
                    app.data.link_child_to(
                        Some(selected_id),
                        app.pending_child_partner.as_deref(),
                        &new_id,
                        app.pending_child_relation,
                    );
                    app.status = format!("Kind angelegt: {} {}", app.relation_query, app.relation_family_name);
                }
                RelationKind::Sibling => {
                    let parent_id = app
                        .data
                        .parents_of(selected_id)
                        .first()
                        .map(|parent| parent.id.clone());
                    if let Some(parent_id) = parent_id {
                        app.data.link_child(&parent_id, &new_id);
                    }
                    app.status = format!("Geschwister angelegt: {} {}", app.relation_query, app.relation_family_name);
                }
            }
            app.relation_picker = None;
            app.relation_query.clear();
            app.relation_family_name.clear();
            app.pending_child_birth.clear();
            app.pending_child_birth_place.clear();
            app.log(format!("Neue Person angelegt: {new_id}"));
        }
    });

    let needle = app.relation_query.to_lowercase();
    if !needle.is_empty() {
        let suggestions: Vec<_> = app
            .data
            .people
            .iter()
            .filter(|person| {
                person.id != selected_id && person.display_name().to_lowercase().contains(&needle)
            })
            .take(5)
            .cloned()
            .collect();
        if !suggestions.is_empty() {
            ui.horizontal_wrapped(|ui| {
            ui.label("Verknüpfen:");
            for candidate in suggestions {
                if ui.small_button(candidate.display_name()).clicked() {
                    let relation = match kind {
                        RelationKind::Partner => "Partner",
                        RelationKind::Parent => "Elternteil",
                        RelationKind::Child => "Kind",
                        RelationKind::Sibling => "Geschwister",
                    };
                    app.snapshot(format!(
                        "{relation} verknüpfen: {}",
                        candidate.display_name()
                    ));
                    match kind {
                        RelationKind::Partner => app.data.link_partner(selected_id, &candidate.id),
                        RelationKind::Parent => app.data.link_child(&candidate.id, selected_id),
                        RelationKind::Child => {
                            app.data.link_child_to(
                                Some(selected_id),
                                app.pending_child_partner.as_deref(),
                                &candidate.id,
                                app.pending_child_relation,
                            );
                        }
                        RelationKind::Sibling => {
                            let parent_id = app
                                .data
                                .parents_of(selected_id)
                                .first()
                                .map(|parent| parent.id.clone());
                            if let Some(parent_id) = parent_id {
                                app.data.link_child(&parent_id, &candidate.id);
                            }
                        }
                    }
                    app.relation_picker = None;
                    app.relation_query.clear();
                    app.relation_family_name.clear();
                }
            }
            });
        }
    }
}

/// Profil-Zeile (Label + Wert).
pub fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(
        egui::RichText::new(label)
            .small()
            .color(crate::ui::panels::dim_text(ui)),
    );
    ui.label(if value.is_empty() { "-" } else { value });
    ui.add_space(7.0);
}

/// Dropdown zur Auswahl der Ereignisart (inkl. „Sonstiges" mit Freitext).
pub fn event_kind_combo(
    ui: &mut egui::Ui,
    kind: &mut crate::model::EventKind,
    id_salt: impl std::hash::Hash,
    width: f32,
) {
    use crate::model::EventKind;
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(kind.label())
        .width(width)
        .show_ui(ui, |ui| {
            for variant in EventKind::all_variants() {
                if ui
                    .selectable_label(EventKind::eq(kind, variant), variant.label())
                    .clicked()
                {
                    *kind = variant.clone();
                }
            }
            let is_custom = matches!(kind, EventKind::Custom(_));
            if ui.selectable_label(is_custom, "Sonstiges").clicked() && !is_custom {
                *kind = EventKind::Custom(String::new());
            }
        });
}

/// Ereignis-Editor (Gramps-nah): **Geburt** und **Tod** sind
/// Standard-Ereignisse (immer vorhanden, Art fest, nicht löschbar), weitere
/// Ereignisse sind frei wählbar. Je Ereignis: Typ als Dropdown, Datum und Ort
/// direkt als Textfelder. Danach werden die Kurzfelder synchronisiert.
pub fn events_editor(ui: &mut egui::Ui, person: &mut Person) {
    use crate::model::{Event, EventKind};
    person.ensure_standard_events();
    let mut remove: Option<usize> = None;
    for index in 0..person.events.len() {
        let event = &mut person.events[index];
        let standard = event.kind == EventKind::Birth || event.kind == EventKind::Death;
        // Titelzeile: Ereignisart als (ggf. deaktiviertes) Dropdown.
        // Titelzeile: Ereignisart als (ggf. deaktiviertes) Dropdown über die
        // volle Breite; bei entfernbaren Ereignissen bleibt Platz für das
        // Entfernen-Icon.
        let combo_width = (ui.available_width() - if standard { 0.0 } else { 28.0 }).max(80.0);
        ui.horizontal(|ui| {
            if standard {
                // Standard-Ereignisse sehen wie ein (deaktiviertes) Dropdown aus.
                let mut fixed = event.kind.clone();
                ui.add_enabled_ui(false, |ui| {
                    event_kind_combo(ui, &mut fixed, ("event-kind-fixed", index), combo_width);
                });
            } else {
                event_kind_combo(ui, &mut event.kind, ("event-kind", index), combo_width);
            }
            if !standard {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_only_button(ui, ICON_CLOSE, "event-remove")
                        .on_hover_text("Ereignis entfernen")
                        .clicked()
                    {
                        remove = Some(index);
                    }
                });
            }
        });
        // Datenzeile: Datum und Ort.
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut event.date)
                    .hint_text("Datum")
                    .desired_width(90.0),
            );
            ui.add(
                egui::TextEdit::singleline(&mut event.place)
                    .hint_text("Ort")
                    .desired_width(ui.available_width()),
            );
        });
        if let EventKind::Custom(text) = &mut event.kind {
            ui.add(
                egui::TextEdit::singleline(text)
                    .hint_text("Eigene Ereignisart")
                    .desired_width(160.0),
            );
        }
        ui.add_space(2.0);
    }
    if let Some(index) = remove {
        person.events.remove(index);
    }
    let width = ui.available_width();
    if ui
        .add_sized([width, 20.0], egui::Button::new("+ Ereignis"))
        .clicked()
    {
        person.events.push(Event::new(EventKind::Residence));
    }
    person.sync_standard_fields();
}

/// Optionen unter einer aufgeklappten Beziehungszeile (Beziehungseditor):
/// Beziehungsart umschalten — Partner: Verheiratet/Partnerschaft, Kinder:
/// leiblich/adoptiert/Stiefkind/Pflegekind, Geschwister: Art über die
/// gemeinsame Familie, Eltern: Ehe der Eltern.
pub fn relation_options(
    app: &mut MiniGramps,
    ui: &mut egui::Ui,
    kind: RelationKind,
    person_id: &str,
    relative_id: &str,
) {
    ui.indent(format!("relation-editor-{relative_id}"), |ui| {
        match kind {
            RelationKind::Partner => {
                // Art der Partnerschaft als Auswahlbox (Dropdown).
                let current = app.data.partner_relation_of(person_id, relative_id);
                let mut selected = current;
                egui::ComboBox::from_id_salt(("partner-rel", relative_id))
                    .selected_text(current.label())
                    .show_ui(ui, |ui| {
                        for relation in [
                            crate::model::PartnerRelation::Unknown,
                            crate::model::PartnerRelation::Married,
                            crate::model::PartnerRelation::Divorced,
                            crate::model::PartnerRelation::Partnered,
                        ] {
                            ui.selectable_value(&mut selected, relation, relation.label());
                        }
                    });
                if selected != current {
                    let name = app
                        .data
                        .find(relative_id)
                        .map(|person| person.display_name())
                        .unwrap_or_else(|| relative_id.to_string());
                    app.snapshot(format!("Partnerbeziehung ändern: {name}"));
                    app.data
                        .set_partner_relation(person_id, relative_id, selected);
                }
            }
            RelationKind::Parent => {
                // Beziehung zur eigenen Herkunft als Dropdown.
                let current = app.data.relation_of_child(relative_id, person_id);
                let mut selected = current;
                egui::ComboBox::from_id_salt(("parent-rel", relative_id))
                    .selected_text(current.label())
                    .show_ui(ui, |ui| {
                        for relation in [
                            ChildRelation::Birth,
                            ChildRelation::Adopted,
                            ChildRelation::Step,
                            ChildRelation::Foster,
                        ] {
                            ui.selectable_value(&mut selected, relation, relation.label());
                        }
                    });
                if selected != current {
                    let name = app
                        .data
                        .find(relative_id)
                        .map(|person| person.display_name())
                        .unwrap_or_else(|| relative_id.to_string());
                    app.snapshot(format!("Elternbeziehung ändern: {name}"));
                    app.data
                        .set_child_relation(relative_id, person_id, selected);
                }
            }
            RelationKind::Child => {
                // Beziehung des Kindes als Dropdown.
                let current = app.data.relation_of_child(person_id, relative_id);
                let mut selected = current;
                egui::ComboBox::from_id_salt(("child-rel", relative_id))
                    .selected_text(current.label())
                    .show_ui(ui, |ui| {
                        for relation in [
                            ChildRelation::Birth,
                            ChildRelation::Adopted,
                            ChildRelation::Step,
                            ChildRelation::Foster,
                        ] {
                            ui.selectable_value(&mut selected, relation, relation.label());
                        }
                    });
                if selected != current {
                    let name = app
                        .data
                        .find(relative_id)
                        .map(|person| person.display_name())
                        .unwrap_or_else(|| relative_id.to_string());
                    app.snapshot(format!("Kindbeziehung ändern: {name}"));
                    app.data
                        .set_child_relation(person_id, relative_id, selected);
                }
                // Geburt und Ort des Kindes direkt pflegbar.
                if let Some(child) = app
                    .data
                    .people
                    .iter_mut()
                    .find(|person| person.id == relative_id)
                {
                    ui.horizontal(|ui| {
                        ui.label("Geburt");
                        ui.add(
                            egui::TextEdit::singleline(&mut child.birth)
                                .hint_text("Datum")
                                .desired_width(90.0),
                        );
                        ui.label("Ort");
                        ui.add(
                            egui::TextEdit::singleline(&mut child.birth_place).hint_text("Ort"),
                        );
                    });
                }
            }
            RelationKind::Sibling => {
                // Geschwister haben keine eigene Beziehungsart — die Zeile
                // ist im Bearbeitungsmodus nicht aufklappbar.
            }
        }
    });
}

/// Datum und Ort kombinieren ("-" wenn beides leer).
pub fn dated_place(date: &str, place: &str) -> String {
    match (date.is_empty(), place.is_empty()) {
        (true, true) => "-".into(),
        (false, true) => date.into(),
        (true, false) => place.into(),
        (false, false) => format!("{date} · {place}"),
    }
}

pub fn gender_symbol(g: Gender) -> &'static str {
    match g {
        Gender::Male => "♂",
        Gender::Female => "♀",
        Gender::Unknown => "○",
    }
}

pub fn gender_label(g: Gender) -> &'static str {
    match g {
        Gender::Male => "Männlich",
        Gender::Female => "Weiblich",
        Gender::Unknown => "Nicht angegeben",
    }
}
