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
use crate::model::{
    Gender, PartnerRelation, RelDiffKind, RelationEventMode, person,
};
use crate::ui::{
    CardLayout, ICON_CHEVRON_LEFT, ICON_CHEVRON_RIGHT, ICON_EXPORT, ICON_EXTERNAL_LINK,
    ICON_TRASH, ICON_UNLINK, MiniGramps, WizardStep, icon_button, icon_only_button,
    panels::palette, window_title,
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
            ui.separator();
            ui.label(
                egui::RichText::new("SCHNELLERFASSUNG")
                    .small()
                    .strong()
                    .color(section),
            );
            ui.label("Geführte Eingabe zur Referenzperson: Abwärts Partner + Kinder, aufwärts pro Knopf Kind + Eltern. Bestehende Personen lassen sich per Suche direkt verknüpfen.");
            if ui.button("Schnellerfassung öffnen…").on_hover_text(
                "Neue Personen ohne Umwege eintippen (Strg+Enter = übernehmen, \
                 Umschalt+Strg+Enter = neuer Partner).",
            ).clicked() {
                app.open_quick_entry();
            }
            ui.separator();
            ui.label(
                egui::RichText::new("SICHERUNG")
                    .small()
                    .strong()
                    .color(section),
            );
            ui.horizontal(|ui| {
                match app.last_backup_label() {
                    Some(label) => {
                        ui.label(format!("Letzte: {label}"));
                        if ui.button("Wiederherstellen").on_hover_text(
                            "Projektordner aus der neuesten Sicherung wiederherstellen \
                             (z. B. nach fehlerhaftem Import).",
                        ).clicked() {
                            app.restore_last_backup();
                        }
                    }
                    None => {
                        ui.label("Noch keine Sicherung (entsteht vor jedem Anhängen).");
                    }
                }
            });
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
                if ui.button("Duplikate finden").on_hover_text(
                    "Im aktuellen Projekt nach Duplikaten suchen (gleicher Vorname, \
                     ähnlicher Nachname, verträgliche Geburt, ähnliche Verwandtschaft \
                     — dort zählen gleiche Verwandten-IDs stark mit).",
                ).clicked() {
                    app.find_project_duplicates();
                }
            });
        });
    app.show_project = app.show_project && open;
}

