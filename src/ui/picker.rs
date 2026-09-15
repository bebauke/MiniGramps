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
use crate::model::{Certainty, ChildRelation, Event, EventKind, Gender, Person, SourceEntry, person};
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

/// Text auf höchstens `max_chars` Zeichen kürzen („…" als letztes Zeichen).
pub(crate) fn ellipsize(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut result: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    result.push('…');
    result
}

/// Platz für einen nachfolgenden Icon-Knopf (z. B. Entfernen), den die
/// Namensbox freihalten muss, damit die Zeile nicht übersteht.
pub(crate) const TRAILING_ICON_RESERVE: f32 = 36.0;

/// Platz für zwei nachfolgende Icon-Knöpfe (z. B. Menü + Entfernen), den die
/// Namensbox freihalten muss, damit die Zeile nicht übersteht.
pub(crate) const TRAILING_TWO_ICON_RESERVE: f32 = 64.0;

/// Klickbare, linksbündige Namenszeile mit FESTEM Rechteck (Flattersatz,
/// kein Blocksatz): Sie belegt exakt `verfügbare Breite minus reserve` und
/// meldet dem Layout nie Wunschbreiten zurück — lange Namen können
/// größenverstellbare Panels dadurch nicht aufziehen
/// (Rückkopplungsschleife). Gekürzt wird ohne Textvermessung per
/// Zeichenbudget aus der tatsächlichen Breite.
pub fn truncated_person_button(
    ui: &mut egui::Ui,
    text: &str,
    hover: &str,
    active: bool,
    reserve: f32,
) -> egui::Response {
    let height = ui.spacing().interact_size.y;
    let width = (ui.available_width() - reserve).max(40.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::Vec2::new(width, height), egui::Sense::click());
    let response = response.on_hover_text(hover);
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, active);
        if active || response.hovered() {
            ui.painter()
                .rect_filled(rect, visuals.corner_radius, visuals.bg_fill);
        }
        let budget = ((rect.width() / 6.5) as usize).max(8);
        ui.painter().text(
            rect.left_center() + egui::Vec2::new(6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            ellipsize(text, budget),
            egui::FontId::proportional(13.0),
            visuals.text_color(),
        );
    }
    response
}

/// Text linksbündig in festem Rechteck, ggf. über mehrere Zeilen umbrechend.
/// Keine Wunschbreiten ans Layout (kein Aufziehen von Panels).
pub fn left_label_wrapped(ui: &mut egui::Ui, text: &str, size: f32, color: egui::Color32) {
    let width = ui.available_width();
    let galley = ui.painter().layout(
        text.to_owned(),
        egui::FontId::proportional(size),
        color,
        width,
    );
    let height = galley.size().y.max(1.0);
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::new(width, height), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().galley(rect.min, galley, color);
    }
}

/// Einzeiliger Text linksbündig in festem Rechteck, bei Bedarf mit „…" am
/// Ende gekürzt (Tooltip nur dann).
pub fn left_label_single(ui: &mut egui::Ui, text: &str, size: f32, color: egui::Color32) {
    let width = ui.available_width();
    let budget = ((width / (size * 0.5)) as usize).max(8);
    let shown = ellipsize(text, budget);
    let font = egui::FontId::proportional(size);
    let height = ui
        .painter()
        .layout(shown.clone(), font.clone(), color, f32::INFINITY)
        .size()
        .y
        .max(1.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::Vec2::new(width, height), egui::Sense::hover());
    let response = if shown == text {
        response
    } else {
        response.on_hover_text(text)
    };
    let _ = response;
    if ui.is_rect_visible(rect) {
        ui.painter().text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            shown,
            font,
            color,
        );
    }
}

/// Beziehungszeile: Avatar + Icon + klickbarer Name (öffnet zur Ansicht).
/// `trailing` reserviert Platz für einen nachfolgenden Knopf (z. B.
/// Entfernen im Bearbeitungsmodus, sonst 0).
pub fn relationship_row(
    ui: &mut egui::Ui,
    icon_bytes: &'static [u8],
    icon_id: &str,
    person: &Person,
    cache: &mut HashMap<String, TextureHandle>,
    media_base: &std::path::Path,
    trailing: f32,
) -> bool {
    let mut selected = false;
    ui.horizontal(|ui| {
        avatar_ui_preview(ui, person, cache, media_base, 27.0, egui::Vec2::ZERO);
        icon(ui, icon_bytes, icon_id);
        let name = person.display_name();
        selected = truncated_person_button(ui, &name, &name, false, trailing).clicked();
    });
    selected
}

