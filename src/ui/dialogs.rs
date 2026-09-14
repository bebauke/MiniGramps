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
            // Beide Wertspalten teilen sich die Dialogbreite (Label- +
            // Radiospalten abgezogen, je min. 220); kein oberer Deckel —
            // lange Namen/Daten stehen voll da, Rest scrollt in der Zelle.
            let cell_max = ((ui.available_width() - 130.0) / 2.0).max(220.0);
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
                                let dates =
                                    |person: Option<&crate::model::Person>| -> (String, String) {
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
                                let relations =
                                    |person: Option<&crate::model::Person>| -> [String; 4] {
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
                                let names = format!(
                                    "{} ↔ {}",
                                    name(keep, &candidate.keep_id),
                                    name(drop, &candidate.drop_id),
                                );
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
                                let rows: Vec<FieldRow> = fields
                                    .into_iter()
                                    .map(|(label, is_choice, new_text, old_text)| FieldRow {
                                        label: label.to_string(),
                                        is_choice,
                                        new_text,
                                        old_text,
                                    })
                                    .collect();
                                (
                                    candidate.drop_id.clone(),
                                    names,
                                    scores,
                                    entry.selected,
                                    entry.take_new_birth,
                                    entry.take_new_death,
                                    rows,
                                )
                            });
                        let Some((drop_id, names, scores, selected, take_birth, take_death, rows)) =
                            rows
                        else {
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
                            // Lange Feldwerte (v. a. Verwandtschaftslisten) bekommen
                            // höchstens ihren Anteil der Dialogbreite; der Rest
                            // scrollt horizontal in der Zelle allein.
                            let scroll_cell = |ui: &mut egui::Ui, text: &str| {
                                egui::ScrollArea::horizontal()
                                    .max_width(cell_max)
                                    .show(ui, |ui| {
                                        ui.label(text);
                                    });
                            };
                            egui::Grid::new(("merge-fields", drop_id.clone()))
                                .num_columns(5)
                                .striped(true)
                                .show(ui, |ui| {
                                    ui.label("");
                                    ui.label(egui::RichText::new("Neu").strong());
                                    ui.label("");
                                    ui.label(egui::RichText::new("Vorhanden").strong());
                                    ui.label("");
                                    ui.end_row();
                                    for (row_index, row) in rows.iter().enumerate() {
                                        ui.label(&row.label);
                                        scroll_cell(ui, &row.new_text);
                                        let current = if row_index == 0 {
                                            take_birth
                                        } else {
                                            take_death
                                        };
                                        if row.is_choice {
                                            let mut take = current;
                                            ui.radio_value(&mut take, true, "");
                                            if take != current {
                                                if let Some(review) = app.merge_review.as_mut() {
                                                    if let Some(entry) =
                                                        review.candidates.get_mut(index)
                                                    {
                                                        if row_index == 0 {
                                                            entry.take_new_birth = take;
                                                        } else {
                                                            entry.take_new_death = take;
                                                        }
                                                    }
                                                }
                                            }
                                        } else {
                                            ui.label("");
                                        }
                                        scroll_cell(ui, &row.old_text);
                                        if row.is_choice {
                                            let mut take = current;
                                            ui.radio_value(&mut take, false, "");
                                            if take != current {
                                                if let Some(review) = app.merge_review.as_mut() {
                                                    if let Some(entry) =
                                                        review.candidates.get_mut(index)
                                                    {
                                                        if row_index == 0 {
                                                            entry.take_new_birth = take;
                                                        } else {
                                                            entry.take_new_death = take;
                                                        }
                                                    }
                                                }
                                            }
                                        } else {
                                            ui.label("");
                                        }
                                        ui.end_row();
                                    }
                                });
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
        .id(egui::Id::new("quick-entry-v1"))
        .open(&mut open)
        .movable(false)
        .resizable(true)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(560.0)
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
    // Gesetzte Zeile: Felder zeigen die Personeninfos, grau + gesperrt.
    let bound = person.bind.is_some();
    let mut unset = false;
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
            let response = if bound {
                ui.add_enabled(false, edit)
            } else {
                ui.add(edit)
            };
            if !bound && focus == Some((row, col)) {
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
        // Hinten: Entsetzen-Button bei gesetzter Zeile.
        if bound
            && ui
                .small_button("entsetzen")
                .on_hover_text("Bindung lösen — Zeile wieder frei erfassen")
                .clicked()
        {
            unset = true;
        }
    });
    if unset {
        unbind_quick_person(app, row);
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
}

/// Übernehmen (Strg+Enter bzw. Button): Eingabe sofort speichern und mit
/// der nächsten eingetragenen Person weitermachen (neu oder gesetzt — alle
/// landen in Zeilenreihenfolge auf der FIFO-`quick_queue`, außer bereits
/// abgearbeiteten; die Referenz springt der Reihe nach darauf, ohne
/// Baum-Umweg). Leere Eingabe springt nur weiter; ohne Warteschlange wird
/// der Referenzstand neu geladen.
fn commit_quick_block(app: &mut MiniGramps) {
    let (processed, created, touched) = app.persist_quick_form();
    for id in touched {
        if !app.quick_visited.contains(&id) && !app.quick_queue.contains(&id) {
            app.quick_queue.push(id);
        }
    }
    if !processed && app.quick_queue.is_empty() {
        app.status = "Nichts zu übernehmen".to_string();
        return;
    }
    advance_quick_ref(app, created.len());
}

/// Neuer Partner (Umsch+Strg+Enter bzw. Button): Eingabe sofort speichern
/// und auf die nächste neue Partner-Seite derselben Referenz wechseln.
/// Eingetragene IDs landen trotzdem auf der FIFO-Queue.
fn commit_quick_partner(app: &mut MiniGramps) {
    let (processed, created, touched) = app.persist_quick_form();
    for id in touched {
        if !app.quick_visited.contains(&id) && !app.quick_queue.contains(&id) {
            app.quick_queue.push(id);
        }
    }
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
    let (processed, created, _) = app.persist_quick_form();
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

/// Referenz auf die nächste Person der FIFO-Queue setzen (bereits
/// abgearbeitete und verschwundene überspringen), Referenzstand laden.
/// `created` = Anzahl neu angelegter Personen dieses Schritts.
fn advance_quick_ref(app: &mut MiniGramps, created: usize) {
    while let Some(next) = app.quick_queue.first().cloned() {
        app.quick_queue.remove(0);
        if app.quick_visited.contains(&next) || app.data.find(&next).is_none() {
            continue;
        }
        let name = app
            .data
            .find(&next)
            .map(|person| person.display_name())
            .unwrap_or_else(|| next.clone());
        app.quick_ref_id = Some(next.clone());
        app.quick_visited.insert(next);
        app.quick_partner_idx = 0;
        quick_load_reference(app);
        app.status = if created > 0 {
            format!("Schnellerfassung: {created} neue Personen — weiter mit {name}")
        } else {
            format!("Weiter mit {name}")
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
                app.export_mfg_dialog();
            }
            ui.add_enabled(false, egui::Button::new("MiniGramps Mini (.mmg)"));
            ui.separator();
            ui.small("Gramps-, GEDCOM- und Mini-Formate folgen in den nächsten Schritten.");
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