/// Vergleichstabelle Neu/Ergebnis/Vorhanden mit Radio-Wahl je Name/Geburt/Tod
/// (Review- und Einzel-Merge-Dialog teilen sich Darstellung + Auswahl-Logik).
/// `rows`: (Label, Auswahl-Spalte?, Neu-Text, Vorhanden-Text) in der Ordnung
/// Name, Geburt, Tod, … — Index 0→Name, 1→Geburt, Rest→Tod. Wertspalten mit
/// maximaler Breite (Inhalt bestimmt, Umbruch statt Abschneiden).
fn merge_fields_grid(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    rows: &[(String, bool, String, String)],
    take_name: &mut bool,
    take_birth: &mut bool,
    take_death: &mut bool,
) {
    // Ergebnis-Vorschau: Auswahl ? Neu : Vorhanden; Verknüpfungen vereint.
    fn union_text(first: &str, second: &str) -> String {
        let mut seen: Vec<&str> = Vec::new();
        for token in first
            .split(", ")
            .chain(second.split(", "))
            .map(str::trim)
            .filter(|token| !token.is_empty() && *token != "—" && *token != "?")
        {
            if !seen.contains(&token) {
                seen.push(token);
            }
        }
        if seen.is_empty() {
            "—".to_string()
        } else {
            seen.join(", ")
        }
    }
    egui::Grid::new(id_salt)
        .num_columns(6)
        .striped(true)
        .max_col_width(560.0)
        .show(ui, |ui| {
            ui.label("");
            ui.label(egui::RichText::new("Neu").strong());
            ui.label("");
            ui.label(egui::RichText::new("Ergebnis").strong());
            ui.label(egui::RichText::new("Vorhanden").strong());
            ui.label("");
            ui.end_row();
            for (row_index, (label, is_choice, new_text, old_text)) in rows.iter().enumerate() {
                ui.label(label);
                ui.label(new_text);
                let current = match row_index {
                    0 => *take_name,
                    1 => *take_birth,
                    _ => *take_death,
                };
                if *is_choice {
                    let mut take = current;
                    ui.radio_value(&mut take, true, "");
                    if take != current {
                        match row_index {
                            0 => *take_name = take,
                            1 => *take_birth = take,
                            _ => *take_death = take,
                        }
                    }
                } else {
                    ui.label("");
                }
                let result = match row_index {
                    0 => {
                        if *take_name {
                            new_text.clone()
                        } else {
                            old_text.clone()
                        }
                    }
                    1 => {
                        if *take_birth {
                            new_text.clone()
                        } else {
                            old_text.clone()
                        }
                    }
                    2 => {
                        if *take_death {
                            new_text.clone()
                        } else {
                            old_text.clone()
                        }
                    }
                    _ => union_text(new_text, old_text),
                };
                ui.label(&result);
                ui.label(old_text);
                if *is_choice {
                    let mut take = current;
                    ui.radio_value(&mut take, false, "");
                    if take != current {
                        match row_index {
                            0 => *take_name = take,
                            1 => *take_birth = take,
                            _ => *take_death = take,
                        }
                    }
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
        });
}

/// Duplikat-Review nach angehängtem Import: pro Treffer selektiv
/// zusammenführen (Checkbox) oder endgültig als Kein-Match ablehnen.
pub fn show_merge_review(app: &mut MiniGramps, ctx: &egui::Context) {
    if !app.show_merge_review || app.merge_review.is_none() {
        return;
    }
    let mut open = true;
    // Eigene Fenster-ID (frische Größe statt gemerkter Vollhöhe) plus
    // Max-Höhe: egui persistiert Resize-Zustände, einmal voll hoch
    // gewachsen bliebe der Dialog sonst für immer so.
    egui::Window::new(window_title("Duplikate prüfen"))
        .id(egui::Id::new("merge-review-v3"))
        .max_height(ctx.content_rect().height() * 0.9)
        .open(&mut open)
        .movable(false)
        .resizable(true)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(900.0)
        .show(ctx, |ui| {
            let count = app
                .merge_review
                .as_ref()
                .map(|review| review.candidates.len())
                .unwrap_or(0);
            // Listenhöhen am Bildschirm festmachen, damit der Dialog auf
            // kleinen Bildschirmen nicht die volle Höhe einnimmt.
            let screen_h = ctx.content_rect().height();
            let manual_list_h = (screen_h * 0.14).clamp(80.0, 120.0);
            let candidate_list_h = (screen_h * 0.35).clamp(200.0, 320.0);
            ui.label(format!(
                "{count} mögliche Duplikate — pro Treffer zusammenführen oder ablehnen."
            ));
            ui.horizontal(|ui| {
                ui.label("Häufige Vornamen ab");
                let changed = ui
                    .add(
                        egui::DragValue::new(&mut app.common_given_threshold).range(2..=10),
                    )
                    .on_hover_text(
                        "Vornamen ab so vielen Gleichnamigen je Nachnamengruppe geben \
                         keinen Exakt-Boost (Default 4, z. B. viele Marias). Änderung \
                         baut die Liste neu auf (Auswahl bleibt, Abgelehnte draußen).",
                    )
                    .changed();
                ui.small("Vorkommen (Default 4)");
                if changed {
                    app.common_given_threshold = app.common_given_threshold.clamp(2, 10);
                    app.recompute_merge_review();
                }
            });
            ui.separator();
            // Manuelle Treffer nur beim Import-Anhang (rechte Seite „Neu");
            // beim Projekt-Scan ist alles Bestand.
            let manual_active = app
                .merge_review
                .as_ref()
                .map(|review| !review.fresh_ids.is_empty())
                .unwrap_or(false);
            if manual_active {
            // Manuelle Treffer: oben Name suchen, beide Seiten durchsuchen,
            // je eine Person wählen — auch mehrfach hintereinander.
            ui.label("Manuell zuordnen (Name suchen, links Bestand, rechts Neu wählen; Referenz und offene Spitzen stehen oben):");
            ui.horizontal(|ui| {
                ui.label("Suche");
                let mut query = app
                    .merge_review
                    .as_ref()
                    .map(|review| review.manual_query.clone())
                    .unwrap_or_default();
                if ui
                    .text_edit_singleline(&mut query)
                    .on_hover_text("Sucht in Bestand und Anhang (Vor- und Nachname).")
                    .changed()
                {
                    if let Some(review) = app.merge_review.as_mut() {
                        review.manual_query = query;
                    }
                }
            });
            // Ungültig gewordene Auswahl (z. B. zusammengeführt) aufräumen.
            let keep_gone = app.merge_review.as_ref().and_then(|review| review.manual_keep.clone()).is_some_and(|id| app.data.find(&id).is_none());
            let drop_gone = app.merge_review.as_ref().and_then(|review| review.manual_drop.clone()).is_some_and(|id| app.data.find(&id).is_none());
            if keep_gone || drop_gone {
                if let Some(review) = app.merge_review.as_mut() {
                    if keep_gone {
                        review.manual_keep = None;
                    }
                    if drop_gone {
                        review.manual_drop = None;
                    }
                }
            }
            // Trefferlisten berechnen (nur lesen). Sortierung: Referenz
            // (Wurzel) zuerst, dann offene Spitzen (Vorfahren ohne erfasste
            // Eltern — dort dockt Anhang typischerweise an), dann alphabetisch.
            struct ManualHit {
                id: String,
                label: String,
            }
            const MANUAL_HITS: usize = 8;
            let manual = app.merge_review.as_ref().map(|review| {
                let needle = review.manual_query.trim().to_lowercase();
                let mut existing: Vec<ManualHit> = Vec::new();
                let mut fresh: Vec<ManualHit> = Vec::new();
                let mut existing_total = 0usize;
                let mut fresh_total = 0usize;
                if !needle.is_empty() {
                    let mut people: Vec<(u8, &crate::model::Person)> = app
                        .data
                        .people
                        .iter()
                        .filter(|person| {
                            person.display_name().to_lowercase().contains(&needle)
                        })
                        .map(|person| {
                            let rank = if app.reference.as_deref() == Some(person.id.as_str()) {
                                0
                            } else if app.data.parents_of(&person.id).is_empty() {
                                1
                            } else {
                                2
                            };
                            (rank, person)
                        })
                        .collect();
                    people.sort_by(|left, right| {
                        left.0.cmp(&right.0).then_with(|| {
                            left.1.display_name().cmp(&right.1.display_name())
                        })
                    });
                    for (rank, person) in people {
                        let mut label = if person.birth.trim().is_empty() {
                            person.display_name()
                        } else {
                            format!("{} · {}", person.display_name(), person.birth.trim())
                        };
                        if rank == 0 {
                            label = format!("{label} (Referenz)");
                        } else if rank == 1 {
                            label = format!("{label} (Spitze)");
                        }
                        let hit = ManualHit {
                            id: person.id.clone(),
                            label,
                        };
                        if review.fresh_ids.contains(&person.id) {
                            fresh_total += 1;
                            if fresh.len() < MANUAL_HITS {
                                fresh.push(hit);
                            }
                        } else {
                            existing_total += 1;
                            if existing.len() < MANUAL_HITS {
                                existing.push(hit);
                            }
                        }
                    }
                }
                (
                    existing,
                    fresh,
                    existing_total,
                    fresh_total,
                    review.manual_keep.clone(),
                    review.manual_drop.clone(),
                )
            });
            if let Some((existing, fresh, existing_total, fresh_total, selected_keep, selected_drop)) =
                manual
            {
                // Spaltenbreite fest teilen (Anteil der Dialogbreite), damit die
                // leeren „—"-Spalten in keiner Richtung wachsen können.
                let col_w = ((ui.available_width() - 30.0) / 2.0).clamp(140.0, 380.0);
                ui.horizontal_top(|ui| {
                    // Trefferlisten wachsen nicht mit: Max-Höhe, Rest scrollt.
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Bestand").strong());
                        egui::ScrollArea::vertical()
                            .max_width(col_w)
                            .max_height(manual_list_h)
                            .show(ui, |ui| {
                                for hit in &existing {
                                    let selected =
                                        selected_keep.as_deref() == Some(hit.id.as_str());
                                    if ui.selectable_label(selected, &hit.label).clicked() {
                                        if let Some(review) = app.merge_review.as_mut() {
                                            review.manual_keep =
                                                if selected { None } else { Some(hit.id.clone()) };
                                        }
                                    }
                                }
                                if existing.is_empty() {
                                    ui.small("—");
                                }
                            });
                        if existing_total > existing.len() {
                            ui.small(format!("… +{} weitere", existing_total - existing.len()));
                        }
                    });
                    // KEIN ui.separator() hier: Im horizontalen Layout nähme er
                    // die volle verfügbare Höhe ein und sprengte den Dialog.
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Neu").strong());
                        egui::ScrollArea::vertical()
                            .max_width(col_w)
                            .max_height(manual_list_h)
                            .show(ui, |ui| {
                                for hit in &fresh {
                                    let selected =
                                        selected_drop.as_deref() == Some(hit.id.as_str());
                                    if ui.selectable_label(selected, &hit.label).clicked() {
                                        if let Some(review) = app.merge_review.as_mut() {
                                            review.manual_drop =
                                                if selected { None } else { Some(hit.id.clone()) };
                                        }
                                    }
                                }
                                if fresh.is_empty() {
                                    ui.small("—");
                                }
                            });
                        if fresh_total > fresh.len() {
                            ui.small(format!("… +{} weitere", fresh_total - fresh.len()));
                        }
                    });
                });
                ui.horizontal(|ui| {
                    if ui
                        .button("Als Match hinzufügen")
                        .on_hover_text(
                            "Gewähltes Paar als Treffer übernehmen (mehrfach möglich, \
                             danach Checkbox zum Zusammenführen setzen).",
                        )
                        .clicked()
                    {
                        app.add_manual_match();
                    }
                    ui.small("Paar erscheint unselektiert in der Liste unten.");
                });
            }
            }
            ui.separator();
            egui::ScrollArea::both()
                .max_height(candidate_list_h)
                .show(ui, |ui| {
                    for index in 0..count {
                        // Lesephase (nur lesen): Anzeige-Daten sammeln.
                        struct FieldRow {
                            label: String,
                            is_choice: bool,
                            new_text: String,
                            old_text: String,
                        }
                        let rows = app
                            .merge_review
                            .as_ref()
                            .and_then(|review| review.candidates.get(index))
                            .map(|entry| {
                                let candidate = &entry.candidate;
                                let (names, scores, fields) =
                                    merge_candidate_fields(app, candidate);
                                let rows: Vec<FieldRow> = fields
                                    .into_iter()
                                    .map(
                                        |(label, is_choice, new_text, old_text)| FieldRow {
                                            label,
                                            is_choice,
                                            new_text,
                                            old_text,
                                        },
                                    )
                                    .collect();
                                (
                                    candidate.keep_id.clone(),
                                    candidate.drop_id.clone(),
                                    names,
                                    scores,
                                    entry.selected,
                                    entry.take_new_birth,
                                    entry.take_new_death,
                                    entry.take_new_name,
                                    entry.parent_pick_new.clone(),
                                    entry.parent_pick_old.clone(),
                                    rows,
                                )
                            });
/// Vergleichsdaten eines Merge-Treffers (Namen, Scores, Zeilen aus Label +
/// Neu/Vorhanden-Text + Auswahl-Flag): geteilt von Review-Dialog und
/// Einzel-Merge-Dialog.
fn merge_candidate_fields(
    app: &MiniGramps,
    candidate: &crate::model::MergeCandidate,
) -> (String, String, Vec<(String, bool, String, String)>) {
    let keep = app.data.find(&candidate.keep_id);
    let drop = app.data.find(&candidate.drop_id);
    let name = |person: Option<&crate::model::Person>, fallback: &str| {
        person
            .map(|person| person.display_name())
            .unwrap_or_else(|| fallback.to_string())
    };
    let fmt_date_place = |date: &str, place: &str| -> String {
        if date.trim().is_empty() {
            "—".to_string()
        } else if place.trim().is_empty() {
            date.to_string()
        } else {
            format!("{date}, {place}")
        }
    };
    let dates = |person: Option<&crate::model::Person>| -> (String, String) {
        match person {
            Some(person) => (
                fmt_date_place(&person.birth, &person.birth_place),
                fmt_date_place(&person.death, &person.death_place),
            ),
            None => ("?".to_string(), "?".to_string()),
        }
    };
    let (drop_birth, drop_death) = dates(drop);
    let (keep_birth, keep_death) = dates(keep);
    let names_of = |ids: Vec<String>| -> String {
        if ids.is_empty() {
            "—".to_string()
        } else {
            ids.iter()
                .filter_map(|id| {
                    app.data
                        .find(id)
                        .map(|relative| relative.display_name())
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    let relations = |person: Option<&crate::model::Person>| -> [String; 4] {
        let unknown = || {
            [
                "?".to_string(),
                "?".to_string(),
                "?".to_string(),
                "?".to_string(),
            ]
        };
        let Some(person) = person else {
            return unknown();
        };
        [
            names_of(
                app.data
                    .parents_of(&person.id)
                    .iter()
                    .map(|relative| relative.id.clone())
                    .collect(),
            ),
            names_of(
                app.data
                    .siblings_of(&person.id)
                    .iter()
                    .map(|relative| relative.id.clone())
                    .collect(),
            ),
            names_of(
                app.data
                    .partners_of(&person.id)
                    .iter()
                    .map(|relative| relative.id.clone())
                    .collect(),
            ),
            names_of(
                app.data
                    .children_of(&person.id)
                    .iter()
                    .map(|relative| relative.id.clone())
                    .collect(),
            ),
        ]
    };
    let drop_rel = relations(drop);
    let keep_rel = relations(keep);
    let keep_name = name(keep, &candidate.keep_id);
    let drop_name = name(drop, &candidate.drop_id);
    let names = format!("{keep_name} ↔ {drop_name}",);
    let scores = format!(
        "— Vorname {:.0} % · Nachname {:.0} % · Verwandt {:.0} %{}",
        candidate.name_score * 100.0,
        candidate.family_score * 100.0,
        candidate.kin_score * 100.0,
        if candidate.birth_match {
            " · Geburt gleich"
        } else {
            ""
        },
    );
    let fields = vec![
        ("Name", true, drop_name, keep_name),
        ("Geburt", true, drop_birth, keep_birth),
        ("Tod", true, drop_death, keep_death),
        (
            "Eltern",
            false,
            drop_rel[0].clone(),
            keep_rel[0].clone(),
        ),
        (
            "Geschwister",
            false,
            drop_rel[1].clone(),
            keep_rel[1].clone(),
        ),
        (
            "Partner",
            false,
            drop_rel[2].clone(),
            keep_rel[2].clone(),
        ),
        (
            "Kinder",
            false,
            drop_rel[3].clone(),
            keep_rel[3].clone(),
        ),
    ];
    let rows: Vec<(String, bool, String, String)> = fields
        .into_iter()
        .map(|(label, is_choice, new_text, old_text)| {
            (label.to_string(), is_choice, new_text, old_text)
        })
        .collect();
    (names, scores, rows)
}
                        let Some((
                            keep_id,
                            drop_id,
                            names,
                            scores,
                            selected,
                            take_birth,
                            take_death,
                            take_name,
                            pick_new,
                            pick_old,
                            rows,
                        )) = rows else {
                            continue;
                        };
                        // Kopfzeile: Namen groß, Scores klein. Auswahl klappt
                        // die Detailzeilen auf.
                        ui.horizontal(|ui| {
                            let mut checked = selected;
                            ui.checkbox(&mut checked, names);
                            ui.small(&scores);
                            if let Some(review) = app.merge_review.as_mut() {
                                if let Some(entry) = review.candidates.get_mut(index) {
                                    entry.selected = checked;
                                }
                            }
                            if ui.small_button("Kein Match").clicked() {
                                app.reject_merge_candidate(&drop_id);
                            }
                        });
                        if selected {
                            let tuples: Vec<(String, bool, String, String)> = rows
                                .iter()
                                .map(|row| {
                                    (
                                        row.label.clone(),
                                        row.is_choice,
                                        row.new_text.clone(),
                                        row.old_text.clone(),
                                    )
                                })
                                .collect();
                            let mut take_birth = take_birth;
                            let mut take_death = take_death;
                            let mut take_name = take_name;
                            merge_fields_grid(
                                ui,
                                ("merge-fields", drop_id.clone()),
                                &tuples,
                                &mut take_name,
                                &mut take_birth,
                                &mut take_death,
                            );
                            if let Some(review) = app.merge_review.as_mut() {
                                if let Some(entry) = review.candidates.get_mut(index) {
                                    entry.take_new_birth = take_birth;
                                    entry.take_new_death = take_death;
                                    entry.take_new_name = take_name;
                                }
                            }
                            // Eltern mit-mergen: je Seite ein Elternteil wählen,
                            // Button fügt das Paar als Treffer hinzu.
                            let keep_parents: Vec<(String, String)> = app
                                .data
                                .parents_of(&keep_id)
                                .iter()
                                .map(|person| (person.id.clone(), person.display_name()))
                                .collect();
                            let drop_parents: Vec<(String, String)> = app
                                .data
                                .parents_of(&drop_id)
                                .iter()
                                .map(|person| (person.id.clone(), person.display_name()))
                                .collect();
                            if !keep_parents.is_empty() && !drop_parents.is_empty() {
                                ui.small("Eltern mit-mergen (je Seite wählen):");
                                ui.horizontal(|ui| {
                                    for (pid, pname) in &drop_parents {
                                        let is_picked = pick_new.as_deref() == Some(pid.as_str());
                                        if ui.selectable_label(is_picked, pname).clicked() {
                                            if let Some(review) = app.merge_review.as_mut() {
                                                if let Some(entry) =
                                                    review.candidates.get_mut(index)
                                                {
                                                    entry.parent_pick_new = if is_picked {
                                                        None
                                                    } else {
                                                        Some(pid.clone())
                                                    };
                                                }
                                            }
                                        }
                                    }
                                    ui.label("↔");
                                    for (pid, pname) in &keep_parents {
                                        let is_picked = pick_old.as_deref() == Some(pid.as_str());
                                        if ui.selectable_label(is_picked, pname).clicked() {
                                            if let Some(review) = app.merge_review.as_mut() {
                                                if let Some(entry) =
                                                    review.candidates.get_mut(index)
                                                {
                                                    entry.parent_pick_old = if is_picked {
                                                        None
                                                    } else {
                                                        Some(pid.clone())
                                                    };
                                                }
                                            }
                                        }
                                    }
                                    let ready = pick_new.is_some() && pick_old.is_some();
                                    if ui
                                        .add_enabled(
                                            ready,
                                            egui::Button::new("+ Elternpaar"),
                                        )
                                        .on_hover_text(
                                            "Gewählte Eltern als Mergepaar hinzufügen",
                                        )
                                        .clicked()
                                        && ready
                                    {
                                        if let (Some(keep_pick), Some(drop_pick)) =
                                            (pick_old.clone(), pick_new.clone())
                                        {
                                            if app.add_parent_pair_as_match(
                                                &keep_pick,
                                                &drop_pick,
                                            ) {
                                                if let Some(review) = app.merge_review.as_mut() {
                                                    if let Some(entry) =
                                                        review.candidates.get_mut(index)
                                                    {
                                                        entry.parent_pick_new = None;
                                                        entry.parent_pick_old = None;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                });
                            }
                            ui.small("Verknüpfungen (Eltern, Geschwister, Partner, Kinder) werden vereint.");
                        }
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                let selected = app
                    .merge_review
                    .as_ref()
                    .map(|review| review.candidates.iter().filter(|entry| entry.selected).count())
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
                // Nur beim Import-Review: Anhang komplett verwerfen.
                let can_discard = app
                    .merge_review
                    .as_ref()
                    .is_some_and(|review| review.pre_import.is_some());
                if can_discard
                    && ui
                        .button("Abbrechen (Anhang verwerfen)")
                        .on_hover_text(
                            "Angehängte Personen samt bereits zusammengeführter \
                             Änderungen verwerfen und Fenster schließen.",
                        )
                        .clicked()
                {
                    app.discard_appended_import();
                }
            });
        });
    if !open {
        app.show_merge_review = false;
    }
}

/// Gesamt-Übereinstimmung eines Treffers in Prozent (Mittel aus Vorname,
/// Nachname, Verwandtschaft, gedeckelt) für Trefferlisten.
fn match_total_percent(candidate: &crate::model::MergeCandidate) -> u32 {
    ((candidate.name_score + candidate.family_score + candidate.kin_score) / 3.0 * 100.0)
        .round()
        .clamp(0.0, 100.0) as u32
}

/// Einzel-Merge-Dialog (rechte Leiste, Zusammenführen): Top-5 Treffer zur
/// gewählten Person (ohne Selbst), Klick öffnet die Detailansicht mit
/// Vergleichstabelle und Zusammenführen-Button.
pub fn show_person_merge(app: &mut MiniGramps, ctx: &egui::Context) {
    if !app.show_person_merge {
        return;
    }
    let mut open = true;
    egui::Window::new(window_title("Person zusammenführen"))
        .id(egui::Id::new("person-merge-v1"))
        .open(&mut open)
        .movable(false)
        .resizable(true)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(720.0)
        .max_height(ctx.content_rect().height() * 0.9)
        .show(ctx, |ui| {
            let target_name = app
                .selected
                .as_deref()
                .and_then(|id| app.data.find(id))
                .map(|person| person.display_name())
                .unwrap_or_else(|| "–".to_string());
            ui.label(
                egui::RichText::new(format!("Zusammenführen mit: {target_name}")).strong(),
            );
            ui.horizontal(|ui| {
                ui.label("Suche");
                ui.text_edit_singleline(&mut app.person_merge_query)
                    .on_hover_text("Filtert die Trefferliste zusätzlich nach Namen.");
            });
            ui.separator();
            // Trefferliste (Top-5): Name + Prozent + Scores.
            let needle = app.person_merge_query.trim().to_lowercase();
            let mut detail_pick: Option<(String, String)> = None;
            for candidate in &app.person_merge_hits {
                let drop_name = app
                    .data
                    .find(&candidate.drop_id)
                    .map(|person| person.display_name())
                    .unwrap_or_else(|| candidate.drop_id.clone());
                if !needle.is_empty() && !drop_name.to_lowercase().contains(&needle) {
                    continue;
                }
                let total = match_total_percent(candidate);
                let label = format!(
                    "{drop_name} — {total} % (V {:.0} · N {:.0} · Verw {:.0})",
                    candidate.name_score * 100.0,
                    candidate.family_score * 100.0,
                    candidate.kin_score * 100.0,
                );
                let selected = app.person_merge_detail.as_ref().is_some_and(
                    |(keep, drop)| {
                        keep == &candidate.keep_id && drop == &candidate.drop_id
                    },
                );
                if ui.selectable_label(selected, label).clicked() {
                    detail_pick = Some((candidate.keep_id.clone(), candidate.drop_id.clone()));
                }
            }
            if app.person_merge_hits.is_empty() {
                ui.small("Keine Übereinstimmungen gefunden.");
            }
            if let Some((keep_id, drop_id)) = detail_pick {
                let take_birth = app
                    .data
                    .find(&keep_id)
                    .map(|person| person.birth.trim().is_empty())
                    .unwrap_or(true);
                let take_death = app
                    .data
                    .find(&keep_id)
                    .map(|person| person.death.trim().is_empty())
                    .unwrap_or(true);
                app.person_merge_take_birth = take_birth;
                app.person_merge_take_death = take_death;
                app.person_merge_detail = Some((keep_id, drop_id));
            }
            // Detailansicht: Scores, Geburt/Tod-Vergleich, Beziehungen der
            // Treffer-Person, Feldwahl und Zusammenführen-Button.
            let detail = app.person_merge_detail.clone().and_then(|(keep, drop)| {
                let threshold = (app.match_threshold / 100.0).clamp(0.0, 1.0);
                let common_min = app.common_given_threshold.clamp(2, 10);
                let candidate =
                    app.data
                        .pair_match_candidate(&keep, &drop, threshold, common_min)?;
                let keep_person = app.data.find(&keep)?.clone();
                let drop_person = app.data.find(&drop)?.clone();
                Some((keep, drop, candidate, keep_person, drop_person))
            });
            if let Some((_, _, candidate, keep_person, drop_person)) = detail {
                let total = match_total_percent(&candidate);
                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "{} ↔ {}",
                        keep_person.display_name(),
                        drop_person.display_name()
                    ))
                    .strong(),
                );
                ui.small(format!(
                    "Übereinstimmung {total} % — Vorname {:.0} % · Nachname {:.0} % · Verwandt {:.0} %{}",
                    candidate.name_score * 100.0,
                    candidate.family_score * 100.0,
                    candidate.kin_score * 100.0,
                    if candidate.birth_match {
                        " · Geburt gleich"
                    } else {
                        ""
                    },
                ));
                let fmt = |date: &str, place: &str| -> String {
                    if date.trim().is_empty() {
                        "—".to_string()
                    } else if place.trim().is_empty() {
                        date.to_string()
                    } else {
                        format!("{date}, {place}")
                    }
                };
                let names_of = |ids: Vec<String>| -> String {
                    if ids.is_empty() {
                        "—".to_string()
                    } else {
                        ids.iter()
                            .filter_map(|id| app.data.find(id))
                            .map(|person| person.display_name())
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                };
                let rel_of = |id: &str| -> [String; 4] {
                    let ids = |people: Vec<&crate::model::Person>| {
                        people
                            .iter()
                            .map(|person| person.id.clone())
                            .collect::<Vec<_>>()
                    };
                    [
                        names_of(ids(app.data.parents_of(id))),
                        names_of(ids(app.data.siblings_of(id))),
                        names_of(ids(app.data.partners_of(id))),
                        names_of(ids(app.data.children_of(id))),
                    ]
                };
                let keep_rel = rel_of(&keep_person.id);
                let drop_rel = rel_of(&drop_person.id);
                let rows: Vec<(String, bool, String, String)> = vec![
                    (
                        "Name".to_string(),
                        true,
                        drop_person.display_name().to_string(),
                        keep_person.display_name().to_string(),
                    ),
                    (
                        "Geburt".to_string(),
                        true,
                        fmt(&drop_person.birth, &drop_person.birth_place),
                        fmt(&keep_person.birth, &keep_person.birth_place),
                    ),
                    (
                        "Tod".to_string(),
                        true,
                        fmt(&drop_person.death, &drop_person.death_place),
                        fmt(&keep_person.death, &keep_person.death_place),
                    ),
                    ("Eltern".to_string(), false, drop_rel[0].clone(), keep_rel[0].clone()),
                    (
                        "Geschwister".to_string(),
                        false,
                        drop_rel[1].clone(),
                        keep_rel[1].clone(),
                    ),
                    (
                        "Partner".to_string(),
                        false,
                        drop_rel[2].clone(),
                        keep_rel[2].clone(),
                    ),
                    (
                        "Kinder".to_string(),
                        false,
                        drop_rel[3].clone(),
                        keep_rel[3].clone(),
                    ),
                ];
                merge_fields_grid(
                    ui,
                    "person-merge-fields",
                    &rows,
                    &mut app.person_merge_take_name,
                    &mut app.person_merge_take_birth,
                    &mut app.person_merge_take_death,
                );
                ui.separator();
                // Eltern mit-mergen: je Seite ein Elternteil wählen, Button
                // fügt das Paar als Treffer hinzu (Review wird geöffnet).
                let keep_parents: Vec<(String, String)> = app
                    .data
                    .parents_of(&keep_person.id)
                    .iter()
                    .map(|person| (person.id.clone(), person.display_name()))
                    .collect();
                let drop_parents: Vec<(String, String)> = app
                    .data
                    .parents_of(&drop_person.id)
                    .iter()
                    .map(|person| (person.id.clone(), person.display_name()))
                    .collect();
                if !keep_parents.is_empty() && !drop_parents.is_empty() {
                    ui.small("Eltern mit-mergen (je Seite wählen):");
                    ui.horizontal(|ui| {
                        for (pid, pname) in &drop_parents {
                            let picked =
                                app.person_merge_parent_new.as_deref() == Some(pid.as_str());
                            if ui.selectable_label(picked, pname).clicked() {
                                app.person_merge_parent_new = if picked {
                                    None
                                } else {
                                    Some(pid.clone())
                                };
                            }
                        }
                        ui.label("↔");
                        for (pid, pname) in &keep_parents {
                            let picked =
                                app.person_merge_parent_old.as_deref() == Some(pid.as_str());
                            if ui.selectable_label(picked, pname).clicked() {
                                app.person_merge_parent_old = if picked {
                                    None
                                } else {
                                    Some(pid.clone())
                                };
                            }
                        }
                        let ready = app.person_merge_parent_new.is_some()
                            && app.person_merge_parent_old.is_some();
                        if ui
                            .add_enabled(ready, egui::Button::new("+ Elternpaar"))
                            .on_hover_text("Gewählte Eltern als Mergepaar hinzufügen")
                            .clicked()
                            && ready
                        {
                            let keep_pick = app.person_merge_parent_old.clone().unwrap();
                            let drop_pick = app.person_merge_parent_new.clone().unwrap();
                            if app.add_parent_pair_as_match(&keep_pick, &drop_pick) {
                                app.person_merge_parent_new = None;
                                app.person_merge_parent_old = None;
                            }
                        }
                    });
                }
                ui.separator();
                if ui
                    .button("Zusammenführen")
                    .on_hover_text("Gewählte Person in die aktuelle einführen und löschen.")
                    .clicked()
                {
                    let (take_birth, take_death, take_name) = (
                        app.person_merge_take_birth,
                        app.person_merge_take_death,
                        app.person_merge_take_name,
                    );
                    app.apply_person_merge(take_birth, take_death, take_name);
                }
            }
        });
    if !open {
        app.show_person_merge = false;
        app.person_merge_detail = None;
    }
}

/// Schnellerfassung: geführte Eingabe an die Referenzperson.
///
/// Abwärts (Standard): Partner + beliebig viele Kinder zur Referenz;
/// Aufwärts: Elternteil 1/2 (M/W vorausgewählt) direkt zur Referenz (die
/// Referenz ist das Kind). Bestehende Kinder/Eltern stehen gebunden in den
/// Zeilen; mehrere Partner werden seitenweise nacheinander durchgeblättert.
/// Zeilen ohne Namen werden übersprungen; unter jeder Zeile erscheinen die
/// drei besten Bestandstreffer mit Geburtsjahr — Klick öffnet Details mit
/// +-Button zum Setzen statt Neuanlegen. Übernehmen speichert sofort und
/// setzt die Referenz per FIFO-Queue auf die nächste eingetragene Person;
/// „Übern. & Übersp." speichert ohne Queue (abwärts: nächster Partner).
pub fn show_quick(app: &mut MiniGramps, ctx: &egui::Context) {
    if !app.show_quick {
        return;
    }
    let mut open = true;
    // Strg+Enter / Umschalt+Strg+Enter werden GLOBAL entgegengenommen, damit
    // sie auch aus einem fokussierten Textfeld heraus wirken.
    let commit_block = ctx.input(|i| {
        i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::Enter)
    });
    let new_partner = ctx.input(|i| {
        i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Enter)
    });
    let mut committed = false;
    egui::Window::new(window_title("Schnellerfassung"))
        .id(egui::Id::new("quick-entry-v2"))
        .open(&mut open)
        .movable(false)
        .resizable(true)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(700.0)
        .max_height(ctx.content_rect().height() * 0.9)
        .show(ctx, |ui| {
            use crate::model::QuickDir;

            // Kopf: Name (+ Geburtsjahr), etwas größer als normal.
            let (ref_name, ref_year) = app
                .quick_ref_id
                .as_ref()
                .and_then(|id| app.data.find(id))
                .map(|person| {
                    (
                        person.display_name(),
                        crate::model::birth_year(&person.birth),
                    )
                })
                .unwrap_or_else(|| ("–".to_string(), String::new()));
            let ref_label = if ref_year.is_empty() {
                ref_name
            } else {
                format!("{ref_name} ({ref_year})")
            };
            ui.label(egui::RichText::new(ref_label).size(16.0).strong());
            // Richtung umschalten (Referenzstand neu laden).
            let dir_before = app.quick_dir;
            ui.horizontal(|ui| {
                ui.label("Richtung");
                ui.radio_value(&mut app.quick_dir, QuickDir::Down, "Abwärts (Partner + Kinder)");
                ui.radio_value(&mut app.quick_dir, QuickDir::Up, "Aufwärts (Eltern zur Referenz)");
            });
            if app.quick_dir != dir_before {
                // Beim Umschalten den laufenden Block verwerfen und den
                // Referenzstand neu laden (gebundene Zeilen).
                app.quick_partner_idx = 0;
                quick_load_reference(app);
            }
            ui.separator();

            let is_up = app.quick_dir == QuickDir::Up;
            // Verwaiste Fokusziele räumen (Kopf nur abwärts, Zeilen nur
            // existent) — sonst hängt der Tastaturfokus in der Luft.
            let focus_ok = match app.quick_focus {
                None => true,
                Some((0, _)) => !is_up,
                Some((row, _)) => row >= 1 && row <= app.quick_rows.len(),
            };
            if !focus_ok {
                app.quick_focus = None;
            }
            if !is_up {
                // Kopfzeile nur abwärts (Partner). Aufwärts ist die Referenz
                // selbst das Kind — das Kind spielt keine Rolle.
                // Partner-Auto-Geschlecht: frische, ungebundene Kopfzeile ohne
                // jede Eingabe folgt dem Gegengeschlecht der Referenz (bewusste
                // Wahl — getippt, gebunden oder umgestellt — bleibt).
                if app.quick_head.bind.is_none()
                    && app.quick_head.is_empty()
                    && app.quick_head.gender == Gender::Unknown
                {
                    let opposite = app
                        .quick_ref_id
                        .as_deref()
                        .and_then(|id| app.data.find(id))
                        .map(|reference| match reference.gender {
                            Gender::Male => Gender::Female,
                            Gender::Female => Gender::Male,
                            Gender::Unknown => Gender::Unknown,
                        });
                    if let Some(gender) = opposite {
                        app.quick_head.gender = gender;
                    }
                }
                let enter_head = quick_row_fields(ui, app, 0, "Partner");
                if enter_head {
                    // Vom Partner aus immer eine Kindzeile sicherstellen.
                    if app.quick_rows.is_empty() {
                        app.quick_rows.push(quick_new_child_row(app));
                    }
                    app.quick_focus = Some((1, 0));
                }
                ui.add_space(4.0);
            }

            // Weitere Zeilen: Abwärts Kinder, Aufwärts Elternteil 1/2 (+).
            for index in 0..app.quick_rows.len() {
                let label = match app.quick_dir {
                    QuickDir::Down => format!("Kind {}", index + 1),
                    QuickDir::Up => format!("Elternteil {}", index + 1),
                };
                let enter = quick_row_fields(ui, app, index + 1, &label);
                if enter {
                    if index + 1 == app.quick_rows.len() {
                        if is_up {
                            // Aufwärts: mehr als zwei Eltern hat niemand —
                            // keine neue Zeile, Fokus zurück zur ersten.
                            app.quick_focus = Some((1, 0));
                        } else {
                            // Enter in der letzten Zeile: neue Zeile (mit
                            // Vatername-Default) anfügen und dort den Fokus setzen.
                            app.quick_rows.push(quick_new_child_row(app));
                            app.quick_focus = Some((app.quick_rows.len(), 0));
                        }
                    } else {
                        // Sonst einen Schritt nach unten.
                        app.quick_focus = Some((index + 2, 0));
                    }
                }
            }
            // Aufwärts sind es genau zwei Eltern (kein + Zeile); abwärts
            // beliebig viele Kinder.
            if !is_up && ui.button("+ Zeile").on_hover_text(
                "Weiteres Kind anfügen (Nachname defaultmäßig vom Vater).",
            ).clicked() {
                app.quick_rows.push(quick_new_child_row(app));
                let idx = app.quick_rows.len();
                app.quick_focus = Some((idx, 0));
            }
            ui.separator();

            // Die Bestandssuche steckt jetzt in jeder Zeile (Inline-Treffer
            // unter der Eingabe, Klick öffnet Details mit +-Button).
            ui.small("Tipp: Passende Bestandspersonen erscheinen direkt unter jeder Zeile — Klick öffnet die Detailansicht mit +-Button zum Setzen.");
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Übernehmen (Strg+Enter)").on_hover_text(
                    "Eingabe speichern und mit der nächsten eingetragenen \
                     Person weitermachen (neu oder gesetzt; leere Eingabe \
                     springt nur weiter).",
                ).clicked() {
                    commit_quick_block(app);
                    committed = true;
                }
                if !is_up && ui.button("Neuer Partner (Umsch+Strg+Enter)").on_hover_text(
                    "Eingabe speichern, aber bei derselben Referenz bleiben — \
                     frische Kopfzeile für den nächsten Partner.",
                ).clicked() {
                    commit_quick_partner(app);
                    committed = true;
                }
                if ui.button("Übern. & Übersp.").on_hover_text(
                    "Eingabe speichern, aber niemanden auf den Stack legen \
                     (Verwandte überspringen) — abwärts weiter zum nächsten \
                     Partner.",
                ).clicked() {
                    commit_quick_skip(app);
                    committed = true;
                }
            });
            if !committed {
                if commit_block {
                    commit_quick_block(app);
                } else if new_partner {
                    commit_quick_partner(app);
                }
            }
            ui.horizontal(|ui| {
                if ui.button("Abschließen").on_hover_text(
                    "Eingabe speichern und Fenster schließen.",
                ).clicked() {
                    app.commit_quick_entry();
                }
            });
        });
    if !open {
        app.show_quick = false;
    }
}

/// Familienname des Vaters für die Abwärts-Erfassung: männlicher Partner
/// der Kopfzeile (getippt oder gebunden) vor männlicher Referenzperson.
/// Leer, wenn kein Vatername bekannt ist.
fn quick_father_family(app: &MiniGramps) -> String {
    if app.quick_head.gender == Gender::Male {
        if !app.quick_head.family.trim().is_empty() {
            return app.quick_head.family.trim().to_string();
        }
        if let Some(id) = app.quick_head.bind.as_deref() {
            if let Some(person) = app.data.find(id) {
                if !person.family_name.trim().is_empty() {
                    return person.family_name.trim().to_string();
                }
            }
        }
    }
    if let Some(id) = app.quick_ref_id.as_deref() {
        if let Some(person) = app.data.find(id) {
            if person.gender == Gender::Male && !person.family_name.trim().is_empty() {
                return person.family_name.trim().to_string();
            }
        }
    }
    String::new()
}

/// Neue Kinderzeile (abwärts) mit Vatername als Default-Nachname.
fn quick_new_child_row(app: &MiniGramps) -> crate::model::QuickPerson {
    crate::model::QuickPerson {
        family: quick_father_family(app),
        ..Default::default()
    }
}

/// Gemeinsame Kinder zweier Eltern (Familien mit genau diesem Elternpaar).
fn joint_children_of(data: &crate::model::TreeData, first: &str, second: &str) -> Vec<String> {
    let mut out = Vec::new();
    for family in &data.families {
        let parents: Vec<&str> = [&family.parent_a, &family.parent_b]
            .into_iter()
            .flatten()
            .map(|parent| parent.as_str())
            .collect();
        if parents.contains(&first) && parents.contains(&second) {
            for child in &family.children {
                if !out.contains(child) {
                    out.push(child.clone());
                }
            }
        }
    }
    out
}

/// Alleinige Kinder: in Familien, wo die Person der einzige erfasste
/// Elternteil ist (kein zweiter Partner hinterlegt).
fn single_children_of(data: &crate::model::TreeData, id: &str) -> Vec<String> {
    let mut out = Vec::new();
    for family in &data.families {
        let is_a = family.parent_a.as_deref() == Some(id);
        let is_b = family.parent_b.as_deref() == Some(id);
        if !is_a && !is_b {
            continue;
        }
        let other = if is_a { &family.parent_b } else { &family.parent_a };
        if other.is_some() {
            continue;
        }
        for child in &family.children {
            if !out.contains(child) {
                out.push(child.clone());
            }
        }
    }
    out
}

/// Referenzstand laden: Bestehende Kinder/Eltern stehen gebunden in den
/// Zeilen (grau + gesperrt, per „entsetzen" lösbar). Abwärts die
/// Partner-Seite `quick_partner_idx` (letzte Seite ohne Partner),
/// aufwärts die (höchstens zwei) Eltern mit M/W-Pad.
pub fn quick_load_reference(app: &mut MiniGramps) {
    use crate::model::QuickDir;
    if app.quick_dir == QuickDir::Up {
        load_quick_up(app);
    } else {
        load_quick_down_page(app);
    }
}

/// Abwärts-Seite laden: Partner[`quick_partner_idx`] als Kopf plus gemeinsame
/// Kinder als Zeilen (alles gebunden); jenseits des letzten Partners die
/// Ohne-Partner-Seite mit alleinigen Kindern der Referenz.
fn load_quick_down_page(app: &mut MiniGramps) {
    let ref_id = app.quick_ref_id.clone();
    let partner_ids: Vec<String> = ref_id
        .as_deref()
        .map(|id| {
            app.data
                .partners_of(id)
                .iter()
                .map(|person| person.id.clone())
                .collect()
        })
        .unwrap_or_default();
    let page = app.quick_partner_idx.min(partner_ids.len());
    app.quick_partner_idx = page;
    app.quick_head = crate::model::QuickPerson::default();
    app.quick_rows.clear();
    app.quick_detail = None;
    if let Some(pid) = partner_ids.get(page) {
        bind_quick_person(app, 0, pid);
        if let Some(rid) = ref_id.as_deref() {
            for cid in joint_children_of(&app.data, rid, pid) {
                app.quick_rows.push(crate::model::QuickPerson::default());
                let row = app.quick_rows.len();
                bind_quick_person(app, row, &cid);
            }
        }
    } else if let Some(rid) = ref_id.as_deref() {
        for cid in single_children_of(&app.data, rid) {
            app.quick_rows.push(crate::model::QuickPerson::default());
            let row = app.quick_rows.len();
            bind_quick_person(app, row, &cid);
        }
    }
    app.quick_focus = Some((0, 0));
}

/// Aufwärts laden: vorhandene Eltern gebunden (max. zwei), Rest mit
/// M/W-Preset aufgefüllt.
fn load_quick_up(app: &mut MiniGramps) {
    use crate::model::{Gender, QuickPerson};
    app.quick_head = QuickPerson::default();
    app.quick_rows.clear();
    app.quick_detail = None;
    if let Some(rid) = app.quick_ref_id.clone() {
        let parent_ids: Vec<String> = app
            .data
            .parents_of(&rid)
            .iter()
            .map(|person| person.id.clone())
            .collect();
        for pid in parent_ids.into_iter().take(2) {
            app.quick_rows.push(QuickPerson::default());
            let row = app.quick_rows.len();
            bind_quick_person(app, row, &pid);
        }
    }
    while app.quick_rows.len() < 2 {
        let preset = if app.quick_rows.is_empty() {
            Gender::Male
        } else {
            Gender::Female
        };
        app.quick_rows
            .push(QuickPerson { gender: preset, ..Default::default() });
    }
    app.quick_focus = Some((1, 0));
}

/// Eine Erfassungszeile (Kopf oder weitere) mit Vorname/Nachname/Geburt/Tod/
/// Geschlecht rendern. `row` ist 0 für die Kopfzeile, sonst der 1-basierte
/// Index der `quick_rows`-Liste. Der Fokus wird beim ersten Feld gesetzt,
/// wenn `quick_focus` darauf zeigt. Darunter: gebundene Person (Chip mit
/// Lösen-Button) oder die drei besten Inline-Treffer mit Geburtsjahr —
/// Klick öffnet die Detailansicht mit +-Button (`quick_detail`).
fn quick_row_fields(
    ui: &mut egui::Ui,
    app: &mut MiniGramps,
    row: usize,
    label: &str,
) -> bool {
    let focus = app.quick_focus;
    let mut focus_consumed = false;
    let mut enter_pressed = false;
    // Disjoint: person (head/rows) + Fokus (eigene Fields).
    let person = if row == 0 {
        &mut app.quick_head
    } else if let Some(index) = row.checked_sub(1) {
        match app.quick_rows.get_mut(index) {
            Some(person) => person,
            None => return false,
        }
    } else {
        return false;
    };
    // Gesetzte Zeile: Nachname + Geschlecht bleiben Anker (grau + gesperrt),
    // Vorname/Geburt/Tod sind editierbar (schreibt beim Speichern zurück).
    let bound = person.bind.is_some();
    let mut unset = false;
    let mut reset_pressed = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("{label}:"));
        // Textfelder (wrappen bei schmalem Fenster auf 2 Zeilen um).
        for (col, field) in [
            &mut person.given,
            &mut person.family,
            &mut person.birth,
            &mut person.death,
        ]
        .into_iter()
        .enumerate()
        {
            // Feldname grau im leeren Feld (Platzhalter statt grauer Fläche).
            let hint = match col {
                0 => "Vorname",
                1 => "Nachname",
                2 => "Geburt",
                _ => "Tod",
            };
            let edit = egui::TextEdit::singleline(field)
                .id(egui::Id::new(("quick-field", row, col)))
                .desired_width(90.0)
                .hint_text(hint);
            // Col 1 (Nachname) bleibt bei gesetzter Zeile gesperrt.
            let locked = bound && col == 1;
            let response = if locked {
                ui.add_enabled(false, edit)
            } else {
                ui.add(edit)
            };
            if !locked && focus == Some((row, col)) {
                response.request_focus();
                focus_consumed = true;
            }
            // Einzelne Textfelder verlieren den Fokus per Enter (surrender_focus
            // intern). Die egui-empfohlene Kombination erkennt den Fall.
            if response.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !ui.input(|i| i.modifiers.command)
            {
                enter_pressed = true;
            }
        }
        ui.add_enabled_ui(!bound, |ui| {
            let combo = egui::ComboBox::from_id_salt(("quick-gender", row))
                .selected_text(crate::ui::picker::gender_label(person.gender))
                .width(110.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut person.gender, Gender::Female, "Weiblich");
                    ui.selectable_value(&mut person.gender, Gender::Male, "Männlich");
                    ui.selectable_value(&mut person.gender, Gender::Unknown, "Nicht angegeben");
                });
            // Popup offen (Maus-Klick): Pfeile wählen die Option direkt an
            // (statt nativem Highlight), Enter schließt + steuert neue Zeile.
            // Geschlossen + fokussiert: dasselbe ohne Popup-Handling.
            let cycle = |gender: Gender, up: bool| match (up, gender) {
                (true, Gender::Female) => Gender::Unknown,
                (true, Gender::Unknown) => Gender::Male,
                (true, Gender::Male) => Gender::Female,
                (false, Gender::Female) => Gender::Male,
                (false, Gender::Unknown) => Gender::Female,
                (false, Gender::Male) => Gender::Unknown,
            };
            let up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
            let down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !ui.input(|i| i.modifiers.command);
            if egui::ComboBox::is_open(ui.ctx(), combo.response.id) {
                if up || down {
                    person.gender = cycle(person.gender, up);
                    egui::Popup::close_id(ui.ctx(), combo.response.id.with("popup"));
                }
                if enter {
                    egui::Popup::close_id(ui.ctx(), combo.response.id.with("popup"));
                    enter_pressed = true;
                }
            } else if combo.response.has_focus() {
                if up || down {
                    person.gender = cycle(person.gender, up);
                }
                // Enter alleine auf dem Geschlecht: neue Zeile (wie Textfeld).
                if enter {
                    enter_pressed = true;
                }
            }
        });
        // Am Zeilenende zwei getrennte Buttons: „entsetzen" (Text, nur
        // gesetzt — Bindung lösen) und Reset (Unlink-Symbol, alle Zeilen —
        // Gebundene auf Stand vor Änderungen, freie Zeile löschen).
        if bound
            && ui
                .small_button("entsetzen")
                .on_hover_text("Bindung lösen — Zeile wieder frei erfassen")
                .clicked()
        {
            unset = true;
        }
        if icon_only_button(ui, ICON_UNLINK, "quick-reset")
            .on_hover_text(if bound {
                "Zurücksetzen: Person auf Stand vor Änderungen"
            } else {
                "Zeile löschen (Kopf: leeren)"
            })
            .clicked()
        {
            reset_pressed = true;
        }
    });
    if unset {
        unbind_quick_person(app, row);
    }
    if reset_pressed {
        reset_quick_row(app, row);
    }
    if focus_consumed {
        app.quick_focus = None;
    }
    // Vornamen-Statistik aus dem Bestand als initiales Geschlecht — nur für
    // unberührte (Unknown), ungebundene Zeilen mit Vornamen. Jede bewusste
    // Wahl (Preset, Partner-Auto, gesetzt, Statistik) bleibt: editierbar,
    // kein Zurückschreiben.
    let guess_token: Option<String> = {
        let target = if row == 0 {
            &app.quick_head
        } else {
            match app.quick_rows.get(row - 1) {
                Some(target) => target,
                None => return enter_pressed,
            }
        };
        if target.bind.is_some() || target.gender != Gender::Unknown {
            None
        } else {
            let token = target
                .given
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            if token.is_empty() {
                None
            } else {
                Some(token)
            }
        }
    };
    if let Some(token) = guess_token {
        let cached = app.quick_gender_cache.get(&token).copied();
        let guess = match cached {
            Some(guess) => guess,
            None => {
                let guess = app.data.gender_for_given_name(&token);
                app.quick_gender_cache.insert(token, guess);
                guess
            }
        };
        if let Some(gender) = guess {
            let target = if row == 0 {
                &mut app.quick_head
            } else {
                match app.quick_rows.get_mut(row - 1) {
                    Some(target) => target,
                    None => return enter_pressed,
                }
            };
            if target.bind.is_none() && target.gender == Gender::Unknown {
                target.gender = gender;
            }
        }
    }
    // Inline-Bestandssuche unter der Eingabe (nur für ungebundene Zeilen):
    // Borrow trennen (Werte kopieren), dann suchen + rendern.
    let (bound_id, given, family) = {
        let target = if row == 0 {
            &app.quick_head
        } else {
            match app.quick_rows.get(row - 1) {
                Some(target) => target,
                None => return enter_pressed,
            }
        };
        (
            target.bind.clone(),
            target.given.clone(),
            target.family.clone(),
        )
    };
    if bound_id.is_none() {
        // Drei beste Treffer mit Geburtsjahr — Klick öffnet Details.
        let hits = quick_inline_matches(&app.data, &given, &family);
        if !hits.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.small("Meintest du:");
                for (id, name, year) in &hits {
                    let label = if year.is_empty() {
                        name.clone()
                    } else {
                        format!("{name} ({year})")
                    };
                    if ui.small_button(label).clicked() {
                        app.quick_detail = Some((row, id.clone()));
                    }
                }
            });
        }
    }
    enter_pressed
}