/// Suchfeld + Vorschläge der offenen Beziehungskategorie.
pub fn suggestions(app: &mut MiniGramps, ui: &mut egui::Ui, kind: RelationKind, selected_id: &str) {
    // Explizite Geschlechtswahl gilt nur für die aktuelle Kategorie +
    // Bezugsperson (sonst zurück auf automatisch).
    if app.new_person_gender_for != Some((kind, selected_id.to_string())) {
        app.new_person_gender = None;
        app.new_person_gender_for = Some((kind, selected_id.to_string()));
    }
    // Neuer Partner erhält per Default das andere Geschlecht.
    let auto_gender = match kind {
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
                .map(|person| ellipsize(&person.display_name(), 26))
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

    // Zwei Zeilen statt einer: So bleibt das "+" immer sichtbar, egal wie
    // schmal die Leiste ist.
    ui.horizontal(|ui| {
        ui.label("Geschlecht:");
        let mut gender = app.new_person_gender.unwrap_or(auto_gender);
        let before = gender;
        ui.radio_value(&mut gender, Gender::Female, "W");
        ui.radio_value(&mut gender, Gender::Male, "M");
        ui.radio_value(&mut gender, Gender::Unknown, "?");
        if gender != before {
            app.new_person_gender = Some(gender);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Vorname:");
        ui.add(
            egui::TextEdit::singleline(&mut app.relation_query)
                .desired_width(ui.available_width()),
        );
    });
    ui.horizontal(|ui| {
        ui.label("Nachname:");
        // Platz für das "+" freihalten (feste Reserve statt Wunschbreite).
        let field = (ui.available_width() - 34.0).max(60.0);
        ui.add(
            egui::TextEdit::singleline(&mut app.relation_family_name).desired_width(field),
        );
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
            let new_gender = app.new_person_gender.unwrap_or(auto_gender);
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
            app.new_person_gender = None;
            app.new_person_gender_for = None;
            app.pending_child_birth.clear();
            app.pending_child_birth_place.clear();
            app.log(format!("Neue Person angelegt: {new_id}"));
        }
    });

    let needle = app.relation_query.to_lowercase();
    if !needle.is_empty() {
        // Token-Suche über Vor- + Nachname (+ Legacy): jedes Token muss
        // vorkommen (Reihenfolge egal — Nachname zählt mit).
        let tokens: Vec<&str> = needle.split_whitespace().collect();
        let suggestions: Vec<_> = app
            .data
            .people
            .iter()
            .filter(|person| {
                person.id != selected_id && {
                    let hay = format!(
                        "{} {} {}",
                        person.given_name, person.family_name, person.name
                    )
                    .to_lowercase();
                    tokens.iter().all(|token| hay.contains(token))
                }
            })
            .take(5)
            .cloned()
            .collect();
        if !suggestions.is_empty() {
            ui.horizontal_wrapped(|ui| {
            ui.label("Verknüpfen:");
            for candidate in suggestions {
                let year = crate::model::birth_year(&candidate.birth);
                let name = if year.is_empty() {
                    candidate.display_name()
                } else {
                    format!("{} ({year})", candidate.display_name())
                };
                if ui
                    .small_button(ellipsize(&name, 30))
                    .on_hover_text(format!(
                        "{} — Klick für Übersicht mit Verknüpfen",
                        candidate.display_name()
                    ))
                    .clicked()
                {
                    // Klick öffnet die Personenübersicht (Verknüpfen dort).
                    app.link_detail =
                        Some((selected_id.to_string(), candidate.id.clone(), kind));
                }
            }
            });
        }
    }
}

/// Vorschlag als bestehende Person verknüpfen (aus dem Übersichtsmodal):
/// Snapshot + Art-spezifisches Verknüpfen + Picker zurücksetzen.
pub fn link_existing(
    app: &mut MiniGramps,
    kind: RelationKind,
    selected_id: &str,
    candidate: &Person,
) {
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

/// Personenübersicht zum Verknüpfungs-Vorschlag (Klick in der Trefferliste):
/// Beziehungen der Person plus Verknüpfen-Button.
pub fn show_link_detail(app: &mut MiniGramps, ctx: &egui::Context) {
    let Some((selected_id, candidate_id, kind)) = app.link_detail.clone() else {
        return;
    };
    let Some(person) = app.data.find(&candidate_id).cloned() else {
        app.link_detail = None;
        return;
    };
    // Beziehungen einsammeln (owned, damit unten `&mut app` geht).
    let relations = crate::ui::tree::TreeRelations::new(&app.data);
    let names_of = |people: &[&Person]| {
        people
            .iter()
            .map(|person| person.display_name())
            .collect::<Vec<_>>()
    };
    let parents = names_of(relations.parents_of(&candidate_id));
    let mut sibling_ids: Vec<String> = Vec::new();
    for parent in relations.parents_of(&candidate_id) {
        for child in relations.children_of(&parent.id) {
            if child.id != candidate_id && !sibling_ids.contains(&child.id) {
                sibling_ids.push(child.id.clone());
            }
        }
    }
    let siblings: Vec<String> = sibling_ids
        .iter()
        .filter_map(|id| app.data.find(id))
        .map(|sibling| sibling.display_name())
        .collect();
    let partners = names_of(relations.partners_of(&candidate_id));
    let children = names_of(relations.children_of(&candidate_id));
    let born = if person.birth.trim().is_empty() {
        "–".to_string()
    } else {
        person.birth.clone()
    };
    let died = if person.death.trim().is_empty() {
        "–".to_string()
    } else {
        person.death.clone()
    };
    let kind_label = match kind {
        RelationKind::Partner => "Partner",
        RelationKind::Parent => "Elternteil",
        RelationKind::Child => "Kind",
        RelationKind::Sibling => "Geschwister",
    };
    let mut open = true;
    let mut link_now = false;
    egui::Window::new(crate::ui::window_title("Personenübersicht"))
        .id(egui::Id::new("link-detail-v1"))
        .open(&mut open)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(380.0)
        .max_height(ctx.content_rect().height() * 0.8)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(person.display_name()).strong());
            ui.small(format!(
                "{} · {born} – {died}",
                gender_label(person.gender)
            ));
            ui.separator();
            for (title, entries) in [
                ("Eltern", &parents),
                ("Geschwister", &siblings),
                ("Partner", &partners),
                ("Kinder", &children),
            ] {
                ui.label(egui::RichText::new(title).strong());
                if entries.is_empty() {
                    ui.small("–");
                }
                for entry in entries {
                    ui.small(entry);
                }
            }
            ui.separator();
            if ui
                .button("Verknüpfen")
                .on_hover_text(format!("{kind_label} verknüpfen"))
                .clicked()
            {
                link_now = true;
            }
        });
    if link_now {
        link_existing(app, kind, &selected_id, &person);
        app.link_detail = None;
    }
    if !open {
        app.link_detail = None;
    }
}
pub fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(
        egui::RichText::new(label)
            .small()
            .color(crate::ui::panels::dim_text(ui)),
    );
    let value = if value.is_empty() { "-" } else { value };
    left_label_single(ui, value, 13.0, ui.visuals().text_color());
    ui.add_space(7.0);
}