/// Bestehende Person an Zeile `row` binden (+-Button der Detailansicht).
/// Schreibt die ID in `bind` und füllt die Zeilenfelder mit den Infos der
/// Person (grau + gesperrt dargestellt, bis „entsetzen" löst).
fn bind_quick_person(app: &mut MiniGramps, row: usize, id: &str) {
    let Some(person) = app.data.find(id) else {
        return;
    };
    let (given, family, birth, death, gender) = (
        person.given_name.clone(),
        person.family_name.clone(),
        person.birth.clone(),
        person.death.clone(),
        person.gender,
    );
    let target = if row == 0 {
        &mut app.quick_head
    } else {
        match app.quick_rows.get_mut(row - 1) {
            Some(target) => target,
            None => return,
        }
    };
    target.bind = Some(id.to_string());
    target.given = given;
    target.family = family;
    target.birth = birth;
    target.death = death;
    target.gender = gender;
    // Fokus zurück auf die Zeile (der +Setzen-Button verschwindet mit dem
    // Detailfenster — sonst hängt der Tastaturfokus in der Luft).
    app.quick_focus = Some((row, 0));
}

/// Bindung von Zeile `row` lösen (entsetzen-Button): Felder zurück auf frisch
/// (Aufwärts mit M/W-Preset), damit kein Duplikat der gesetzten Person per
/// Übernehmen neu angelegt und auf die Queue gelegt wird.
fn unbind_quick_person(app: &mut MiniGramps, row: usize) {
    let preset = if app.quick_dir == crate::model::QuickDir::Up {
        match row {
            1 => Gender::Male,
            2 => Gender::Female,
            _ => Gender::Unknown,
        }
    } else {
        Gender::Unknown
    };
    let target = if row == 0 {
        &mut app.quick_head
    } else {
        match app.quick_rows.get_mut(row - 1) {
            Some(target) => target,
            None => return,
        }
    };
    target.bind = None;
    target.given.clear();
    target.family.clear();
    target.birth.clear();
    target.death.clear();
    target.gender = preset;
    // Fokus zurück auf die Zeile (der Reset-Button bleibt, die Bindung löst sich).
    app.quick_focus = Some((row, 0));
}

/// Reset-Button je Zeile (Unlink-Symbol, anderer Button als Entsetzen):
/// Gebundene, existierende Person auf Stand vor Änderungen zurücksetzen
/// (Bindung bleibt); ohne gesetzte Person die Zeile löschen (Kopf: leeren).
fn reset_quick_row(app: &mut MiniGramps, row: usize) {
    let bound = if row == 0 {
        app.quick_head.bind.clone()
    } else {
        app.quick_rows
            .get(row - 1)
            .and_then(|target| target.bind.clone())
    };
    match bound.and_then(|id| app.data.find(&id).map(|person| (id, person.clone()))) {
        Some((id, person)) => {
            let target = if row == 0 {
                &mut app.quick_head
            } else {
                match app.quick_rows.get_mut(row - 1) {
                    Some(target) => target,
                    None => return,
                }
            };
            target.bind = Some(id);
            target.given = person.given_name;
            target.family = person.family_name;
            target.birth = person.birth;
            target.death = person.death;
            target.gender = person.gender;
            app.quick_focus = Some((row, 0));
        }
        None => {
            if row == 0 {
                app.quick_head = crate::model::QuickPerson::default();
                app.quick_focus = Some((0, 0));
            } else if row - 1 < app.quick_rows.len() {
                app.quick_rows.remove(row - 1);
                app.quick_focus = Some((row.min(app.quick_rows.len().max(1)), 0));
            }
        }
    }
}