/// Warnfarbe für Handlungsbedarf (Sicherheits-Warnstufe).
pub fn warn_color() -> egui::Color32 {
    egui::Color32::from_rgb(235, 175, 60)
}

/// Profil-Zeile mit Warntönung des Werts (Sicherheit unter Warnstufe).
pub fn info_row_warn(ui: &mut egui::Ui, label: &str, value: &str, warn: bool) {
    ui.label(
        egui::RichText::new(label)
            .small()
            .color(crate::ui::panels::dim_text(ui)),
    );
    let value = if value.is_empty() { "-" } else { value };
    let color = if warn {
        warn_color()
    } else {
        ui.visuals().text_color()
    };
    left_label_single(ui, value, 13.0, color);
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
/// Kontextmenü je Eintrag (Ereignis, Beziehung, Name …): Notiz ein/aus,
/// Quelle hinzufügen, hinterlegte Quellenmedien öffnen, Sicherheit wählen.
/// Gibt (Warnung, zu öffnendes Medium) zurück: Warnung bei Beglaubigt ohne
/// Quelle (Aufrufer meldet per Status), Medium öffnet der Aufrufer.
pub fn entry_menu(
    ui: &mut egui::Ui,
    notes: &mut Option<String>,
    sources: &mut Vec<SourceEntry>,
    certainty: &mut Certainty,
) -> (bool, Option<String>) {
    let mut warn = false;
    let mut open_media: Option<String> = None;
    ui.label(egui::RichText::new("Eintrag").strong());
    ui.separator();
    let note_label = if notes.is_some() {
        "Notiz entfernen"
    } else {
        "Notiz hinzufügen"
    };
    if ui.selectable_label(false, note_label).clicked() {
        if notes.is_some() {
            *notes = None;
        } else {
            *notes = Some(String::new());
        }
        ui.close();
    }
    if ui.selectable_label(false, "+ Quelle").clicked() {
        sources.push(SourceEntry {
            title: String::new(),
            detail: String::new(),
            media: None,
        });
        ui.close();
    }
    // Hinterlegte Quellenmedien (separat von der Galerie) hier öffnen.
    for source in sources.iter() {
        if let Some(media) = source.media.as_deref() {
            if media.trim().is_empty() {
                continue;
            }
            let name = std::path::Path::new(media)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(media);
            if ui
                .selectable_label(false, format!("Medium öffnen: {name}"))
                .clicked()
            {
                open_media = Some(media.to_string());
                ui.close();
            }
        }
    }
    ui.separator();
    ui.label(egui::RichText::new("Sicherheit").strong());
    for level in [
        Certainty::Unset,
        Certainty::Oral,
        Certainty::Document,
        Certainty::Certified,
    ] {
        if ui
            .selectable_label(*certainty == level, level.label())
            .clicked()
        {
            *certainty = level;
            if level == Certainty::Certified && sources.iter().all(|source| source.is_empty()) {
                warn = true;
            }
            ui.close();
        }
    }
    (warn, open_media)
}

/// Notiz-/Quellen-Editor unter einem Eintrag (nur Vorhandenes rendern;
/// `None` blendet den jeweiligen Teil aus — so lassen sich Map-gestützte
/// Einträge (Namensteile) ohne Adapter anbinden). `media_base` fürs Anhängen
/// von Quellenmedien (separat von der Galerie).
pub fn entry_extras_editor(
    ui: &mut egui::Ui,
    notes: Option<&mut String>,
    sources: Option<&mut Vec<SourceEntry>>,
    media_base: &std::path::Path,
) {
    if let Some(note) = notes {
        ui.horizontal(|ui| {
            ui.label("Notiz");
            ui.add(
                egui::TextEdit::multiline(note)
                    .desired_rows(2)
                    .desired_width(ui.available_width()),
            );
        });
    }
    if let Some(sources) = sources {
        if sources.is_empty() {
            return;
        }
        let mut remove_source: Option<usize> = None;
        for (index, source) in sources.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(if index == 0 { "Quelle" } else { "" });
                ui.add(
                    egui::TextEdit::singleline(&mut source.title)
                        .hint_text("Titel")
                        .desired_width(100.0),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut source.detail)
                        .hint_text("Detail/Seite")
                        .desired_width(ui.available_width()),
                );
            });
            ui.horizontal(|ui| {
                ui.label("");
                match source.media.clone() {
                    None => {
                        if ui
                            .small_button("+ Medium")
                            .on_hover_text("Mediendatei als Beleg anhängen (separat von der Galerie)")
                            .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new().pick_file() {
                                if let Some(relative) = crate::media::import_media_file_async(
                                    ui.ctx(),
                                    media_base,
                                    &path,
                                ) {
                                    source.media = Some(relative);
                                }
                            }
                        }
                    }
                    Some(media) => {
                        let name = std::path::Path::new(&media)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or(&media);
                        ui.small(format!("Medium: {name}"));
                        if ui
                            .small_button("×")
                            .on_hover_text("Medium lösen (Datei bleibt erhalten)")
                            .clicked()
                        {
                            source.media = None;
                        }
                    }
                }
                if ui
                    .small_button("×")
                    .on_hover_text("Quelle entfernen")
                    .clicked()
                {
                    remove_source = Some(index);
                }
            });
        }
        if let Some(index) = remove_source {
            sources.remove(index);
        }
    }
}