/// Übernehmen (Strg+Enter bzw. Button): Eingabe sofort speichern und mit
/// dem nächsten Stack-Eintrag weitermachen (Kombis und Singles in
/// Auswahl-/Zeilenreihenfolge; die Referenz springt der Reihe nach darauf,
/// ohne Baum-Umweg). Leere Eingabe springt nur weiter; ohne Warteschlange
/// wird der Referenzstand neu geladen.
fn commit_quick_block(app: &mut MiniGramps) {
    let (processed, created) = app.persist_quick_form(true);
    if !processed && app.quick_queue.is_empty() {
        app.status = "Nichts zu übernehmen".to_string();
        return;
    }
    advance_quick_ref(app, created.len());
}

/// Neuer Partner (Umsch+Strg+Enter bzw. Button): Eingabe sofort speichern
/// und auf die nächste neue Partner-Seite derselben Referenz wechseln
/// (Queue läuft daneben weiter).
fn commit_quick_partner(app: &mut MiniGramps) {
    let (processed, created) = app.persist_quick_form(true);
    if !processed {
        app.status = "Nichts zu übernehmen".to_string();
        return;
    }
    // Hinter den (ggf. neu) gespeicherten Partner blättern: Ohne-Partner-Seite.
    let pages = app
        .quick_ref_id
        .as_deref()
        .map(|id| app.data.partners_of(id).len())
        .unwrap_or(0);
    app.quick_partner_idx = pages;
    quick_load_reference(app);
    app.status = format!("Schnellerfassung: {} neue Personen", created.len());
}

/// Übern. & Übersp. (nur Button): Eingabe speichern, aber niemanden auf den
/// Stack legen (Verwandte werden übersprungen). Abwärts weiter zur nächsten
/// Partner-Seite (mehrere Partner nacheinander), aufwärts Stand neu laden.
fn commit_quick_skip(app: &mut MiniGramps) {
    use crate::model::QuickDir;
    let (processed, created) = app.persist_quick_form(false);
    let mut advanced = false;
    if app.quick_dir == QuickDir::Down {
        let pages = app
            .quick_ref_id
            .as_deref()
            .map(|id| app.data.partners_of(id).len())
            .unwrap_or(0);
        if app.quick_partner_idx < pages {
            app.quick_partner_idx += 1;
            advanced = true;
        }
    }
    quick_load_reference(app);
    if processed {
        app.status = format!(
            "Schnellerfassung: {} neue Personen (ohne Stack)",
            created.len()
        );
        app.log(app.status.clone());
    } else if advanced {
        app.status = "Nächster Partner".to_string();
    } else {
        app.status = "Nichts zu speichern".to_string();
    }
}

/// Referenz auf den nächsten Stack-Eintrag setzen: (Person, Partner?) —
/// Kombis springen exakt auf die Partner-Seite (Kinder dort sind die des
/// angegebenen Partners), Singles laden normal. Verschwundene überspringen.
/// `created` = Anzahl neu angelegter Personen dieses Schritts.
fn advance_quick_ref(app: &mut MiniGramps, created: usize) {
    use crate::model::QuickDir;
    while let Some((next, partner)) = app.quick_queue.first().cloned() {
        app.quick_queue.remove(0);
        if app.data.find(&next).is_none() {
            continue;
        }
        let name = app
            .data
            .find(&next)
            .map(|person| person.display_name())
            .unwrap_or_else(|| next.clone());
        app.quick_ref_id = Some(next.clone());
        app.quick_visited.insert(next);
        // Kombi-Partner-Seite anspringen (abwärts), sonst erste Seite.
        app.quick_partner_idx = if app.quick_dir == QuickDir::Down {
            partner
                .as_deref()
                .and_then(|pid| {
                    app.data
                        .partners_of(app.quick_ref_id.as_deref().unwrap_or(""))
                        .iter()
                        .position(|person| person.id == pid)
                })
                .unwrap_or(0)
        } else {
            0
        };
        quick_load_reference(app);
        let status = if created > 0 {
            format!("Schnellerfassung: {created} neue Personen — weiter mit {name}")
        } else {
            format!("Weiter mit {name}")
        };
        app.status = match partner
            .as_deref()
            .and_then(|pid| app.data.find(pid))
            .map(|person| person.display_name())
        {
            Some(partner_name) => format!("{status} (+ {partner_name})"),
            None => status,
        };
        app.log(app.status.clone());
        return;
    }
    // Warteschlange leer: Stand derselben Referenz neu laden.
    quick_load_reference(app);
    if created > 0 {
        app.status = format!("Schnellerfassung: {created} neue Personen");
        app.log(app.status.clone());
    }
}

/// Inline-Treffer einer Erfassungszeile: die drei Personen, die am besten
/// zu Vor-/Nachname passen (exakt > Präfix > alle Token > ein Token, dann
/// alphabetisch). Rückgabe: (ID, Anzeigename, Geburtsjahr).
fn quick_inline_matches(
    data: &crate::model::TreeData,
    given: &str,
    family: &str,
) -> Vec<(String, String, String)> {
    let full = format!("{} {}", given.trim(), family.trim())
        .trim()
        .to_lowercase();
    if full.is_empty() {
        return Vec::new();
    }
    let tokens: Vec<&str> = full.split_whitespace().collect();
    let mut scored: Vec<(u8, &crate::model::Person)> = data
        .people
        .iter()
        .filter_map(|person| {
            let hay = person.display_name().to_lowercase();
            let rank = if hay == full {
                0
            } else if hay.starts_with(&full) {
                1
            } else if tokens.iter().all(|token| hay.contains(token)) {
                2
            } else if tokens.iter().any(|token| hay.contains(token)) {
                3
            } else {
                return None;
            };
            Some((rank, person))
        })
        .collect();
    scored.sort_by(|left, right| {
        left.0.cmp(&right.0).then_with(|| {
            left.1.display_name().cmp(&right.1.display_name())
        })
    });
    scored
        .into_iter()
        .take(3)
        .map(|(_, person)| {
            (
                person.id.clone(),
                person.display_name(),
                crate::model::birth_year(&person.birth),
            )
        })
        .collect()
}

/// „Name (Jahr)" für Detailansichten (ohne Jahr nur der Name).
fn quick_detail_label(person: &crate::model::Person) -> String {
    let year = crate::model::birth_year(&person.birth);
    if year.is_empty() {
        person.display_name()
    } else {
        format!("{} ({year})", person.display_name())
    }
}

/// Detailansicht eines Inline-Treffers: Beziehungen der Person (Eltern,
/// Geschwister, Partner, Kinder) plus +-Button, der die Person an die
/// Zeile setzt (`quick_detail` = (Zeile, ID)).
pub fn show_quick_detail(app: &mut MiniGramps, ctx: &egui::Context) {
    let Some((row, id)) = app.quick_detail.clone() else {
        return;
    };
    let Some(person) = app.data.find(&id) else {
        app.quick_detail = None;
        return;
    };
    // Beziehungen einsammeln (owned, damit unten `&mut app` geht).
    let relations = crate::ui::tree::TreeRelations::new(&app.data);
    let parents: Vec<String> = relations
        .parents_of(&id)
        .iter()
        .map(|parent| quick_detail_label(parent))
        .collect();
    let mut sibling_ids: Vec<String> = Vec::new();
    for parent in relations.parents_of(&id) {
        for child in relations.children_of(&parent.id) {
            if child.id != id && !sibling_ids.contains(&child.id) {
                sibling_ids.push(child.id.clone());
            }
        }
    }
    let siblings: Vec<String> = sibling_ids
        .iter()
        .filter_map(|sibling| app.data.find(sibling))
        .map(quick_detail_label)
        .collect();
    let partners: Vec<String> = relations
        .partners_of(&id)
        .iter()
        .map(|partner| quick_detail_label(partner))
        .collect();
    let children: Vec<String> = relations
        .children_of(&id)
        .iter()
        .map(|child| quick_detail_label(child))
        .collect();
    let name = person.display_name();
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
    let gender = crate::ui::picker::gender_label(person.gender).to_string();
    let mut open = true;
    let mut bind_now = false;
    egui::Window::new(window_title("Personendetails"))
        .id(egui::Id::new("quick-detail-v1"))
        .open(&mut open)
        .movable(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(380.0)
        .max_height(ctx.content_rect().height() * 0.8)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(&name).strong());
            ui.small(format!("{gender} · {born} – {died}"));
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
                .button("+ Setzen")
                .on_hover_text("Diese Person an die Zeile setzen (statt neu anlegen)")
                .clicked()
            {
                bind_now = true;
            }
        });
    if bind_now {
        bind_quick_person(app, row, &id);
        app.quick_detail = None;
    }
    if !open {
        app.quick_detail = None;
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
            if ui
                .button("MiniGramps Full (.mfg)")
                .on_hover_text(
                    "Komplettpaket: Daten, Layout, Medien und Einstellungen in einer Datei.",
                )
                .clicked()
            {
                app.export_mfg_dialog(ctx);
            }
            ui.add_enabled(false, egui::Button::new("MiniGramps Mini (.mmg)"));
            ui.separator();
            ui.small("Gramps-, GEDCOM- und Mini-Formate folgen in den nächsten Schritten.");
        });
    app.show_export = app.show_export && open;
}

/// Ladebildschirm für den laufenden MFG-Export (Hintergrundthread): Spinner
/// + Dateiname, blockiert wie andere Modals. Pro Frame wird der
/// Ergebniskanal gepollt (`try_recv` — kein Blockieren); fertig → aufräumen
/// + Status/Log, danach schließt das Fenster von selbst.
pub fn show_export_progress(app: &mut MiniGramps, ctx: &egui::Context) {
    enum Poll {
        Pending,
        Done(Result<(), String>),
        Gone,
    }
    let poll = match app.export_progress.as_ref() {
        None => return,
        Some(progress) => match progress.rx.try_recv() {
            Ok(result) => Poll::Done(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => Poll::Pending,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Poll::Gone,
        },
    };
    match poll {
        Poll::Done(result) => {
            let file_name = app
                .export_progress
                .as_ref()
                .map(|progress| progress.file_name.clone())
                .unwrap_or_default();
            app.export_progress = None;
            match result {
                Ok(()) => {
                    app.status = format!("Exportiert: {file_name}");
                    app.log(format!("Exportiert: {file_name}"));
                }
                Err(error) => {
                    app.status = format!("Export fehlgeschlagen: {error}");
                    app.log(format!("Export fehlgeschlagen: {error}"));
                }
            }
        }
        Poll::Gone => {
            app.export_progress = None;
            app.status = "Export fehlgeschlagen: Hintergrundthread abgebrochen".into();
        }
        Poll::Pending => {
            let file_name = app
                .export_progress
                .as_ref()
                .map(|progress| progress.file_name.clone())
                .unwrap_or_default();
            egui::Window::new(window_title("Export läuft"))
                .movable(false)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .default_width(360.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(28.0));
                        ui.vertical(|ui| {
                            ui.label("Komplettpaket wird geschrieben …");
                            ui.small(&file_name);
                        });
                    });
                    ui.small("Bitte warten — das Fenster schließt automatisch.");
                });
        }
    }
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
                ui.label("Ganzfoto mit Foto");
                ui.add(
                    egui::DragValue::new(&mut app.photo_full_zoom)
                        .range(0.2..=2.0)
                        .speed(0.05),
                )
                .on_hover_text(
                    "Zoom-Faktor fürs Umschalten auf die Ganzfoto-Ansicht bei \
                     Personen MIT Foto (darunter Ganzfoto, darüber Avatar+Text).",
                );
            });
            ui.horizontal(|ui| {
                ui.label("Ganzfoto ohne Foto");
                ui.add(
                    egui::DragValue::new(&mut app.initials_full_zoom)
                        .range(0.2..=2.0)
                        .speed(0.05),
                )
                .on_hover_text(
                    "Dasselbe für Personen OHNE Foto (Initialen-Großansicht).",
                );
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
            ui.horizontal(|ui| {
                ui.label("Nicht-Partner-Abstand");
                ui.add(
                    egui::Slider::new(&mut app.non_partner_gap, 0.0..=150.0)
                        .step_by(5.0)
                        .show_value(true),
                )
                .on_hover_text(
                    "Extra-Abstand im Vorfahrenbaum zwischen Nachbarkarten ohne \
                     Partner-Verbindung (keine gemeinsamen Kinder). Paare mit \
                     gemeinsamen Kindern bleiben kompakt. \
                     0 schaltet das Extra ab.",
                );
            });
            ui.horizontal(|ui| {
                ui.label("Warnstufe");
                ui.selectable_value(&mut app.warn_certainty, None, "Aus");
                ui.selectable_value(
                    &mut app.warn_certainty,
                    Some(crate::model::Certainty::Unset),
                    "Ungesetzt",
                );
                ui.selectable_value(
                    &mut app.warn_certainty,
                    Some(crate::model::Certainty::Oral),
                    "Mündliche Info",
                );
                ui.selectable_value(
                    &mut app.warn_certainty,
                    Some(crate::model::Certainty::Document),
                    "Dokument",
                );
            })
            .response
            .on_hover_text(
                "Sicherheit bis zu dieser Stufe wird in Baum und Seitenleiste \
                 farblich hervorgehoben (Handlungsbedarf).",
            );
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
            // Auswahl per Links/Rechts (Buttons liegen horizontal, zyklisch),
            // Enter bestätigt. Default 0 = Speichern und wechseln
            // (hervorgehoben). Hover folgt der Auswahl.
            let choice = app.pending_select_choice.min(2);
            if ui.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                app.pending_select_choice = (choice + 1) % 3;
            }
            if ui.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                app.pending_select_choice = (choice + 2) % 3;
            }
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !ui.input(|i| i.modifiers.command || i.modifiers.shift);
            let mut activated: Option<usize> = None;
            ui.horizontal(|ui| {
                for (index, label) in [
                    "Speichern und wechseln",
                    "Verwerfen und wechseln",
                    "Abbrechen",
                ]
                .into_iter()
                .enumerate()
                {
                    let active = app.pending_select_choice == index;
                    let response =
                        ui.add(egui::Button::new(label).selected(active));
                    if response.hovered() {
                        app.pending_select_choice = index;
                    }
                    if response.clicked() {
                        activated = Some(index);
                    }
                }
            });
            // Klick (auch nativ per Enter auf fokussiertem Button) schlägt die
            // manuelle Enter-Bestätigung (keine Doppel-Ausführung).
            if activated.is_none() && enter {
                activated = Some(app.pending_select_choice.min(2));
            }
            if let Some(index) = activated {
                let ctx = ui.ctx().clone();
                activate_pending_select(app, &ctx, index);
            }
        });
    if !open {
        app.pending_select = None;
    }
}

/// Wechsel-Dialog-Option ausführen (Button-Klick oder Enter auf Auswahl).
fn activate_pending_select(app: &mut MiniGramps, ctx: &egui::Context, choice: usize) {
    match choice {
        // Speichern und wechseln → Ziel gleich wieder im Bearbeitenmodus.
        0 => {
            app.commit_draft();
            app.inline_edit = false;
            app.status = "Profil gespeichert".into();
            app.save();
            app.apply_pending_select(ctx, true);
        }
        // Verwerfen und wechseln → Ziel in der Ansicht.
        1 => {
            app.inline_edit = false;
            app.relation_picker = None;
            app.status = "Änderungen verworfen".into();
            app.apply_pending_select(ctx, false);
        }
        // Abbrechen.
        _ => {
            app.pending_select = None;
        }
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

/// Bildbearbeitung im Viewer (explizit, auf der Basisdatei).
enum GalleryEdit {
    RotateCw,
    RotateCcw,
    Descreen,
}

/// Galeriebild an der Basis ändern und alle abgeleiteten Thumbs + Caches
/// auffrischen (Profilbild-Ausschnitt wird bei Bedarf zurückgesetzt).
fn apply_gallery_edit(app: &mut MiniGramps, path: &str, op: GalleryEdit) {
    let owner = app
        .data
        .people
        .iter()
        .find(|person| {
            person.photo.as_deref() == Some(path)
                || person.gallery.iter().any(|entry| entry == path)
        })
        .map(|person| person.id.clone());
    let done = match op {
        GalleryEdit::RotateCw => crate::media::rotate_image_file_90(&app.library, path),
        GalleryEdit::RotateCcw => crate::media::rotate_image_file_ccw(&app.library, path),
        GalleryEdit::Descreen => crate::media::descreen_image_file(&app.library, path),
    };
    let label = match op {
        GalleryEdit::RotateCw | GalleryEdit::RotateCcw => "gedreht",
        GalleryEdit::Descreen => "Scanlinien entfernt",
    };
    if !done {
        app.status = format!("Bearbeiten fehlgeschlagen ({label})");
        return;
    }
    if let Some(id) = owner {
        if let Some(person) = app.data.find(&id).cloned() {
            crate::media::delete_person_thumbs(&app.library, &person);
        }
        clear_person_photo_cache(&mut app.photo_cache, &id);
        let mut cropped = false;
        if let Some(person) = app.data.people.iter_mut().find(|person| person.id == id) {
            if person.photo.as_deref() == Some(path) && person.photo_crop.is_some() {
                person.photo_crop = None;
                cropped = true;
            }
        }
        if cropped {
            app.save();
        }
    }
    app.photo_cache.remove(&format!("lightbox-{path}"));
    app.photo_meta_cache.remove(path);
    app.status = format!("Bild {label} — Thumbs werden neu erzeugt");
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
            // Bearbeiten an der Basisdatei (nur explizit hier, nie automatisch
            // in der Pipeline): danach Thumbs + Caches auffrischen.
            egui::CollapsingHeader::new("Bearbeiten")
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.small_button("Links drehen").clicked() {
                            apply_gallery_edit(app, &path, GalleryEdit::RotateCcw);
                        }
                        if ui.small_button("Rechts drehen").clicked() {
                            apply_gallery_edit(app, &path, GalleryEdit::RotateCw);
                        }
                        if ui
                            .small_button("Scanlinien entfernen")
                            .on_hover_text(
                                "Druckraster/Scanlinien aus der Basisdatei rechnen \
                                 (3×3-Median, einmalig — Thumbs werden neu erzeugt).",
                            )
                            .clicked()
                        {
                            apply_gallery_edit(app, &path, GalleryEdit::Descreen);
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

/// Geführter Import-Abgleich (Fixpunkt-Wizard): 1 Fixpunkt → 2 Overlay →
/// 3 Ergänzung → 4 Prüfen → 5 Fertig. Ein Modal, das sauber durchführt:
/// Fixpunkt setzen, Baum automatisch darüberlegen, sichere Infos automatisch
/// ergänzen, nur Unterschiede paarweise entscheiden.
pub fn show_import_wizard(app: &mut MiniGramps, ctx: &egui::Context) {
    if app.import_wizard.is_none() {
        return;
    }
    let step = app
        .import_wizard
        .as_ref()
        .map(|wizard| wizard.step)
        .unwrap_or(WizardStep::Anchor);
    egui::Window::new(window_title("Import-Abgleich"))
        .id(egui::Id::new("import-wizard-v1"))
        .movable(false)
        .resizable(true)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(880.0)
        .max_height(ctx.content_rect().height() * 0.92)
        .show(ctx, |ui| {
            wizard_step_header(ui, step);
            ui.separator();
            match step {
                WizardStep::Anchor => wizard_anchor(app, ui),
                WizardStep::Overlay => wizard_overlay(app, ui),
                WizardStep::Supplement => wizard_supplement(app, ui),
                WizardStep::Review => wizard_review(app, ui),
                WizardStep::Done => wizard_done(app, ui),
            }
        });
}

/// Schritt-Leiste oben im Wizard (aktiver Schritt hervorgehoben).
fn wizard_step_header(ui: &mut egui::Ui, step: WizardStep) {
    let steps = [
        (WizardStep::Anchor, "1 Fixpunkt"),
        (WizardStep::Overlay, "2 Overlay"),
        (WizardStep::Supplement, "3 Ergänzung"),
        (WizardStep::Review, "4 Prüfen"),
        (WizardStep::Done, "5 Fertig"),
    ];
    ui.horizontal(|ui| {
        for (index, (kind, label)) in steps.iter().enumerate() {
            if index > 0 {
                ui.label("→");
            }
            let text = if *kind == step {
                egui::RichText::new(*label).strong()
            } else {
                egui::RichText::new(*label).color(crate::ui::panels::dim_text(ui))
            };
            ui.label(text);
        }
    });
}

/// Personen-Kurzlabel für den Wizard (Name + Geburtsjahr, falls bekannt).
fn wiz_person_label(app: &MiniGramps, id: &str) -> String {
    match app.data.find(id) {
        Some(person) => {
            let birth = person.birth.trim();
            if birth.is_empty() {
                person.display_name()
            } else {
                format!("{} · {birth}", person.display_name())
            }
        }
        None => id.to_string(),
    }
}

/// Score-Zeile eines Kandidaten (Name/Nachname/Verwandtschaft in Prozent).
fn wiz_scores_line(name: f32, family: f32, kin: f32) -> String {
    format!(
        "Name {:.0} % · Nachname {:.0} % · Verwandt {:.0} %",
        name * 100.0,
        family * 100.0,
        kin * 100.0
    )
}

/// Abbrechen-Button (Anhang verwerfen, Stand davor wiederherstellen).
fn wizard_discard_button(app: &mut MiniGramps, ui: &mut egui::Ui) {
    if ui
        .button("Abbrechen (Anhang verwerfen)")
        .on_hover_text(
            "Angehängte Personen samt Einbettungen verwerfen und schließen.",
        )
        .clicked()
    {
        app.wizard_discard();
    }
}

/// Schritt 1: Fixpunkt wählen (Vorschlag vorausgewählt, Suche + Top-Liste).
fn wizard_anchor(app: &mut MiniGramps, ui: &mut egui::Ui) {
    ui.label(
        "Fixpunkt: An welcher Person soll der angehängte Baum ausgerichtet werden? \
         Der Vorschlag ist der sicherste Treffer — oder manuell wählen.",
    );
    ui.add_space(4.0);
    // Suche (filtert die Vorschlagsliste).
    let mut query = app
        .import_wizard
        .as_ref()
        .map(|wizard| wizard.anchor_query.clone())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Suche");
        if ui.text_edit_singleline(&mut query).changed() {
            if let Some(wizard) = app.import_wizard.as_mut() {
                wizard.anchor_query = query.clone();
            }
        }
    });
    // Optionen lesen (gefiltert), Auswahl als IDs.
    let needle = query.trim().to_lowercase();
    let options: Vec<(String, String, String)> = app
        .import_wizard
        .as_ref()
        .map(|wizard| {
            wizard
                .anchor_options
                .iter()
                .filter(|candidate| {
                    if needle.is_empty() {
                        return true;
                    }
                    let keep = app
                        .data
                        .find(&candidate.keep_id)
                        .map(|person| person.display_name().to_lowercase())
                        .unwrap_or_default();
                    let drop = app
                        .data
                        .find(&candidate.drop_id)
                        .map(|person| person.display_name().to_lowercase())
                        .unwrap_or_default();
                    keep.contains(&needle) || drop.contains(&needle)
                })
                .take(12)
                .map(|candidate| {
                    let label = format!(
                        "{}  ↔  {}   ({})",
                        wiz_person_label(app, &candidate.keep_id),
                        wiz_person_label(app, &candidate.drop_id),
                        wiz_scores_line(
                            candidate.name_score,
                            candidate.family_score,
                            candidate.kin_score
                        )
                    );
                    (
                        candidate.keep_id.clone(),
                        candidate.drop_id.clone(),
                        label,
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if options.is_empty() {
        ui.label(
            egui::RichText::new("Keine Treffer — alles wird als neu angehängt.")
                .italics()
                .color(crate::ui::panels::dim_text(ui)),
        );
    } else {
        let selected = app
            .import_wizard
            .as_ref()
            .and_then(|wizard| {
                wizard
                    .anchor_keep
                    .clone()
                    .zip(wizard.anchor_drop.clone())
            });
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .show(ui, |ui| {
                for (keep, drop, label) in &options {
                    let is_selected =
                        selected.as_ref().is_some_and(|(k, d)| k == keep && d == drop);
                    if ui.selectable_label(is_selected, label).clicked() {
                        if let Some(wizard) = app.import_wizard.as_mut() {
                            wizard.anchor_keep = Some(keep.clone());
                            wizard.anchor_drop = Some(drop.clone());
                        }
                    }
                }
            });
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let has_anchor = app
            .import_wizard
            .as_ref()
            .is_some_and(|wizard| wizard.anchor_keep.is_some() && wizard.anchor_drop.is_some());
        if ui
            .add_enabled(has_anchor, egui::Button::new("Weiter: Baum abgleichen"))
            .on_hover_text("Fixpunkt übernehmen und 1:1-Overlay aufbauen.")
            .clicked()
            && has_anchor
        {
            app.wizard_confirm_anchor();
        }
        if options.is_empty() && ui.button("Weiter ohne Fixpunkt").clicked() {
            app.wizard_skip_anchor();
        }
        wizard_discard_button(app, ui);
    });
}

/// Schritt 2: Overlay prüfen (1:1-Mapping ab Fixpunkt, Status je Paar).
fn wizard_overlay(app: &mut MiniGramps, ui: &mut egui::Ui) {
    let (exact, unsure, fresh_total, fresh_mapped) = app
        .import_wizard
        .as_ref()
        .map(|wizard| {
            (
                wizard.mappings.iter().filter(|entry| entry.exact).count(),
                wizard.mappings.iter().filter(|entry| !entry.exact).count(),
                wizard.fresh_total,
                wizard
                    .mappings
                    .iter()
                    .map(|entry| entry.drop_id.clone())
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default();
    let fresh_new = app
        .import_wizard
        .as_ref()
        .map(|wizard| {
            wizard
                .fresh_ids
                .iter()
                .filter(|id| !fresh_mapped.contains(id))
                .count()
        })
        .unwrap_or(0);
    ui.label(format!(
        "{exact} sicher (automatisch) · {unsure} zu prüfen · {fresh_new} neu von {fresh_total} — \
         jede Person höchstens einmal zugeordnet."
    ));
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .max_height(300.0)
        .show(ui, |ui| {
            let rows: Vec<(bool, String, String)> = app
                .import_wizard
                .as_ref()
                .map(|wizard| {
                    wizard
                        .mappings
                        .iter()
                        .map(|entry| {
                            (
                                entry.exact,
                                wiz_person_label(app, &entry.keep_id),
                                wiz_person_label(app, &entry.drop_id),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            for (exact, keep_label, drop_label) in &rows {
                ui.horizontal(|ui| {
                    let chip = if *exact { "Sicher" } else { "Prüfen" };
                    ui.label(
                        egui::RichText::new(chip)
                            .small()
                            .color(crate::ui::panels::dim_text(ui)),
                    );
                    ui.label(format!("{keep_label}  ↔  {drop_label}"));
                });
            }
            if rows.is_empty() {
                ui.label(
                    egui::RichText::new("Keine Zuordnungen — alles bleibt neu.")
                        .italics()
                        .color(crate::ui::panels::dim_text(ui)),
                );
            }
        });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("Zurück").clicked() {
            if let Some(wizard) = app.import_wizard.as_mut() {
                wizard.step = WizardStep::Anchor;
            }
        }
        if ui
            .button("Weiter: automatisch ergänzen")
            .on_hover_text("Sichere Paare einbetten (nur Lücken füllen, mit Protokoll).")
            .clicked()
        {
            app.wizard_run_supplement();
        }
        wizard_discard_button(app, ui);
    });
}

/// Schritt 3: Auto-Ergänzung prüfen (Protokoll der übernommenen Infos).
fn wizard_supplement(app: &mut MiniGramps, ui: &mut egui::Ui) {
    let (auto_count, protocol, unsure) = app
        .import_wizard
        .as_ref()
        .map(|wizard| {
            (
                wizard.auto_count,
                wizard.protocol.clone(),
                wizard.mappings.len(),
            )
        })
        .unwrap_or_default();
    ui.label(format!(
        "{auto_count} Paare automatisch eingebettet — nur Lücken gefüllt, nichts überschrieben."
    ));
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .max_height(300.0)
        .show(ui, |ui| {
            if protocol.is_empty() {
                ui.label(
                    egui::RichText::new("Nichts zu ergänzen.")
                        .italics()
                        .color(crate::ui::panels::dim_text(ui)),
                );
            } else {
                for line in &protocol {
                    ui.small(format!("• {line}"));
                }
            }
        });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if unsure > 0 {
            if ui
                .button(format!("Weiter: Unterschiede prüfen ({unsure})"))
                .on_hover_text("Unsichere Paare einzeln durchgehen und entscheiden.")
                .clicked()
            {
                if let Some(wizard) = app.import_wizard.as_mut() {
                    wizard.review_index = 0;
                    wizard.step = WizardStep::Review;
                }
            }
        } else if ui.button("Fertig").clicked() {
            app.wizard_finish();
        }
        wizard_discard_button(app, ui);
    });
}

/// Rel-Schlüssel für Überspringen (Behalten): stabil je Unterschied.
fn wiz_rel_key(kind: &RelDiffKind, family_drop_id: &str) -> String {
    match kind {
        RelDiffKind::PartnerRelation { .. } => format!("{family_drop_id}:partner"),
        RelDiffKind::RelationEvent { event, .. } => format!(
            "{family_drop_id}:event:{}:{}:{}",
            event.kind.label(),
            event.date,
            event.place
        ),
        RelDiffKind::FamilyNote { .. } => format!("{family_drop_id}:famnote"),
        RelDiffKind::FamilySources { .. } => format!("{family_drop_id}:famsources"),
        RelDiffKind::ChildMissing { child_id, .. } => {
            format!("{family_drop_id}:child:{child_id}")
        }
        RelDiffKind::ChildRelation { child_id, .. } => {
            format!("{family_drop_id}:childrel:{child_id}")
        }
        RelDiffKind::NewFamily => format!("{family_drop_id}:newfamily"),
    }
}

/// Schritt 4: Unterschiede paarweise prüfen — Person für Person, nur
/// Unterschiede; jede Änderung einzeln annehmen oder ablehnen.
/// Beziehungsunterschiede bieten Ersetzen/Ergänzen/Zusammenführen.
fn wizard_review(app: &mut MiniGramps, ui: &mut egui::Ui) {
    let order = app.wizard_review_order();
    if order.is_empty() {
        ui.label("Keine unsicheren Paare — alles eingebettet oder neu.");
        ui.horizontal(|ui| {
            if ui.button("Fertig").clicked() {
                app.wizard_finish();
            }
            wizard_discard_button(app, ui);
        });
        return;
    }
    let len = order.len();
    let index = app
        .import_wizard
        .as_ref()
        .map(|wizard| wizard.review_index.min(len - 1))
        .unwrap_or(0);
    if app
        .import_wizard
        .as_ref()
        .is_some_and(|wizard| wizard.review_index != index)
    {
        app.import_wizard.as_mut().unwrap().review_index = index;
    }
    let (keep_id, drop_id) = order[index].clone();
    let (keep, drop) = match (
        app.data.find(&keep_id).cloned(),
        app.data.find(&drop_id).cloned(),
    ) {
        (Some(keep), Some(drop)) => (keep, drop),
        _ => {
            ui.label("Paar aufgelöst — weiter.");
            ui.horizontal(|ui| {
                if ui.button("Weiter →").clicked() {
                    wizard_review_advance(app, len);
                }
                wizard_discard_button(app, ui);
            });
            return;
        }
    };
    // Scores für die Kopfzeile.
    let scores = app
        .import_wizard
        .as_ref()
        .and_then(|wizard| {
            wizard
                .mappings
                .iter()
                .find(|entry| entry.keep_id == keep_id && entry.drop_id == drop_id)
        })
        .map(|entry| wiz_scores_line(entry.name_score, entry.family_score, entry.kin_score))
        .unwrap_or_default();
    ui.label(
        egui::RichText::new(format!(
            "Paar {} von {} — {}  ↔  {}",
            index + 1,
            len,
            keep.display_name(),
            drop.display_name()
        ))
        .strong(),
    );
    ui.small(&scores);
    // Diffs berechnen (Eigentum für borrow-freies Rendern).
    let scalars = crate::model::TreeData::person_scalar_diffs(&keep, &drop);
    let missing_events: Vec<crate::model::Event> = drop
        .events
        .iter()
        .filter(|event| {
            !keep.events.iter().any(|own| {
                own.kind == event.kind && own.date == event.date && own.place == event.place
            })
        })
        .cloned()
        .collect();
    let keep_only_events = keep
        .events
        .iter()
        .filter(|event| {
            !drop.events.iter().any(|other| {
                other.kind == event.kind
                    && other.date == event.date
                    && other.place == event.place
            })
        })
        .count();
    let missing_gallery: Vec<String> = drop
        .gallery
        .iter()
        .filter(|path| !keep.gallery.iter().any(|own| own == *path))
        .cloned()
        .collect();
    let missing_docs: Vec<crate::model::DocumentEntry> = drop
        .documents
        .iter()
        .filter(|document| {
            !keep
                .documents
                .iter()
                .any(|own| own.path == document.path)
        })
        .cloned()
        .collect();
    let missing_sources: Vec<crate::model::SourceEntry> = drop
        .sources
        .iter()
        .filter(|source| !keep.sources.contains(source))
        .cloned()
        .collect();
    let missing_alts: Vec<crate::model::AlternativeName> = drop
        .alt_names
        .iter()
        .filter(|alt| !alt.is_empty() && !keep.alt_names.contains(alt))
        .cloned()
        .collect();
    let drop_to_keep = app.wizard_drop_to_keep();
    let fresh_ids = app
        .import_wizard
        .as_ref()
        .map(|wizard| wizard.fresh_ids.clone())
        .unwrap_or_default();
    let rels = app
        .data
        .relation_diffs(&keep_id, &drop_id, &drop_to_keep, &fresh_ids);
    // Übersprungene ausblenden (Behalten).
    let skipped_scalars: Vec<(String, String)> = app
        .import_wizard
        .as_ref()
        .map(|wizard| {
            wizard
                .skipped_scalars
                .iter()
                .filter(|(drop, _)| drop == &drop_id)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let skipped_lists: Vec<(String, String, String)> = app
        .import_wizard
        .as_ref()
        .map(|wizard| {
            wizard
                .skipped_lists
                .iter()
                .filter(|(drop, _, _)| drop == &drop_id)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let skipped_rels: Vec<String> = app
        .import_wizard
        .as_ref()
        .map(|wizard| wizard.skipped_rels.clone())
        .unwrap_or_default();
    // Offene zählen (ohne reine Anzeige-Zeilen).
    let open_scalars = scalars
        .iter()
        .filter(|diff| {
            !diff.new_text.trim().is_empty()
                && !skipped_scalars
                    .iter()
                    .any(|(_, key)| key == diff.key)
        })
        .count();
    let open_lists = missing_events.len()
        + missing_gallery.len()
        + missing_docs.len()
        + missing_sources.len()
        + missing_alts.len()
        - skipped_lists.len().min(
            missing_events.len()
                + missing_gallery.len()
                + missing_docs.len()
                + missing_sources.len()
                + missing_alts.len(),
        );
    let open_rels = rels
        .iter()
        .filter(|diff| {
            !matches!(diff.kind, RelDiffKind::NewFamily)
                && !skipped_rels.contains(&wiz_rel_key(&diff.kind, &diff.family_drop_id))
        })
        .count();
    ui.small(format!(
        "{} offen (Person: {open_scalars}, Listen: {open_lists}, Beziehung: {open_rels})",
        open_scalars + open_lists + open_rels
    ));
    egui::ScrollArea::vertical()
        .max_height(380.0)
        .show(ui, |ui| {
            wizard_person_diffs(app, ui, &keep_id, &drop_id, &keep, &scalars, &skipped_scalars);
            wizard_list_diffs(
                app,
                ui,
                &keep_id,
                &drop_id,
                &keep,
                &missing_events,
                keep_only_events,
                &missing_gallery,
                &missing_docs,
                &missing_sources,
                &missing_alts,
                &skipped_lists,
            );
            wizard_rel_diffs(
                app,
                ui,
                &keep_id,
                &drop_id,
                &rels,
                &skipped_rels,
            );
        });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(index > 0, egui::Button::new("← Zurück"))
            .clicked()
            && index > 0
        {
            if let Some(wizard) = app.import_wizard.as_mut() {
                wizard.review_index = index - 1;
            }
        }
        if index + 1 < len {
            if ui.button("Weiter →").clicked() {
                if let Some(wizard) = app.import_wizard.as_mut() {
                    wizard.review_index = index + 1;
                }
            }
        } else if ui.button("Fertig").clicked() {
            if let Some(wizard) = app.import_wizard.as_mut() {
                wizard.step = WizardStep::Done;
            }
        }
        wizard_discard_button(app, ui);
    });
}

/// Review fortsetzen (nach aufgelöstem Paar).
fn wizard_review_advance(app: &mut MiniGramps, len: usize) {
    let next = app
        .import_wizard
        .as_ref()
        .map(|wizard| wizard.review_index + 1)
        .unwrap_or(0);
    if next >= len {
        if let Some(wizard) = app.import_wizard.as_mut() {
            wizard.step = WizardStep::Done;
        }
    } else if let Some(wizard) = app.import_wizard.as_mut() {
        wizard.review_index = next;
    }
}

/// Personen-Felder: nur Unterschiede, je Zeile Übernehmen/Behalten (leeres
/// Neu = nur Anzeige, Übernehmen würde sonst löschen).
#[allow(clippy::too_many_arguments)]
fn wizard_person_diffs(
    app: &mut MiniGramps,
    ui: &mut egui::Ui,
    keep_id: &str,
    drop_id: &str,
    keep: &crate::model::Person,
    scalars: &[crate::model::ScalarDiff],
    skipped: &[(String, String)],
) {
    ui.label(egui::RichText::new("PERSON").strong());
    let mut shown = false;
    for diff in scalars {
        if skipped.iter().any(|(_, key)| key == diff.key) {
            continue;
        }
        shown = true;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(diff.label).strong());
            ui.label(&diff.keep_text);
            ui.label("→");
            ui.label(&diff.new_text);
            if diff.new_text.trim().is_empty() {
                ui.small("(nur im Bestand)");
            } else {
                if ui.small_button("Übernehmen").clicked() {
                    app.data
                        .apply_scalar_diff(keep_id, diff.key, &diff.new_text);
                    wizard_protocol(
                        app,
                        format!(
                            "{}: {} übernommen ({})",
                            keep.display_name(),
                            diff.label,
                            diff.new_text.trim()
                        ),
                    );
                }
                if ui.small_button("Behalten").clicked() {
                    if let Some(wizard) = app.import_wizard.as_mut() {
                        wizard
                            .skipped_scalars
                            .push((drop_id.to_string(), diff.key.to_string()));
                    }
                }
            }
        });
    }
    if !shown {
        ui.small("Keine Feldunterschiede.");
    }
}

/// Listen-Diffs: fehlende Ereignisse/Medien/Dokumente/Quellen/Namen je
/// Eintrag Hinzufügen/Behalten; reine Bestands-Einträge nur als Hinweis.
#[allow(clippy::too_many_arguments)]
fn wizard_list_diffs(
    app: &mut MiniGramps,
    ui: &mut egui::Ui,
    keep_id: &str,
    drop_id: &str,
    keep: &crate::model::Person,
    missing_events: &[crate::model::Event],
    keep_only_events: usize,
    missing_gallery: &[String],
    missing_docs: &[crate::model::DocumentEntry],
    missing_sources: &[crate::model::SourceEntry],
    missing_alts: &[crate::model::AlternativeName],
    skipped: &[(String, String, String)],
) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new("ERGÄNZUNGEN").strong());
    let skipped_here = |kind: &str, ident: &str| {
        skipped
            .iter()
            .any(|(_, kind2, ident2)| kind2 == kind && ident2 == ident)
    };
    let mut shown = false;
    for event in missing_events {
        let ident = format!(
            "{}:{}:{}",
            event.kind.label(),
            event.date,
            event.place
        );
        if skipped_here("event", &ident) {
            continue;
        }
        shown = true;
        ui.horizontal(|ui| {
            ui.label(format!(
                "Ereignis: {} {}",
                event.kind.label(),
                crate::ui::picker::dated_place(&event.date, &event.place)
            ));
            if ui.small_button("Hinzufügen").clicked() {
                if let Some(person) = app
                    .data
                    .people
                    .iter_mut()
                    .find(|person| person.id == keep_id)
                {
                    person.events.push(event.clone());
                }
                wizard_protocol(
                    app,
                    format!(
                        "{}: Ereignis {} ergänzt",
                        keep.display_name(),
                        event.kind.label()
                    ),
                );
            }
            if ui.small_button("Behalten").clicked() {
                if let Some(wizard) = app.import_wizard.as_mut() {
                    wizard.skipped_lists.push((
                        drop_id.to_string(),
                        "event".to_string(),
                        ident,
                    ));
                }
            }
        });
    }
    for path in missing_gallery {
        if skipped_here("gallery", path) {
            continue;
        }
        shown = true;
        ui.horizontal(|ui| {
            ui.label(format!("Bild: {path}"));
            if ui.small_button("Hinzufügen").clicked() {
                if let Some(person) = app
                    .data
                    .people
                    .iter_mut()
                    .find(|person| person.id == keep_id)
                {
                    person.gallery.push(path.clone());
                }
                wizard_protocol(app, format!("{}: Bild ergänzt", keep.display_name()));
            }
            if ui.small_button("Behalten").clicked() {
                if let Some(wizard) = app.import_wizard.as_mut() {
                    wizard.skipped_lists.push((
                        drop_id.to_string(),
                        "gallery".to_string(),
                        path.clone(),
                    ));
                }
            }
        });
    }
    for document in missing_docs {
        if skipped_here("doc", &document.path) {
            continue;
        }
        shown = true;
        ui.horizontal(|ui| {
            ui.label(format!("Dokument: {} ({})", document.name, document.path));
            if ui.small_button("Hinzufügen").clicked() {
                if let Some(person) = app
                    .data
                    .people
                    .iter_mut()
                    .find(|person| person.id == keep_id)
                {
                    person.documents.push(document.clone());
                }
                wizard_protocol(
                    app,
                    format!("{}: Dokument ergänzt", keep.display_name()),
                );
            }
            if ui.small_button("Behalten").clicked() {
                if let Some(wizard) = app.import_wizard.as_mut() {
                    wizard.skipped_lists.push((
                        drop_id.to_string(),
                        "doc".to_string(),
                        document.path.clone(),
                    ));
                }
            }
        });
    }
    for source in missing_sources {
        if skipped_here("source", &source.title) {
            continue;
        }
        shown = true;
        ui.horizontal(|ui| {
            ui.label(format!("Quelle: {}", source.title));
            if ui.small_button("Hinzufügen").clicked() {
                if let Some(person) = app
                    .data
                    .people
                    .iter_mut()
                    .find(|person| person.id == keep_id)
                {
                    person.sources.push(source.clone());
                }
                wizard_protocol(
                    app,
                    format!("{}: Quelle ergänzt", keep.display_name()),
                );
            }
            if ui.small_button("Behalten").clicked() {
                if let Some(wizard) = app.import_wizard.as_mut() {
                    wizard.skipped_lists.push((
                        drop_id.to_string(),
                        "source".to_string(),
                        source.title.clone(),
                    ));
                }
            }
        });
    }
    for alt in missing_alts {
        let ident = alt.display();
        if skipped_here("alt", &ident) {
            continue;
        }
        shown = true;
        ui.horizontal(|ui| {
            ui.label(format!("Alternativname: {ident}"));
            if ui.small_button("Hinzufügen").clicked() {
                if let Some(person) = app
                    .data
                    .people
                    .iter_mut()
                    .find(|person| person.id == keep_id)
                {
                    person.alt_names.push(alt.clone());
                }
                wizard_protocol(
                    app,
                    format!("{}: Alternativname ergänzt", keep.display_name()),
                );
            }
            if ui.small_button("Behalten").clicked() {
                if let Some(wizard) = app.import_wizard.as_mut() {
                    wizard.skipped_lists.push((
                        drop_id.to_string(),
                        "alt".to_string(),
                        ident,
                    ));
                }
            }
        });
    }
    if keep_only_events > 0 {
        ui.small(format!(
            "Nur im Bestand: {keep_only_events} Ereignis(se) — bleibt unverändert."
        ));
    }
    if !shown {
        ui.small("Keine Ergänzungen.");
    }
}

/// Beziehungs-Diffs: je Unterschied Ersetzen/Ergänzen/Zusammenführen/
/// Hinzufügen oder Behalten (neue Familien nur Anzeige).
fn wizard_rel_diffs(
    app: &mut MiniGramps,
    ui: &mut egui::Ui,
    keep_id: &str,
    drop_id: &str,
    rels: &[crate::model::RelDiff],
    skipped: &[String],
) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new("BEZIEHUNG").strong());
    let keep_name = wiz_person_label(app, keep_id);
    let drop_name = wiz_person_label(app, drop_id);
    let mut shown = false;
    for diff in rels {
        let key = wiz_rel_key(&diff.kind, &diff.family_drop_id);
        if skipped.contains(&key) {
            continue;
        }
        match &diff.kind {
            RelDiffKind::NewFamily => {
                ui.small("Neue Familie im Anhang — bleibt angehängt.");
            }
            RelDiffKind::PartnerRelation { keep, new } => {
                shown = true;
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "Beziehungsart: {} → {}",
                        keep.label(),
                        new.label()
                    ));
                    let can_set = *new != PartnerRelation::Unknown;
                    let can_add = *keep == PartnerRelation::Unknown && can_set;
                    if ui
                        .add_enabled(can_set, egui::Button::new("Ersetzen"))
                        .on_hover_text("Beziehungsart durch die importierte ersetzen.")
                        .clicked()
                        && can_set
                    {
                        if let Some(family) = diff.family_keep_id.clone() {
                            app.data.apply_family_partner_relation(&family, *new);
                            wizard_hide_rel(app, &key);
                            wizard_protocol(
                                app,
                                format!("{keep_name}: Beziehungsart → {}", new.label()),
                            );
                        }
                    }
                    if ui
                        .add_enabled(can_add, egui::Button::new("Ergänzen"))
                        .on_hover_text("Nur setzen, weil bisher unbekannt.")
                        .clicked()
                        && can_add
                    {
                        if let Some(family) = diff.family_keep_id.clone() {
                            app.data.apply_family_partner_relation(&family, *new);
                            wizard_hide_rel(app, &key);
                            wizard_protocol(
                                app,
                                format!("{keep_name}: Beziehungsart ergänzt ({})", new.label()),
                            );
                        }
                    }
                    if ui.small_button("Behalten").clicked() {
                        wizard_hide_rel(app, &key);
                    }
                });
            }
            RelDiffKind::RelationEvent { event, keep_event } => {
                shown = true;
                ui.label(format!(
                    "{}: {}",
                    event.kind.label(),
                    crate::ui::picker::dated_place(&event.date, &event.place)
                ));
                if let Some(have) = keep_event {
                    ui.small(format!(
                        "Bestand: {}",
                        crate::ui::picker::dated_place(&have.date, &have.place)
                    ));
                }
                ui.horizontal(|ui| {
                    if ui
                        .small_button("Ersetzen")
                        .on_hover_text("Datum/Ort/Notiz/Quellen vom Import übernehmen.")
                        .clicked()
                    {
                        if let Some(family) = diff.family_keep_id.clone() {
                            app.data.apply_relation_event(
                                &family,
                                keep_id,
                                event,
                                RelationEventMode::Replace,
                            );
                            wizard_hide_rel(app, &key);
                            wizard_protocol(
                                app,
                                format!(
                                    "{keep_name}: {} ersetzt ({}, {})",
                                    event.kind.label(),
                                    event.date,
                                    event.place
                                ),
                            );
                        }
                    }
                    if ui
                        .small_button("Ergänzen")
                        .on_hover_text("Als zusätzliches Ereignis daneben anlegen.")
                        .clicked()
                    {
                        if let Some(family) = diff.family_keep_id.clone() {
                            app.data.apply_relation_event(
                                &family,
                                keep_id,
                                event,
                                RelationEventMode::Add,
                            );
                            wizard_hide_rel(app, &key);
                            wizard_protocol(
                                app,
                                format!(
                                    "{keep_name}: {} ergänzt ({}, {})",
                                    event.kind.label(),
                                    event.date,
                                    event.place
                                ),
                            );
                        }
                    }
                    if ui
                        .small_button("Zusammenführen")
                        .on_hover_text("Datum/Ort behalten, Notizen/Quellen vereinen.")
                        .clicked()
                    {
                        if let Some(family) = diff.family_keep_id.clone() {
                            app.data.apply_relation_event(
                                &family,
                                keep_id,
                                event,
                                RelationEventMode::Merge,
                            );
                            wizard_hide_rel(app, &key);
                            wizard_protocol(
                                app,
                                format!(
                                    "{keep_name}: {} zusammengeführt",
                                    event.kind.label()
                                ),
                            );
                        }
                    }
                    if ui.small_button("Behalten").clicked() {
                        wizard_hide_rel(app, &key);
                    }
                });
            }
            RelDiffKind::FamilyNote { keep, new } => {
                shown = true;
                ui.label(format!("Familiennotiz (Anhang): {new}"));
                if let Some(have) = keep {
                    ui.small(format!("Bestand: {have}"));
                }
                ui.horizontal(|ui| {
                    if ui
                        .small_button("Ersetzen")
                        .on_hover_text("Bestandsnotiz überschreiben.")
                        .clicked()
                    {
                        if let Some(family) = diff.family_keep_id.clone() {
                            app.data.apply_family_note(&family, new, true);
                            wizard_hide_rel(app, &key);
                            wizard_protocol(app, format!("{keep_name}: Familiennotiz ersetzt"));
                        }
                    }
                    if ui
                        .small_button("Ergänzen")
                        .on_hover_text("Anhang-Notiz dazuschreiben.")
                        .clicked()
                    {
                        if let Some(family) = diff.family_keep_id.clone() {
                            app.data.apply_family_note(&family, new, false);
                            wizard_hide_rel(app, &key);
                            wizard_protocol(app, format!("{keep_name}: Familiennotiz ergänzt"));
                        }
                    }
                    if ui.small_button("Behalten").clicked() {
                        wizard_hide_rel(app, &key);
                    }
                });
            }
            RelDiffKind::FamilySources { missing } => {
                shown = true;
                let titles: Vec<String> =
                    missing.iter().map(|source| source.title.clone()).collect();
                ui.label(format!("Familienquellen (Anhang): {}", titles.join(", ")));
                ui.horizontal(|ui| {
                    if ui.small_button("Alle übernehmen").clicked() {
                        if let Some(family) = diff.family_keep_id.clone() {
                            app.data.apply_family_sources(&family, missing);
                            wizard_hide_rel(app, &key);
                            wizard_protocol(
                                app,
                                format!("{keep_name}: Familienquellen übernommen"),
                            );
                        }
                    }
                    if ui.small_button("Behalten").clicked() {
                        wizard_hide_rel(app, &key);
                    }
                });
            }
            RelDiffKind::ChildMissing {
                child_id,
                child_name,
            } => {
                shown = true;
                ui.horizontal(|ui| {
                    ui.label(format!("Kind fehlt im Bestand: {child_name}"));
                    if ui
                        .small_button("Hinzufügen")
                        .on_hover_text("Kind in die Bestands-Familie aufnehmen.")
                        .clicked()
                    {
                        if let Some(family) = diff.family_keep_id.clone() {
                            app.data.apply_add_child(
                                &family,
                                &diff.family_drop_id,
                                child_id,
                            );
                            wizard_hide_rel(app, &key);
                            wizard_protocol(
                                app,
                                format!("{keep_name}: Kind {child_name} aufgenommen"),
                            );
                        }
                    }
                    if ui.small_button("Behalten").clicked() {
                        wizard_hide_rel(app, &key);
                    }
                });
            }
            RelDiffKind::ChildRelation {
                child_id,
                child_name,
                keep,
                new,
            } => {
                shown = true;
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{child_name}: {} → {}",
                        keep.label(),
                        new.label()
                    ));
                    if ui
                        .small_button("Ersetzen")
                        .on_hover_text("Kind-Art aus dem Anhang übernehmen.")
                        .clicked()
                    {
                        if let Some(family) = diff.family_keep_id.clone() {
                            app.data.apply_child_relation(&family, child_id, *new);
                            wizard_hide_rel(app, &key);
                            wizard_protocol(
                                app,
                                format!("{keep_name}: {child_name} → {}", new.label()),
                            );
                        }
                    }
                    if ui.small_button("Behalten").clicked() {
                        wizard_hide_rel(app, &key);
                    }
                });
            }
        }
    }
    if !shown {
        ui.small("Keine Beziehungsunterschiede.");
    }
    let _ = drop_name;
}