/// Zusatzinfos unter einem Eintrag (lesen + bearbeiten): Notiz, Quellen,
/// Sicherheit (nur Gesetztes zeigen).
pub fn entry_extras_view(
    ui: &mut egui::Ui,
    notes: Option<&str>,
    sources: &[SourceEntry],
    certainty: Certainty,
) {
    if let Some(note) = notes {
        if !note.trim().is_empty() {
            ui.small(format!("Notiz: {}", note.trim()));
        }
    }
    for source in sources.iter().filter(|source| !source.is_empty()) {
        let mut text = format!("Quelle: {}", source.title.trim());
        if !source.detail.trim().is_empty() {
            text += &format!(" — {}", source.detail.trim());
        }
        if let Some(media) = source.media.as_deref() {
            if !media.trim().is_empty() {
                text += &format!(" [Medium: {media}]");
            }
        }
        ui.small(text);
    }
    if certainty != Certainty::Unset {
        ui.small(format!("Sicherheit: {}", certainty.label()));
    }
}

/// Ereigniseditor (Entwurf): Gibt (Warnung, zu öffnendes Medium) zurück —
/// Warnung bei Beglaubigt ohne Quelle, Medium öffnet der Aufrufer.
pub fn events_editor(
    ui: &mut egui::Ui,
    person: &mut Person,
    media_base: &std::path::Path,
) -> (bool, Option<String>) {
    use crate::model::{Event, EventKind};
    person.ensure_standard_events();
    let mut warned = false;
    let mut open_media: Option<String> = None;
    let mut remove: Option<usize> = None;
    for index in 0..person.events.len() {
        let event = &mut person.events[index];
        let standard = event.kind == EventKind::Birth || event.kind == EventKind::Death;
        // Titelzeile: Ereignisart als (ggf. deaktiviertes) Dropdown über die
        // volle Breite; bei entfernbaren Ereignissen bleibt Platz für das
        // Entfernen-Icon plus Menü-Button (Eintrags-Menü: Notiz, Quellen,
        // Sicherheit — Links- wie Rechtsklick; reine Zeilen-Responses hätten
        // keinen Click-Sense für Rechtsklick).
        let extra = if standard { 24.0 } else { 52.0 };
        let combo_width = (ui.available_width() - extra).max(80.0);
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !standard {
                    if icon_only_button(ui, ICON_CLOSE, "event-remove")
                        .on_hover_text("Ereignis entfernen")
                        .clicked()
                    {
                        remove = Some(index);
                    }
                }
                let menu_button = ui
                    .small_button("…")
                    .on_hover_text("Eintrags-Menü: Notiz, Quellen, Sicherheit");
                egui::Popup::menu(&menu_button).show(|ui| {
                    let (warn, open) = entry_menu(
                        ui,
                        &mut event.notes,
                        &mut event.sources,
                        &mut event.certainty,
                    );
                    if warn {
                        warned = true;
                    }
                    if open.is_some() {
                        open_media = open;
                    }
                });
                egui::Popup::context_menu(&menu_button).show(|ui| {
                    let (warn, open) = entry_menu(
                        ui,
                        &mut event.notes,
                        &mut event.sources,
                        &mut event.certainty,
                    );
                    if warn {
                        warned = true;
                    }
                    if open.is_some() {
                        open_media = open;
                    }
                });
            });
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
        // Zusatzinfos darunter (Notiz, Quellen).
        entry_extras_editor(
            ui,
            event.notes.as_mut(),
            Some(&mut event.sources),
            media_base,
        );
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
    (warned, open_media)
}

fn is_relation_event_kind(kind: &EventKind) -> bool {
    matches!(kind, EventKind::Marriage | EventKind::Divorce)
}

fn same_relation_event(event: &Event, kind: &EventKind, date: &str, place: &str) -> bool {
    event.kind == *kind && event.date == date && event.place == place
}

fn relation_event_keys(
    app: &MiniGramps,
    person_id: &str,
    relative_id: &str,
) -> Vec<(EventKind, String, String)> {
    let mut keys: Vec<(EventKind, String, String)> = Vec::new();
    for pid in [person_id, relative_id] {
        if let Some(person) = app.data.find(pid) {
            for event in &person.events {
                if !is_relation_event_kind(&event.kind) {
                    continue;
                }
                let key = (event.kind.clone(), event.date.clone(), event.place.clone());
                if !keys
                    .iter()
                    .any(|(kind, date, place)| kind == &key.0 && date == &key.1 && place == &key.2)
                {
                    keys.push(key);
                }
            }
        }
    }
    keys
}

fn find_relation_event(
    app: &MiniGramps,
    person_id: &str,
    relative_id: &str,
    key: &(EventKind, String, String),
) -> Option<Event> {
    for pid in [person_id, relative_id] {
        if let Some(person) = app.data.find(pid) {
            if let Some(event) = person
                .events
                .iter()
                .find(|event| same_relation_event(event, &key.0, &key.1, &key.2))
            {
                return Some(event.clone());
            }
        }
    }
    None
}