/// Rel-Diff ausblenden (Behalten oder angewandt).
fn wizard_hide_rel(app: &mut MiniGramps, key: &str) {
    if let Some(wizard) = app.import_wizard.as_mut() {
        wizard.skipped_rels.push(key.to_string());
    }
}

/// Protokollzeile für den Abschluss-Schritt anhängen.
fn wizard_protocol(app: &mut MiniGramps, line: String) {
    app.log(format!("Abgleich: {line}"));
    if let Some(wizard) = app.import_wizard.as_mut() {
        wizard.protocol.push(line);
    }
}

/// Schritt 5: Abschluss — Zähler + Protokoll, Fertig oder Verwerfen.
fn wizard_done(app: &mut MiniGramps, ui: &mut egui::Ui) {
    let (auto_count, review_total, fresh_left, fresh_total, protocol) = app
        .import_wizard
        .as_ref()
        .map(|wizard| {
            (
                wizard.auto_count,
                wizard.mappings.len(),
                wizard.fresh_ids.len(),
                wizard.fresh_total,
                wizard.protocol.clone(),
            )
        })
        .unwrap_or_default();
    ui.label(format!(
        "{auto_count} automatisch eingebettet · {review_total} geprüft · {fresh_left} neu von {fresh_total}."
    ));
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .max_height(300.0)
        .show(ui, |ui| {
            if protocol.is_empty() {
                ui.label(
                    egui::RichText::new("Keine Änderungen protokolliert.")
                        .italics()
                        .color(crate::ui::panels::dim_text(ui)),
                );
            } else {
                for line in &protocol {
                    ui.small(format!("• {line}"));
                }
            }
        });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("Fertig & schließen").clicked() {
            app.wizard_finish();
        }
        wizard_discard_button(app, ui);
    });
}