fn upsert_relation_event(
    app: &mut MiniGramps,
    pid: &str,
    old_key: &(EventKind, String, String),
    next: &Event,
) {
    if let Some(person) = app.data.people.iter_mut().find(|person| person.id == pid) {
        if let Some(event) = person
            .events
            .iter_mut()
            .find(|event| same_relation_event(event, &old_key.0, &old_key.1, &old_key.2))
        {
            *event = next.clone();
        } else {
            person.events.push(next.clone());
        }
    }
}

fn remove_relation_event(app: &mut MiniGramps, pid: &str, key: &(EventKind, String, String)) {
    if let Some(person) = app.data.people.iter_mut().find(|person| person.id == pid) {
        person
            .events
            .retain(|event| !same_relation_event(event, &key.0, &key.1, &key.2));
    }
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
                // Beziehungsereignisse (Gramps: Ereignisse an der Beziehung):
                // Heirat/Scheidung einmal anzeigen und auf beide Partner-
                // Events spiegeln.
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("BEZIEHUNGSEREIGNISSE")
                        .small()
                        .color(crate::ui::panels::dim_text(ui)),
                );
                let keys = relation_event_keys(app, person_id, relative_id);
                let mut remove_key: Option<(EventKind, String, String)> = None;
                for (index, key) in keys.iter().enumerate() {
                    let Some(mut event) = find_relation_event(app, person_id, relative_id, key) else {
                        continue;
                    };
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt(("rel-event-kind", relative_id, index))
                            .selected_text(event.kind.label())
                            .width(110.0)
                            .show_ui(ui, |ui| {
                                for kind in [EventKind::Marriage, EventKind::Divorce] {
                                    ui.selectable_value(&mut event.kind, kind.clone(), kind.label());
                                }
                            });
                        ui.add(
                            egui::TextEdit::singleline(&mut event.date)
                                .hint_text("Datum")
                                .desired_width(92.0),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut event.place)
                                .hint_text("Ort")
                                .desired_width(ui.available_width().max(80.0)),
                        );
                        let menu_button = ui
                            .small_button("…")
                            .on_hover_text("Eintrags-Menü: Notiz, Quellen, Sicherheit");
                        egui::Popup::menu(&menu_button).show(|ui| {
                            let (warn, open) = entry_menu(
                                ui,
                                &mut event.notes,
                                &mut event.sources,
                                &mut event.certainty,
                            );
                            if warn {
                                app.status = "Achtung: Beglaubigt ohne Quelle!".into();
                            }
                            if let Some(relative) = open {
                                app.open_source_media(&relative);
                            }
                        });
                        egui::Popup::context_menu(&menu_button).show(|ui| {
                            let (warn, open) = entry_menu(
                                ui,
                                &mut event.notes,
                                &mut event.sources,
                                &mut event.certainty,
                            );
                            if warn {
                                app.status = "Achtung: Beglaubigt ohne Quelle!".into();
                            }
                            if let Some(relative) = open {
                                app.open_source_media(&relative);
                            }
                        });
                        if icon_only_button(ui, ICON_CLOSE, "rel-event-remove")
                            .on_hover_text("Beziehungsereignis entfernen")
                            .clicked()
                        {
                            remove_key = Some(key.clone());
                        }
                    });
                    entry_extras_editor(
                        ui,
                        event.notes.as_mut(),
                        Some(&mut event.sources),
                        &app.library,
                    );
                    upsert_relation_event(app, person_id, key, &event);
                    upsert_relation_event(app, relative_id, key, &event);
                }
                if let Some(key) = remove_key {
                    let name = app
                        .data
                        .find(relative_id)
                        .map(|person| person.display_name())
                        .unwrap_or_else(|| relative_id.to_string());
                    app.snapshot(format!("Beziehungsereignis entfernen: {name}"));
                    remove_relation_event(app, person_id, &key);
                    remove_relation_event(app, relative_id, &key);
                }
                if keys.is_empty() {
                    ui.label(
                        egui::RichText::new("Noch keine")
                            .italics()
                            .color(crate::ui::panels::dim_text(ui)),
                    );
                }
                if ui.small_button("+ Beziehungsereignis").clicked() {
                    let name = app
                        .data
                        .find(relative_id)
                        .map(|person| person.display_name())
                        .unwrap_or_else(|| relative_id.to_string());
                    app.snapshot(format!("Beziehungsereignis hinzufügen: {name}"));
                    for pid in [person_id, relative_id] {
                        if let Some(partner) =
                            app.data.people.iter_mut().find(|person| person.id == pid)
                        {
                            partner.events.push(Event::new(EventKind::Marriage));
                        }
                    }
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
