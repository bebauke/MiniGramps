//! Stammbaum-Zeichnung: Generations-Berechnung, Layout-Verhandlung, Karten.
//!
//! Verdrahtung:
//! - `draw_tree` wird im zentralen Panel von `ui::MiniGramps::update`
//!   aufgerufen. Parameter spiegeln App-Zustand:
//!     * `reference`   → `MiniGramps::reference` (Wurzel des Graphen),
//!     * `viewed`      → `MiniGramps::selected` (nur Hervorhebung),
//!     * `expanded`    → `MiniGramps::expanded` (ausgeklappte Grenzknoten),
//!     * `long_press_used` → `MiniGramps::long_press_used`
//!       (wird im CentralPanel zurückgesetzt, wenn kein Finger/Maus unten),
//!     * `generation_limit` → `MiniGramps::max_generations`
//!       (Einstellungen; 0 = alle Generationen),
//!     * `photo_cache` → `media::photo_texture` Cache.
//! - Rückaktionen laufen über `TreeAction` und werden in `update` ausgewertet:
//!   View (Klick), Reference (Shift+Klick, Doppelklick ohne Ziehen, langer
//!   Touch), ToggleExpand (`+`/`−`-Abzeichen an Grenzkarten).
//! - Layout: Ebenen via `model::children_of`/`parents_of` (BFS, Tiefe
//!   `generation_limit`), Reihenfolge ebenenweise (Verwandte neben dem
//!   Bezugspunkt), dann Verhandlung: Kinder ziehen zum Eltern-Junction,
//!   Eltern folgen den Kindern, überlappende Karten/Gruppen stoßen sich ab.
//! - Nachfahrensicht: Paare als verschmolzener Rahmen (Couple-Frame),
//!   Geschwister-Container nur bei > 1 sichtbarem Geschwister, Verbindungslinie
//!   startet am Couple-Frame und endet am Container (bzw. direkt am Kind).

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, TextureHandle, Vec2};

use crate::media::{initials, round_avatar_texture_cached};
use crate::model::TreeData;

/// Ansichtsmodus (Schalter in der Stammbaum-Werkzeugleiste, `ui`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeView {
    Descendants,
    Ancestors,
    Fan,
}

/// Ausrichtung der Generationsachse (Schalter in der Werkzeugleiste).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TreeOrientation {
    Vertical,
    Horizontal,
}

/// Kategorie im Beziehungspicker der rechten Seitenleiste (`ui`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RelationKind {
    Partner,
    Parent,
    Sibling,
    Child,
}

/// Klick-Aktion einer Baumkarte; Auswertung in `ui::MiniGramps::update`.
pub enum TreeAction {
    /// Person zur Ansicht/Bearbeitung öffnen (normaler Klick).
    View(String),
    /// Als Referenzperson setzen (Shift+Klick, Doppelklick, langer Touch).
    Reference(String),
    /// Weitere Generationen an diesem Knoten aus-/einklappen.
    ToggleExpand(String),
}

pub fn draw_tree(
    painter: &egui::Painter,
    rect: Rect,
    data: &TreeData,
    reference: Option<&str>,
    viewed: Option<&str>,
    action: &mut Option<TreeAction>,
    expanded: &HashSet<String>,
    long_press_used: &mut bool,
    card_drag: &mut Option<(String, Vec<String>)>,
    frame_drag: &mut bool,
    generation_limit: usize,
    manual_offsets: &mut HashMap<String, f32>,
    media_base: &std::path::Path,
    photo_cache: &mut HashMap<String, TextureHandle>,
    view: TreeView,
    orientation: TreeOrientation,
    zoom: f32,
    pan: Vec2,
    drag_started: bool,
    drag_ended: bool,
) -> Rect {
    let root: &str = match reference.filter(|id| data.find(id).is_some()) {
        Some(id) => id,
        None => match data.people.first() {
            Some(p) => p.id.as_str(),
            None => return Rect::ZERO,
        },
    };
    let ancestors = view != TreeView::Descendants;
    // BFS von der Referenzperson; Grenzknoten (generations_limit erreicht)
    // werden nur weiter expandiert, wenn sie in `expanded` stehen.
    let mut levels: HashMap<&str, usize> = HashMap::from([(root, 0usize)]);
    let mut frontier = vec![root];
    while let Some(id) = frontier.pop() {
        let level = levels[&id];
        if generation_limit > 0 && level + 1 >= generation_limit && !expanded.contains(id) {
            continue;
        }
        let relatives = if ancestors {
            data.parents_of(id)
        } else {
            data.children_of(id)
        };
        for relative in relatives {
            if !levels.contains_key(relative.id.as_str()) {
                levels.insert(relative.id.as_str(), level + 1);
                frontier.push(relative.id.as_str());
            }
        }
    }
    // Ebenenweise Ordnung: Verwandte erscheinen direkt neben der Person, die
    // sie in den Graphen gebracht hat (Eltern des Vaters beim Vater usw.).
    let max_level = levels.values().copied().max().unwrap_or(0);
    let mut rows: Vec<Vec<&str>> = vec![Vec::new(); max_level + 1];
    let mut placed: HashSet<&str> = HashSet::new();
    if let Some(root_level) = levels.get(&root) {
        rows[*root_level].push(root);
        placed.insert(root);
    }
    for level in 1..=max_level {
        let previous: Vec<&str> = rows[level - 1].clone();
        for prev_id in &previous {
            let relatives = if ancestors {
                data.parents_of(prev_id)
            } else {
                data.children_of(prev_id)
            };
            for relative in relatives {
                // Verwandte an der Sichtbarkeitsgrenze sind nicht in `levels`
                // (BFS bricht dort ab) – daher get() statt Indexzugriff.
                if levels.get(relative.id.as_str()) == Some(&level)
                    && placed.insert(relative.id.as_str())
                {
                    rows[level].push(relative.id.as_str());
                }
            }
        }
    }
    for (id, level) in &levels {
        if placed.insert(*id) {
            rows[*level].push(*id);
        }
    }
    let visible = |id: &str| levels.contains_key(id);
    // Gezeichnete Partner-Pseudokarten: Nachfahrensicht alle; Vorfahrensicht
    // nur Partner, die gemeinsam mit der Person Eltern eines sichtbaren
    // Kindes sind (also zum gezeigten Zweig gehören). Das Set steuert
    // Zeichnung UND Layout-Footprints.
    let mut shown_partners: HashSet<&str> = HashSet::new();
    for person in &data.people {
        if !visible(&person.id) {
            continue;
        }
        for partner in data.partners_of(&person.id) {
            if visible(&partner.id) || shown_partners.contains(partner.id.as_str()) {
                continue;
            }
            if view != TreeView::Ancestors
                || data.families.iter().any(|family| {
                    ((family.parent_a.as_deref() == Some(person.id.as_str())
                        && family.parent_b.as_deref() == Some(partner.id.as_str()))
                        || (family.parent_a.as_deref() == Some(partner.id.as_str())
                            && family.parent_b.as_deref() == Some(person.id.as_str())))
                        && family
                            .children
                            .iter()
                            .any(|child| levels.contains_key(child.as_str()))
                })
            {
                shown_partners.insert(partner.id.as_str());
            }
        }
    }
    // Layout-Konstanten (Kartenhöhe auch in `draw_person_card`).
    let card_h = 78.0f32;
    // Paarabstand: im horizontalen Baum stapeln Paare grundsätzlich OHNE
    // Zwischenraum (Verschieben regelt der Benutzer per Shift+Ziehen),
    // im vertikalen Baum knapp getrennt.
    let couple_gap = if orientation == TreeOrientation::Horizontal {
        0.0f32
    } else {
        4.0f32
    };
    let gap = 24.0f32;
    let gen_gap = 36.0f32;
    let row_widths: Vec<f32> = rows
        .iter()
        .map(|ids| {
            let mut width = 215.0f32;
            for id in ids {
                let person = data.find(id).unwrap();
                width = width.max(card_width_for(person, painter));
                for partner in data.partners_of(id) {
                    if !visible(&partner.id) && shown_partners.contains(partner.id.as_str()) {
                        width = width.max(card_width_for(partner, painter));
                    }
                }
            }
            width
        })
        .collect();
    // Startpositionen der horizontalen Generationsspalten.
    let mut col_x: Vec<f32> = Vec::with_capacity(rows.len());
    let mut acc = 0.0f32;
    for (row, _) in rows.iter().enumerate() {
        if row > 0 {
            acc += row_widths[row - 1] + gen_gap;
        }
        col_x.push(acc);
    }
    let widths: HashMap<&str, f32> = data
        .people
        .iter()
        .map(|person| (person.id.as_str(), card_width_for(person, painter)))
        .collect();
    // Horizontale Bäume: alle Karten einer Generation teilen die Zeilenbreite;
    // vertikale Bäume: jede Karte ihre eigene gemessene Breite.
    let card_w = |row: usize, id: &str| -> f32 {
        if orientation == TreeOrientation::Horizontal {
            row_widths[row]
        } else {
            widths[id]
        }
    };
    // Zeilen nach Breite (absteigend): die BREITESTE Stelle des Baums
    // verhandelt bei Kollisionen zuerst (bleibt stehen), schmalere weichen
    // aus — siehe repel_pass und finale Zentrierung.
    let row_order: Vec<usize> = {
        let mut order: Vec<usize> = (0..rows.len()).collect();
        order.sort_by(|&a, &b| {
            let width = |r: usize| -> f32 {
                if view == TreeView::Fan {
                    0.0
                } else if orientation == TreeOrientation::Horizontal {
                    rows[r].len() as f32 * card_h
                } else {
                    rows[r].iter().map(|id| widths[id]).sum::<f32>()
                        + gap * rows[r].len().saturating_sub(1) as f32
                }
            };
            width(b).total_cmp(&width(a))
        });
        order
    };
    // Ausdehnung entlang der Verteilungsachse: im vertikalen Baum die
    // Kartenbreite, im horizontalen Baum die Kartenhöhe (dort stapeln sich
    // Karten und Partner vertikal).
    let own_extent = |row: usize, id: &str| -> f32 {
        if orientation == TreeOrientation::Horizontal {
            card_h
        } else {
            card_w(row, id)
        }
    };
    let partner_extent = |row: usize, id: &str| -> f32 {
        if orientation == TreeOrientation::Horizontal {
            card_h
        } else {
            card_w(row, id)
        }
    };
    // Ursprungs-Familie je Person (nur Nachfahrensicht): Geschwister-Container
    // werden als starre Gruppe verhandelt.
    let group_of: HashMap<&str, &str> = if view == TreeView::Descendants {
        let mut map = HashMap::new();
        for family in &data.families {
            for child in &family.children {
                map.insert(child.as_str(), family.id.as_str());
            }
        }
        map
    } else {
        HashMap::new()
    };

    // Startpaketierung je Zeile (zentriert) oder perfektes rekursives Vorfahren-Layout:
    let mut spread: HashMap<&str, f32> = HashMap::new();
    if view == TreeView::Ancestors {
        // Berechne das perfekte, überschneidungsfreie Vorfahren-Layout rekursiv!
        spread = layout_ancestors(root, data, &levels, &widths, gap);

        // Partner-Pseudokarten einmalig an ihre Person koppeln.
        for (row, ids) in rows.iter().enumerate() {
            for id in ids {
                if orientation == TreeOrientation::Vertical {
                    if let Some(&sx) = spread.get(id) {
                        let mut px = sx + card_w(row, id) / 2.0 + couple_gap;
                        for partner in data.partners_of(id) {
                            if visible(&partner.id) || !shown_partners.contains(partner.id.as_str())
                            {
                                continue;
                            }
                            spread.insert(partner.id.as_str(), px + card_w(row, &partner.id) / 2.0);
                            px += card_w(row, &partner.id) + couple_gap;
                        }
                    }
                } else {
                    if let Some(&sy) = spread.get(id) {
                        let mut py = sy + card_h / 2.0 + couple_gap;
                        for partner in data.partners_of(id) {
                            if visible(&partner.id) || !shown_partners.contains(partner.id.as_str())
                            {
                                continue;
                            }
                            spread.insert(partner.id.as_str(), py + card_h / 2.0);
                            py += card_h + couple_gap;
                        }
                    }
                }
            }
        }
    } else {
        // Standard-Paketierung für andere Ansichten
        for (row, ids) in rows.iter().enumerate() {
            if view == TreeView::Fan {
                continue;
            }
            let mut footprints: Vec<(&str, f32)> = Vec::new();
            let mut total = 0.0f32;
            for id in ids {
                let mut footprint = own_extent(row, id);
                for partner in data.partners_of(id) {
                    if !visible(&partner.id) && shown_partners.contains(partner.id.as_str()) {
                        footprint += partner_extent(row, &partner.id) + couple_gap;
                    }
                }
                footprints.push((*id, footprint));
                total += footprint;
            }
            total += gap * footprints.len().saturating_sub(1) as f32;
            let mut cursor = -total / 2.0;
            for (id, footprint) in footprints {
                spread.insert(id, cursor + own_extent(row, id) / 2.0);
                cursor += footprint + gap;
            }
        }
        for _ in 0..12 {
            // 1) Partner-Pseudokarten an ihre Person koppeln.
            for (row, ids) in rows.iter().enumerate() {
                if view == TreeView::Fan {
                    continue;
                }
                for id in ids {
                    if orientation == TreeOrientation::Vertical {
                        let mut px = spread[*id] + card_w(row, id) / 2.0 + couple_gap;
                        for partner in data.partners_of(id) {
                            if visible(&partner.id) || !shown_partners.contains(partner.id.as_str())
                            {
                                continue;
                            }
                            spread.insert(partner.id.as_str(), px + card_w(row, &partner.id) / 2.0);
                            px += card_w(row, &partner.id) + couple_gap;
                        }
                    } else {
                        let mut py = spread[*id] + card_h / 2.0 + couple_gap;
                        for partner in data.partners_of(id) {
                            if visible(&partner.id) || !shown_partners.contains(partner.id.as_str())
                            {
                                continue;
                            }
                            spread.insert(partner.id.as_str(), py + card_h / 2.0);
                            py += card_h + couple_gap;
                        }
                    }
                }
            }
            // 2) Attraktion: Kinder → Eltern-Junction, Eltern → Kinder-Mittelwert.
            for family in &data.families {
                let children: Vec<&str> = family
                    .children
                    .iter()
                    .filter(|child| spread.contains_key(child.as_str()))
                    .map(|child| child.as_str())
                    .collect();
                if children.is_empty() {
                    continue;
                }
                let parents: Vec<&str> = [&family.parent_a, &family.parent_b]
                    .into_iter()
                    .flatten()
                    .filter(|parent| spread.contains_key(parent.as_str()))
                    .map(|parent| parent.as_str())
                    .collect();
                if parents.is_empty() {
                    continue;
                }
                let junction =
                    parents.iter().map(|p| spread[*p]).sum::<f32>() / parents.len() as f32;
                if view == TreeView::Descendants {
                    // Geschwister bewegen sich als starre Gruppe (Container).
                    let mean =
                        children.iter().map(|c| spread[*c]).sum::<f32>() / children.len() as f32;
                    let delta = 0.5 * (junction - mean);
                    for child in &children {
                        if let Some(value) = spread.get_mut(child) {
                            *value += delta;
                        }
                    }
                } else {
                    for child in &children {
                        if let Some(value) = spread.get_mut(child) {
                            *value += 0.5 * (junction - *value);
                        }
                    }
                }
                let target =
                    children.iter().map(|c| spread[*c]).sum::<f32>() / children.len() as f32;
                let parent_mean =
                    parents.iter().map(|p| spread[*p]).sum::<f32>() / parents.len() as f32;
                let delta = 0.4 * (target - parent_mean);
                for parent in &parents {
                    if let Some(value) = spread.get_mut(parent) {
                        *value += delta;
                    }
                }
            }
            // 3) Abstoßung: innerhalb der Gruppen (Karten) und zwischen den
            //    Gruppen (Container) als starre Blöcke (siehe `repel` oben).
            repel_pass(
                &mut spread,
                &rows,
                &row_order,
                data,
                &levels,
                view,
                orientation,
                card_h,
                &widths,
                gap,
                couple_gap,
                &group_of,
                &shown_partners,
            );
        }
    }

    // Endgültige Positionen (Layout-Koordinaten → beim Zeichnen skaliert).
    // Zeilen nach der Verhandlung neu zentrieren – das monotone
    // Rechts-Schieben der Abstoßung verschiebt die Zeilen nach rechts.
    if view != TreeView::Ancestors {
        for ids in rows.iter() {
            if view == TreeView::Fan || ids.is_empty() {
                continue;
            }
            let mean = ids.iter().map(|id| spread[*id]).sum::<f32>() / ids.len() as f32;
            for id in ids {
                if let Some(value) = spread.get_mut(*id) {
                    *value -= mean;
                }
            }
        }
    }

    // Pfad-Knoten: Referenzperson + alle sichtbaren Kinder (sie "tragen" den
    // Zweig von oben). Der Zweig einer Familie geht im Nachfahrenbaum vom
    // PARTNER weiter, von dem die Kinder sind – so steht die Person immer
    // zuerst und je Frau/Mann hangt der jeweilige Kinderzweig darunter.
    let mut path_nodes: HashSet<&str> = HashSet::from([root]);
    for family in &data.families {
        for child in &family.children {
            if levels.contains_key(child.as_str()) {
                path_nodes.insert(child.as_str());
            }
        }
    }

    // Manuelle Verschiebungen (Shift+Ziehen): Offset wird NUR auf die
    // gezogene Person (+ mitziehenden Partner) gesetzt und hierarchisch
    // vererbt — Nachfahren erben den Junction-Versatz abwärts, Vorfahren
    // aufwärts. Danach ein finaler Abstoßungspass: geschobene Teilbäume
    // dürfen andere Gruppen NICHT überlagern.
    let mut peak_eff = 0.0f32;
    let eff_owned = effective_offsets(data, &levels, &spread, manual_offsets, view);
    for (id, value) in &eff_owned {
        if let Some(spread_value) = spread.get_mut(id.as_str()) {
            *spread_value += *value;
        }
        peak_eff = peak_eff.max(value.abs());
    }
    if view != TreeView::Ancestors {
        repel_pass(
            &mut spread,
            &rows,
            &row_order,
            data,
            &levels,
            view,
            orientation,
            card_h,
            &widths,
            gap,
            couple_gap,
            &group_of,
            &shown_partners,
        );
    } else if let Some((drag_root, _)) = card_drag {
        block_ancestor_drag(
            &mut spread,
            &rows,
            data,
            &levels,
            &widths,
            &shown_partners,
            drag_root,
            manual_offsets,
            gap,
            view,
        );
    }

    // Finale Zentrierung: Gruppen werden minimal (kollisionsfrei) in
    // Richtung ihrer Junction verschoben — Kinder hängen so weit wie möglich
    // senkrecht unter dem Elternpaar, Eltern-Paare über ihrem Kind. Manuell
    // verschobene Personen blockieren ihre Gruppe (bleibt, wo hingeschoben).
    if view != TreeView::Ancestors {
        let manual_ids: HashSet<&str> = manual_offsets.keys().map(|key| key.as_str()).collect();
        let mut anchors: HashMap<&str, f32> = HashMap::new();
        let mut group_key_of: HashMap<&str, &str> = HashMap::new();
        for family in &data.families {
            let children: Vec<&str> = family
                .children
                .iter()
                .filter(|child| spread.contains_key(child.as_str()))
                .map(|child| child.as_str())
                .collect();
            let parents: Vec<&str> = [&family.parent_a, &family.parent_b]
                .into_iter()
                .flatten()
                .filter(|parent| spread.contains_key(parent.as_str()))
                .map(|parent| parent.as_str())
                .collect();
            if children.is_empty() || parents.is_empty() {
                continue;
            }
            if view == TreeView::Descendants {
                let anchor = parents.iter().map(|p| spread[*p]).sum::<f32>() / parents.len() as f32;
                for child in children {
                    anchors.insert(child, anchor);
                    group_key_of.insert(child, family.id.as_str());
                }
            } else {
                let anchor =
                    children.iter().map(|c| spread[*c]).sum::<f32>() / children.len() as f32;
                for parent in parents {
                    anchors.insert(parent, anchor);
                    group_key_of.insert(parent, family.id.as_str());
                }
            }
        }
        // Zeilen von der breitesten zur schmalsten: breite Stellen behalten
        // ihren kollisionsfreien Platz, schmale passen sich ein.
        for &row in &row_order {
            let ids = &rows[row];
            if view == TreeView::Fan || ids.is_empty() {
                continue;
            }
            let mut members: Vec<(&str, f32)> = ids
                .iter()
                .map(|id| {
                    let mut footprint = own_extent(row, id);
                    for partner in data.partners_of(id) {
                        if !visible(&partner.id) && shown_partners.contains(partner.id.as_str()) {
                            footprint += partner_extent(row, &partner.id) + couple_gap;
                        }
                    }
                    (*id, footprint)
                })
                .collect();
            members.sort_by(|a, b| spread[a.0].total_cmp(&spread[b.0]));
            let key_of = |id: &str| group_key_of.get(id).copied().unwrap_or("");
            let mut groups: Vec<Vec<(&str, f32)>> = Vec::new();
            for member in members {
                match groups.last_mut() {
                    Some(last)
                        if !key_of(last[0].0).is_empty()
                            && key_of(last[0].0) == key_of(member.0) =>
                    {
                        last.push(member)
                    }
                    _ => groups.push(vec![member]),
                }
            }
            // Nach aktueller Mitte sortieren und links→rechts mit Klemmen
            // (linke Gruppe, rechte Gruppe) auf das Ziel schieben.
            // Nach ZIEL sortieren (Junction-Anker), nicht nach aktueller
            // Mitte: Die Reihenfolge der Kindergruppen folgt dann ALWAYS der
            // Reihenfolge ihrer Eltern-Junctions — Verbindungslinien können
            // sich NICHT kreuzen. Gruppen ohne Ziel folgen nach aktueller
            // Mitte.
            type GroupItem<'b> = (Vec<(&'b str, f32)>, Option<f32>, bool, f32);
            let mut ordered: Vec<GroupItem> = groups
                .iter()
                .map(|group| {
                    let start = group
                        .iter()
                        .map(|(id, fp)| spread[*id] - fp / 2.0)
                        .fold(f32::MAX, f32::min);
                    let end = group
                        .iter()
                        .map(|(id, fp)| spread[*id] + fp / 2.0)
                        .fold(f32::MIN, f32::max);
                    let target = group.iter().find_map(|(id, _)| anchors.get(*id).copied());
                    let manual = group.iter().any(|(id, _)| manual_ids.contains(*id));
                    (group.clone(), target, manual, (start + end) / 2.0)
                })
                .collect();
            ordered.sort_by(|a, b| match (a.1, b.1) {
                (Some(target_a), Some(target_b)) => target_a
                    .total_cmp(&target_b)
                    .then_with(|| a.3.total_cmp(&b.3)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.3.total_cmp(&b.3),
            });
            let mut prev_end: Option<f32> = None;
            for (index, (group, target, manual, _)) in ordered.iter().enumerate() {
                let start = group
                    .iter()
                    .map(|(id, fp)| spread[*id] - fp / 2.0)
                    .fold(f32::MAX, f32::min);
                let end = group
                    .iter()
                    .map(|(id, fp)| spread[*id] + fp / 2.0)
                    .fold(f32::MIN, f32::max);
                let next_start = ordered.get(index + 1).and_then(|(next, _, _, _)| {
                    next.iter().map(|(id, fp)| spread[*id] - fp / 2.0).fold(
                        None::<f32>,
                        |acc, value| {
                            Some(match acc {
                                Some(best) => best.min(value),
                                None => value,
                            })
                        },
                    )
                });
                if let Some(target) = target {
                    if !*manual {
                        let min_d = prev_end
                            .map(|prev| prev + gap - start)
                            .unwrap_or(f32::NEG_INFINITY);
                        let max_d = next_start
                            .map(|next| next - gap - end)
                            .unwrap_or(f32::INFINITY);
                        if min_d <= max_d {
                            let center = (start + end) / 2.0;
                            let delta = (*target - center).clamp(min_d, max_d);
                            for (id, _) in group {
                                if let Some(value) = spread.get_mut(*id) {
                                    *value += delta;
                                }
                            }
                        }
                    }
                }
                let end_after = group
                    .iter()
                    .map(|(id, fp)| spread[*id] + fp / 2.0)
                    .fold(f32::MIN, f32::max);
                prev_end = Some(match prev_end {
                    Some(previous) => previous.max(end_after),
                    None => end_after,
                });
            }
        }
    }

    let mut positions: HashMap<&str, (f32, f32)> = HashMap::new();
    for (row, ids) in rows.iter().enumerate() {
        if view == TreeView::Fan {
            for (index, id) in ids.iter().enumerate() {
                let count = ids.len();
                let angle = if count <= 1 {
                    if orientation == TreeOrientation::Vertical {
                        -std::f32::consts::FRAC_PI_2
                    } else {
                        0.
                    }
                } else if orientation == TreeOrientation::Vertical {
                    std::f32::consts::PI + std::f32::consts::PI * index as f32 / (count - 1) as f32
                } else {
                    -std::f32::consts::FRAC_PI_2
                        + std::f32::consts::PI * index as f32 / (count - 1) as f32
                };
                let radius = 175. * row as f32;
                positions.insert(*id, (radius * angle.cos(), radius * angle.sin()));
            }
            continue;
        }
        for id in ids {
            let s = spread[*id];
            if orientation == TreeOrientation::Vertical {
                // Vorfahrenbaum gespiegelt: das Kind (Referenz) steht unten,
                // die Vorfahren wachsen nach oben.
                let y = row as f32 * (card_h + 40.0) * if ancestors { -1.0 } else { 1.0 };
                positions.insert(*id, (s, y));
                let mut px = s + card_w(row, id) / 2.0 + couple_gap;
                for partner in data.partners_of(id) {
                    if visible(&partner.id) || !shown_partners.contains(partner.id.as_str()) {
                        continue;
                    }
                    let pw = card_w(row, &partner.id);
                    // Pseudo-Partner folgen automatisch — ein eigener Versatz
                    // würde die Footprint-Bilanz der Nachbarn brechen.
                    positions.insert(partner.id.as_str(), (px + pw / 2.0, y));
                    px += pw + couple_gap;
                }
            } else {
                // Horizontal gespiegelt: Vorfahren wachsen nach links.
                let x = (col_x[row] + card_w(row, id) / 2.0) * if ancestors { -1.0 } else { 1.0 };
                positions.insert(*id, (x, s));
                // Stapelung beginnt an der UNTerkante der Karte (s ist die
                // Mitte!) — sonst entsteht ein halber Kartenhöhe-Abstand
                // zwischen Partnern.
                let mut py = s + card_h / 2.0 + couple_gap;
                for partner in data.partners_of(id) {
                    if visible(&partner.id) || !shown_partners.contains(partner.id.as_str()) {
                        continue;
                    }
                    // Pseudo-Partner folgen automatisch — kein eigener
                    // Versatz (siehe vertikaler Zweig oben).
                    positions.insert(partner.id.as_str(), (x, py + card_h / 2.0));
                    py += card_h + couple_gap;
                }
            }
        }
    }
    let center = rect.center() + pan;
    // Inhaltsmaße in Layout-Koordinaten (fuer den Zoom-Fit in ui::update):
    // alle Karten inklusive Pseudo-Partner.
    let content_bounds = {
        let mut min = Pos2::new(f32::INFINITY, f32::INFINITY);
        let mut max = Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        for (id, &(x, y)) in positions.iter() {
            let half = widths.get(id).copied().unwrap_or(215.0) / 2.0;
            min = min.min(Pos2::new(x - half, y - card_h / 2.0));
            max = max.max(Pos2::new(x + half, y + card_h / 2.0));
        }
        if min.x.is_finite() {
            Rect::from_min_max(min, max)
        } else {
            Rect::ZERO
        }
    };
    let pos = |id: &str| {
        positions
            .get(id)
            .map(|(x, y)| center + Vec2::new(*x * zoom, *y * zoom))
    };
    if view == TreeView::Descendants {
        for person in &data.people {
            if !visible(&person.id) {
                continue;
            }
            let Some(at) = pos(&person.id) else {
                continue;
            };
            let Some(row) = levels.get(person.id.as_str()) else {
                continue;
            };
            let mut frame =
                Rect::from_center_size(at, Vec2::new(card_w(*row, &person.id), card_h) * zoom);
            let mut merged = false;
            for partner in data.partners_of(&person.id) {
                if visible(&partner.id) || !shown_partners.contains(partner.id.as_str()) {
                    continue;
                }
                if let Some(pp) = pos(&partner.id) {
                    frame = frame.union(Rect::from_center_size(
                        pp,
                        Vec2::new(card_w(*row, &partner.id), card_h) * zoom,
                    ));
                    merged = true;
                }
            }
            if merged {
                // Paarrahmen: dezenter Hintergrund (10 % Schwarz) + goldene
                // Umrandung — hebt sich klar vom Geschwister-Container ab.
                let outer = frame.expand(4.0 * zoom);
                painter.rect_filled(outer, 12.0 * zoom, Color32::from_black_alpha(25));
                painter.rect_stroke(
                    outer,
                    12.0 * zoom,
                    Stroke::new(1.5, Color32::from_rgb(201, 170, 96)),
                    egui::StrokeKind::Inside,
                );
                // Rahmen ziehen (Klick+Drag, ohne Shift): der ganze Zweig
                // folgt — Person + sichtbare Partner erhalten den Versatz,
                // die Kinder erben ihn über die Junction-Vererbung.
                let frame_hit = painter.ctx().input(|i| {
                    i.pointer.primary_down()
                        && !i.modifiers.shift
                        && i.pointer.press_origin().is_some_and(|q| outer.contains(q))
                });
                if frame_hit
                    && card_drag
                        .as_ref()
                        .is_none_or(|(id, _)| id == person.id.as_str())
                {
                    let delta = painter.ctx().input(|i| {
                        let d = i.pointer.delta();
                        if orientation == TreeOrientation::Vertical {
                            d.x
                        } else {
                            d.y
                        }
                    }) / zoom;
                    if delta != 0.0 {
                        let mut members = vec![person.id.clone()];
                        for partner in data.partners_of(&person.id) {
                            if levels.contains_key(partner.id.as_str()) {
                                members.push(partner.id.clone());
                            }
                        }
                        for id in &members {
                            *manual_offsets.entry(id.clone()).or_insert(0.0) += delta;
                        }
                        *card_drag = Some((person.id.clone(), members));
                    }
                    *frame_drag = true;
                }
            }
        }
    }
    // Verbindungslinien je Familie.
    for family in &data.families {
        let pa = family.parent_a.as_deref().and_then(&pos);
        let pb = family.parent_b.as_deref().and_then(&pos);
        if view == TreeView::Descendants {
            // Linie startet am Couple-Frame der Eltern; Ziel ist der
            // Geschwister-Container (ab 2 Geschwistern) oder direkt das Kind.
            let couple_frame: Option<Rect> = {
                let mut frame: Option<Rect> = None;
                for parent in [&family.parent_a, &family.parent_b].into_iter().flatten() {
                    if let Some(pp) = pos(parent) {
                        let Some(prow) = levels.get(parent.as_str()) else {
                            continue;
                        };
                        let rect = Rect::from_center_size(
                            pp,
                            Vec2::new(card_w(*prow, parent), card_h) * zoom,
                        );
                        frame = Some(match frame {
                            Some(previous) => previous.union(rect),
                            None => rect,
                        });
                    }
                }
                frame
            };
            let Some(couple_frame) = couple_frame else {
                continue;
            };
            // Zweigursprung: die Partnerkarte (nicht der Pfad-Knoten) – so
            // hangt der Kinderzweig klar an der Person, von der die Kinder
            // sind. Fallback: Mitte des Couple-Frames.
            let branch_origin = {
                let parent_pair = match (&family.parent_a, &family.parent_b) {
                    (Some(a), Some(b)) => Some((a.as_str(), b.as_str())),
                    _ => None,
                };
                let partner = parent_pair.and_then(|(a, b)| {
                    let a_path = path_nodes.contains(a);
                    let b_path = path_nodes.contains(b);
                    if a_path && !b_path {
                        Some(b)
                    } else if b_path && !a_path {
                        Some(a)
                    } else {
                        None
                    }
                });
                partner.and_then(|partner_id| {
                    pos(partner_id).map(|center| {
                        let prow = levels.get(partner_id).copied().unwrap_or(0);
                        let pw = card_w(prow, partner_id);
                        if orientation == TreeOrientation::Vertical {
                            Pos2::new(center.x, center.y + card_h * zoom / 2.0)
                        } else {
                            Pos2::new(center.x + pw * zoom / 2.0, center.y)
                        }
                    })
                })
            };
            let origin = branch_origin.unwrap_or_else(|| {
                if orientation == TreeOrientation::Vertical {
                    Pos2::new(couple_frame.center().x, couple_frame.bottom())
                } else {
                    Pos2::new(couple_frame.right(), couple_frame.center().y)
                }
            });
            let mut blocks: Vec<Rect> = Vec::new();
            for child in &family.children {
                // Kinder, die primär einer anderen Familie zugeordnet sind
                // (Doppelmitgliedschaft), nur dort zeichnen.
                if group_of
                    .get(child.as_str())
                    .is_some_and(|primary| *primary != family.id.as_str())
                {
                    continue;
                }
                let Some(at) = pos(child) else {
                    continue;
                };
                let Some(crow) = levels.get(child.as_str()) else {
                    continue;
                };
                let mut block =
                    Rect::from_center_size(at, Vec2::new(card_w(*crow, child), card_h) * zoom);
                for partner in data.partners_of(child) {
                    if visible(&partner.id) || !shown_partners.contains(partner.id.as_str()) {
                        continue;
                    }
                    if let Some(pp) = pos(&partner.id) {
                        block = block.union(Rect::from_center_size(
                            pp,
                            Vec2::new(card_w(*crow, &partner.id), card_h) * zoom,
                        ));
                    }
                }
                blocks.push(block);
            }
            if blocks.is_empty() {
                continue;
            }
            let mut container: Option<Rect> = None;
            if blocks.len() > 1 {
                let mut bounds = blocks[0];
                for block in &blocks[1..] {
                    bounds = bounds.union(*block);
                }
                let container_rect = bounds.expand(12.0 * zoom);
                painter.rect_filled(container_rect, 10.0 * zoom, Color32::from_black_alpha(25));
                painter.rect_stroke(
                    container_rect,
                    10.0 * zoom,
                    Stroke::new(1.0, Color32::from_rgb(70, 105, 110)),
                    egui::StrokeKind::Outside,
                );
                container = Some(container_rect);
            }
            let target = match &container {
                Some(container_rect) => {
                    if orientation == TreeOrientation::Vertical {
                        Pos2::new(
                            origin.x.clamp(
                                container_rect.left() + 10.0 * zoom,
                                container_rect.right() - 10.0 * zoom,
                            ),
                            container_rect.top(),
                        )
                    } else {
                        Pos2::new(
                            container_rect.left(),
                            origin.y.clamp(
                                container_rect.top() + 10.0 * zoom,
                                container_rect.bottom() - 10.0 * zoom,
                            ),
                        )
                    }
                }
                None => {
                    let block = blocks[0];
                    if orientation == TreeOrientation::Vertical {
                        Pos2::new(
                            origin
                                .x
                                .clamp(block.left() + 10.0 * zoom, block.right() - 10.0 * zoom),
                            block.top(),
                        )
                    } else {
                        Pos2::new(
                            block.left(),
                            origin
                                .y
                                .clamp(block.top() + 10.0 * zoom, block.bottom() - 10.0 * zoom),
                        )
                    }
                }
            };
            painter.line_segment(
                [origin, target],
                Stroke::new(2.0, Color32::from_rgb(74, 111, 119)),
            );
            continue;
        }
        // Vorfahren-/Fächer-Sicht: Paarlinie zwischen sichtbaren Eltern,
        // Linien vom Junction zu jedem Kind.
        if let (Some(a), Some(b)) = (pa, pb) {
            painter.line_segment([a, b], Stroke::new(2.0, Color32::from_rgb(120, 170, 160)));
        }
        let junction = match (pa, pb) {
            (Some(a), Some(b)) => {
                if view == TreeView::Ancestors {
                    let id_a = family.parent_a.as_deref().unwrap_or("");
                    let id_b = family.parent_b.as_deref().unwrap_or("");
                    let wa = card_w(levels.get(id_a).copied().unwrap_or(0), id_a);
                    let wb = card_w(levels.get(id_b).copied().unwrap_or(0), id_b);

                    let (left_x, left_w, right_x, right_w) = if a.x < b.x {
                        (a.x, wa, b.x, wb)
                    } else {
                        (b.x, wb, a.x, wa)
                    };
                    let jx = (left_x + left_w * zoom / 2.0 + right_x - right_w * zoom / 2.0) / 2.0;
                    Pos2::new(jx, (a.y + b.y) / 2.0)
                } else {
                    Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)
                }
            }
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => continue,
        };
        for child in &family.children {
            if let Some(c) = pos(child) {
                painter.line_segment(
                    [junction, c],
                    Stroke::new(2.0, Color32::from_rgb(74, 111, 119)),
                );
            }
        }
    }
    // Karten zeichnen (Personen + Partner-Pseudokarten) und Aktionen sammeln.
    for person in &data.people {
        let Some(at) = pos(&person.id) else {
            continue;
        };
        let Some(level) = levels.get(person.id.as_str()) else {
            continue;
        };
        let width = match view {
            TreeView::Fan => 260.0,
            _ => card_w(*level, &person.id),
        };
        let card = Rect::from_center_size(at, Vec2::new(width, card_h) * zoom);
        let card_clicked = draw_person_card(
            painter,
            person,
            at,
            width,
            zoom,
            viewed == Some(person.id.as_str()),
            false,
            media_base,
            photo_cache,
        );
        // Ausklapp-Abzeichen: Grenzknoten mit nicht sichtbaren Verwandten.
        let has_more = {
            let relatives = if ancestors {
                data.parents_of(&person.id)
            } else {
                data.children_of(&person.id)
            };
            relatives
                .iter()
                .any(|relative| !levels.contains_key(relative.id.as_str()))
        };
        let mut badge_clicked = false;
        // Badge-Position (Ausklapp-Pfeil) vorab berechnen, damit auch das
        // Hovern ÜBER dem Badge die Karte als "gehovert" zählt und der
        // Pfeil nicht verschwindet, sobald die Maus von der Karte auf den
        // Pfeil (oberhalb der Box bzw. links daneben) wandert.
        let badge_at = if view == TreeView::Descendants {
            Pos2::new(card.right() - 12.0 * zoom, card.top() + 12.0 * zoom)
        } else if orientation == TreeOrientation::Vertical {
            Pos2::new(card.center().x, card.top() - 13.0 * zoom)
        } else {
            Pos2::new(card.left() - 13.0 * zoom, card.center().y)
        };
        let badge_r = 9.0 * zoom;
        let badge_rect = Rect::from_center_size(badge_at, Vec2::splat(badge_r * 2.0));
        let pointer_pos = painter.ctx().input(|i| i.pointer.interact_pos());
        let card_hovered = pointer_pos
            .is_some_and(|q| card.contains(q) || badge_rect.contains(q));
        if view != TreeView::Fan && card_hovered && (has_more || expanded.contains(&person.id)) {
            // Nachfahrensicht: Badge in der rechten oberen Ecke. Vorfahren-
            // sicht: der Ausklapp-Pfeil liegt ÜBER der Box (bzw. links im
            // horizontalen Baum), weil die Vorfahren dort erscheinen.
            painter.circle_filled(badge_at, badge_r, Color32::from_rgb(24, 40, 48));
            painter.circle_stroke(
                badge_at,
                badge_r,
                Stroke::new(1.0, Color32::from_rgb(120, 170, 160)),
            );
            // Pfeil statt "+": Ausklapprichtung — Nachfahren nach unten,
            // Vorfahren nach oben; ausgeklappt zeigt der Pfeil zurück.
            let expand_down = view == TreeView::Descendants;
            let arrow_down = if expanded.contains(&person.id) {
                !expand_down
            } else {
                expand_down
            };
            if arrow_down {
                painter.arrow(
                    badge_at + Vec2::new(0.0, 4.5 * zoom),
                    Vec2::new(0.0, -9.0 * zoom),
                    Stroke::new(1.5, Color32::from_rgb(158, 213, 199)),
                );
            } else {
                painter.arrow(
                    badge_at - Vec2::new(0.0, 4.5 * zoom),
                    Vec2::new(0.0, 9.0 * zoom),
                    Stroke::new(1.5, Color32::from_rgb(158, 213, 199)),
                );
            }
            badge_clicked = painter.ctx().input(|i| {
                i.pointer.any_click()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|q| badge_rect.contains(q))
            });
            if badge_clicked {
                *action = Some(TreeAction::ToggleExpand(person.id.clone()));
            }
        }
        // Aufwärtspfeil über der Referenzperson (Nachfahrensicht): springt
        // zum Vater – Fallback Mutter – und macht ihn zur Referenz.
        if view == TreeView::Descendants && person.id == root {
            let parents = data.parents_of(&person.id);
            let target = parents
                .iter()
                .find(|parent| parent.gender == crate::model::Gender::Male)
                .or_else(|| parents.first());
            if let Some(parent) = target {
                draw_up_badge(painter, card, zoom, orientation, action, &parent.id);
            }
        }
        if !badge_clicked && card_clicked {
            let shift = painter.ctx().input(|i| i.modifiers.shift);
            *action = Some(if shift {
                TreeAction::Reference(person.id.clone())
            } else {
                TreeAction::View(person.id.clone())
            });
        }
        if !badge_clicked {
            // Langer Touch (≥ 0,6 s) setzt die Referenzperson – NUR bei
            // echtem Touch (`any_touches`), nicht bei gedrückter Maustaste;
            // pro Drücken nur einmal (`long_press_used`, Reset in `ui`).
            let long_pressed = painter.ctx().input(|i| {
                i.any_touches()
                    && i.pointer.press_origin().is_some_and(|q| card.contains(q))
                    && i.pointer
                        .press_start_time()
                        .is_some_and(|t0| i.time - t0 >= 0.6)
            });
            if long_pressed && !*long_press_used {
                *action = Some(TreeAction::Reference(person.id.clone()));
                *long_press_used = true;
            }
        }
        // Shift+Ziehen: Person manuell verschieben – inkl. Partnern und
        // aller sichtbaren Verwandten in Ansichtsrichtung ("Teilbaum
        // auseinander ziehen"). Der Drag ist zustandsgebunden (`card_drag`):
        // Einmal gestartet, läuft er bis zum Loslassen – auch wenn die Karte
        // den ursprünglichen Press-Punkt längst verlassen hat.
        let (manual_delta, drag_start) = painter.ctx().input(|i| {
            if !(i.pointer.primary_down() && i.modifiers.shift) {
                return (0.0, false);
            }
            let pressed_here = i.pointer.press_origin().is_some_and(|q| card.contains(q));
            let is_active = card_drag
                .as_ref()
                .is_some_and(|(id, _)| id == person.id.as_str());
            if !(pressed_here || is_active) {
                return (0.0, false);
            }
            let delta = i.pointer.delta();
            (
                if orientation == TreeOrientation::Vertical {
                    delta.x
                } else {
                    delta.y
                },
                pressed_here,
            )
        });
        if manual_delta != 0.0 || (drag_start && card_drag.is_none()) {
            let layout_delta = manual_delta / zoom;
            // Im Vorfahren- und Nachfahrensicht bleibt der Partner stehen
            // (das Kind zentriert sich beim Ziehen automatisch zwischen den
            // Eltern — der Down-Pass gibt ihm die halbe Verschiebung). Nur
            // bei Fan-View klebt der Partner (fast) an der Person.
            let partner_follows = |base_center: Pos2, base_width: f32, partner_id: &str| -> bool {
                if view != TreeView::Fan {
                    return false;
                }
                let Some(partner_center) = pos(partner_id) else {
                    return false;
                };
                let partner_width = widths.get(partner_id).copied().unwrap_or(215.0);
                let edge_gap = if orientation == TreeOrientation::Vertical {
                    (base_center.x - partner_center.x).abs()
                        - (base_width + partner_width) * zoom / 2.0
                } else {
                    (base_center.y - partner_center.y).abs() - card_h * zoom
                };
                edge_gap <= 10.0
            };
            // Move-Set: nur die gezogene Person (+ ggf. mitklebender
            // sichtbarer Partner). Vererbung der Versätze an Nachfahren bzw.
            // Vorfahren übernimmt die hierarchische Anwendung weiter oben —
            // dadurch bleiben Kinder exakt relativ zum Elternteil.
            let mut move_set: Vec<String> = vec![person.id.clone()];
            for partner in data.partners_of(&person.id) {
                if levels.contains_key(partner.id.as_str())
                    && !move_set.iter().any(|m| m == &partner.id)
                    && partner_follows(at, width, &partner.id)
                {
                    move_set.push(partner.id.clone());
                }
            }
            // Move-Set beim Drag-Start einfrieren und als aktiven Drag
            // merken; in den Folgetrames wird es wiederverwendet.
            if let Some((_, members)) = card_drag {
                // Laufender Drag: bekannte Menge verschieben.
                for id in members.clone() {
                    *manual_offsets.entry(id.clone()).or_insert(0.0) += layout_delta;
                }
            } else {
                for id in &move_set {
                    *manual_offsets.entry(id.clone()).or_insert(0.0) += layout_delta;
                }
                if view == TreeView::Ancestors {
                    let offsets: Vec<_> = move_set.iter()
                        .map(|id| format!("{}:{:.1}", id, manual_offsets.get(id.as_str()).copied().unwrap_or(0.0)))
                        .collect();
                    println!("DRAG_PICK person={} move_set=[{}] zoom={:.3} layout_delta={:.1}",
                        person.id, offsets.join(", "), zoom, layout_delta);
                }
                *card_drag = Some((person.id.clone(), move_set));
            }
        }
        if view == TreeView::Fan {
            continue;
        }
        // Partner-Pseudokarten (Partner nicht selbst im Graphen).
        for partner in data.partners_of(&person.id) {
            if visible(&partner.id) || !shown_partners.contains(partner.id.as_str()) {
                continue;
            }
            let Some(pat) = pos(&partner.id) else {
                continue;
            };
            let partner_width = card_w(*level, &partner.id);
            let (line_a, line_b) = if orientation == TreeOrientation::Vertical {
                (
                    Pos2::new(at.x + width * zoom / 2.0, at.y),
                    Pos2::new(pat.x - partner_width * zoom / 2.0, pat.y),
                )
            } else {
                (
                    Pos2::new(at.x, at.y + card_h * zoom / 2.0),
                    Pos2::new(pat.x, pat.y - card_h * zoom / 2.0),
                )
            };
            if view != TreeView::Descendants {
                painter.line_segment(
                    [line_a, line_b],
                    Stroke::new(2.0, Color32::from_rgb(120, 170, 160)),
                );
            }
            let partner_card = Rect::from_center_size(pat, Vec2::new(partner_width, card_h) * zoom);
            let partner_clicked = draw_person_card(
                painter,
                partner,
                pat,
                partner_width,
                zoom,
                viewed == Some(partner.id.as_str()),
                true,
                media_base,
                photo_cache,
            );
            let partner_long = painter.ctx().input(|i| {
                i.any_touches()
                    && i.pointer
                        .press_origin()
                        .is_some_and(|q| partner_card.contains(q))
                    && i.pointer
                        .press_start_time()
                        .is_some_and(|t0| i.time - t0 >= 0.6)
            });
            if partner_clicked {
                let shift = painter.ctx().input(|i| i.modifiers.shift);
                *action = Some(if shift {
                    TreeAction::Reference(partner.id.clone())
                } else {
                    TreeAction::View(partner.id.clone())
                });
            }
            // Aufwärtspfeil über dem Ehepartner der Referenz: dessen Vater
            // (Fallback Mutter) als Referenz.
            if view == TreeView::Descendants && person.id == root {
                let parents = data.parents_of(&partner.id);
                let target = parents
                    .iter()
                    .find(|parent| parent.gender == crate::model::Gender::Male)
                    .or_else(|| parents.first());
                if let Some(parent) = target {
                    draw_up_badge(painter, partner_card, zoom, orientation, action, &parent.id);
                }
            }
            if partner_long && !*long_press_used {
                *action = Some(TreeAction::Reference(partner.id.clone()));
                *long_press_used = true;
            }
            // Partner-Karte verschieben (Shift+Ziehen) — der Versatz gilt
            // entlang der Verteilungsachse in beiden Ausrichtungen und wird
            // in der Layout-Datei gesichert.
            let (partner_delta, partner_start) = painter.ctx().input(|i| {
                if !(i.pointer.primary_down() && i.modifiers.shift) {
                    return (0.0, false);
                }
                let pressed_here = i
                    .pointer
                    .press_origin()
                    .is_some_and(|q| partner_card.contains(q));
                let active = card_drag
                    .as_ref()
                    .is_some_and(|(id, _)| id == partner.id.as_str());
                if !(pressed_here || active) {
                    return (0.0, false);
                }
                let delta = i.pointer.delta();
                (
                    if orientation == TreeOrientation::Vertical {
                        delta.x
                    } else {
                        delta.y
                    },
                    pressed_here,
                )
            });
            if partner_delta != 0.0 || (partner_start && card_drag.is_none()) {
                let layout_delta = partner_delta / zoom;
                // Ziehen an der Pseudo-Partnerkarte verschiebt die zugehörige
                // Person — der Partner folgt automatisch (ein eigener Versatz
                // würde ihn in Nachbarblöcke hineinzeichnen).
                if let Some((_, members)) = card_drag {
                    for id in members.clone() {
                        *manual_offsets.entry(id).or_insert(0.0) += layout_delta;
                    }
                } else {
                    *manual_offsets.entry(person.id.clone()).or_insert(0.0) += layout_delta;
                    *card_drag = Some((person.id.clone(), vec![person.id.clone()]));
                }
            }
        }
    }
    if (drag_started || drag_ended)
        && orientation == TreeOrientation::Vertical
    {
        let mut total = 0.0f32;
        let mut count = 0usize;
        for f in &data.families {
            if let (Some(a), Some(b)) = (
                f.parent_a.as_deref().and_then(&pos),
                f.parent_b.as_deref().and_then(&pos),
            ) {
                let d = (a.x - b.x).abs();
                total += d;
                count += 1;
                if drag_ended && count <= 6 {
                    let ea = eff_owned.get(f.parent_a.as_deref().unwrap_or("")).copied().unwrap_or(0.0);
                    let eb = eff_owned.get(f.parent_b.as_deref().unwrap_or("")).copied().unwrap_or(0.0);
                    let oa = manual_offsets.get(f.parent_a.as_deref().unwrap_or("")).copied().unwrap_or(0.0);
                    let ob = manual_offsets.get(f.parent_b.as_deref().unwrap_or("")).copied().unwrap_or(0.0);
                    println!("  FAM {}: {}({:.0}) <-> {}({:.0}) dist={:.1} offsets=({:.1},{:.1}) eff=({:.1},{:.1})",
                        f.id, f.parent_a.as_deref().unwrap_or("?"), a.x,
                        f.parent_b.as_deref().unwrap_or("?"), b.x, d, oa, ob, ea, eb);
                }
            }
        }
        let avg = if count > 0 { total / count as f32 } else { 0.0 };
        if drag_started {
            println!("DRAG_START avg_parent_dist={:.1} ({} families, view={:?}, zoom={:.3}, root={} gen_limit={} visible={} peak_eff={:.1})",
                avg, count, view, zoom, root, generation_limit, rows.iter().map(|r| r.len()).sum::<usize>(), peak_eff);
        } else {
            println!("DRAG_END   avg_parent_dist={:.1} ({} families, view={:?}, zoom={:.3}, root={} gen_limit={} visible={} peak_eff={:.1})",
                avg, count, view, zoom, root, generation_limit, rows.iter().map(|r| r.len()).sum::<usize>(), peak_eff);
        }
    }
    content_bounds
}

/// Berechnet die effektiven Versätze der sichtbaren Karten: manuelle Offsets
/// sind der Startwert; ein Aufwärtspass vererbt Versätze an die Vorfahren, ein
/// Abwärtspass zentriert Kinder zwischen ihren Eltern. Manuell verschobene
/// Kinder behalten ihren Eigenversatz und folgen den Eltern zusätzlich (minus
/// eigenem Startanteil), statt beim Eltern-Ziehen stehen zu bleiben.
/// Gibt die Versätze als eigenständige Map zurück (String-Snapshots).
fn effective_offsets<'a>(
    data: &'a TreeData,
    levels: &HashMap<&'a str, usize>,
    drawn: &HashMap<&'a str, f32>,
    manual_offsets: &HashMap<String, f32>,
    view: TreeView,
) -> HashMap<String, f32> {
    let mut eff: HashMap<&'a str, f32> = HashMap::new();
    for (id, offset) in manual_offsets {
        if drawn.contains_key(id.as_str()) {
            if let Some(&key) = levels.keys().find(|key| **key == id.as_str()) {
                eff.insert(key, *offset);
            }
        }
    }
    let mut ordered_families: Vec<(usize, Vec<&'a str>, Vec<&'a str>)> = Vec::new();
    for family in &data.families {
        let children: Vec<&'a str> = family
            .children
            .iter()
            .filter(|child| drawn.contains_key(child.as_str()))
            .map(|child| child.as_str())
            .collect();
        if children.is_empty() {
            continue;
        }
        let parents: Vec<&'a str> = [&family.parent_a, &family.parent_b]
            .into_iter()
            .flatten()
            .filter(|parent| drawn.contains_key(parent.as_str()))
            .map(|parent| parent.as_str())
            .collect();
        if parents.is_empty() {
            continue;
        }
        let child_level = children
            .iter()
            .filter_map(|child| levels.get(child).copied())
            .min()
            .unwrap_or(0);
        ordered_families.push((child_level, children, parents));
    }
    ordered_families.sort_by_key(|(level, _, _)| *level);
    if view == TreeView::Descendants {
        // Nachfahrensicht: Kinder erben vom Pfad-Elternteil (Junction-Mittel).
        for (_level, children, parents) in &ordered_families {
            let path_shifts: Vec<f32> = parents
                .iter()
                .filter_map(|parent| eff.get(*parent).copied())
                .collect();
            let jshift = if path_shifts.is_empty() {
                0.0
            } else {
                path_shifts.iter().sum::<f32>() / path_shifts.len() as f32
            };
            for child in children {
                *eff.entry(child).or_insert(0.0) += jshift;
            }
        }
    } else {
        // Vorfahrensicht: Start-Versatz je Familie merken (nur manuelle
        // Kinder-Offsets, vor der Propagation).
        let mut family_cshift: HashMap<Vec<&'a str>, f32> = HashMap::new();
        for (_level, children, _parents) in &ordered_families {
            let seed = children
                .iter()
                .map(|child| eff.get(*child).copied().unwrap_or(0.0))
                .sum::<f32>()
                / children.len() as f32;
            family_cshift.insert(children.clone(), seed);
        }
        // 1. Aufwärtspass: Vorfahren 1:1 mitverschieben, wenn das Kind verschoben ist.
        for (_level, children, parents) in &ordered_families {
            let cshift = children
                .iter()
                .map(|child| eff.get(*child).copied().unwrap_or(0.0))
                .sum::<f32>()
                / children.len() as f32;
            if cshift != 0.0 {
                for parent in parents {
                    *eff.entry(parent).or_insert(0.0) += cshift;
                }
            }
        }
        // 2. Abwärtspass: Kinder mittig zwischen ihren Eltern; manuell
        // verschobene Kinder folgen den Eltern mit (eigener Offset + Versatz
        // durch die Eltern - eigener Startanteil).
        let mut ordered_down = ordered_families.clone();
        ordered_down.sort_by_key(|(level, _, _)| std::cmp::Reverse(*level));
        for (_level, children, parents) in &ordered_down {
            let pshift = parents
                .iter()
                .map(|parent| eff.get(*parent).copied().unwrap_or(0.0))
                .sum::<f32>()
                / parents.len() as f32;
            for child in children {
                if manual_offsets.contains_key(*child) {
                    let own = manual_offsets.get(*child).copied().unwrap_or(0.0);
                    let seed = family_cshift.get(children).copied().unwrap_or(0.0);
                    eff.insert(child, own + pshift - seed);
                } else {
                    eff.insert(child, pshift);
                }
            }
        }
    }
    eff.iter()
        .map(|(id, value)| ((*id).to_string(), *value))
        .collect()
}

/// Kollisions-Blockade der Vorfahrensicht: Nach allen manuellen Versätzen
/// wird je Ebene geprüft, ob die mitbewegten Karten des gezogenen Zweigs
/// (Person + ganze Vorfahrenlinie samt Partnern UND dank Abwärtspass auch die
/// Kinder/Nachkommen) mit fest bleibenden Nachbarkarten kollidieren. Statt den
/// Nachbarn zu verschieben, wird der Versatz der gezogenen Person auf die
/// erlaubte Spanne geklemmt (min/max über alle Ebenen). Die Bewegungs-Steigung
/// je Karte wird aus den echten effektiven Versätzen gewonnen
/// (`effective_offsets` mit simuliertem +1.0-Offset), damit auch die
/// Kollisionen der mitgezogenen (z.T. manuell verschobenen) Kinder exakt
/// berücksichtigt sind.
fn block_ancestor_drag<'a>(
    spread: &mut HashMap<&'a str, f32>,
    rows: &[Vec<&'a str>],
    data: &'a TreeData,
    levels: &HashMap<&'a str, usize>,
    widths: &HashMap<&'a str, f32>,
    shown_partners: &HashSet<&'a str>,
    drag_root: &'a str,
    manual_offsets: &mut HashMap<String, f32>,
    gap: f32,
    view: TreeView,
) {
    let visible = |id: &str| levels.contains_key(id);
    let current = manual_offsets.get(drag_root).copied().unwrap_or(0.0);
    if current == 0.0 {
        return;
    }
    // Echte Bewegungs-Steigung je Karte: effektive Versätze aktuell und mit
    // simuliertem +1.0-Offset an der Ziehperson; Differenz = Faktor, mit dem
    // die Karte pro Einheit des Zieh-Offsets wandert (inkl. Kinder).
    let base_eff = effective_offsets(data, levels, spread, manual_offsets, view);
    let mut bumped = manual_offsets.clone();
    bumped.insert(drag_root.to_string(), current + 1.0);
    let bumped_eff = effective_offsets(data, levels, spread, &bumped, view);
    let mut scale: HashMap<&'a str, f32> = HashMap::new();
    for id in levels.keys() {
        let s = bumped_eff.get(*id).copied().unwrap_or(0.0)
            - base_eff.get(*id).copied().unwrap_or(0.0);
        if s.abs() > 1e-4 {
            scale.insert(*id, s);
        }
    }
    // Erlaubte Offset-Spanne: jede mitbewegte Karte darf nicht in eine feste
    // Nachbarkarte ragen — ausgedrückt in Einheiten des Zieh-Offsets.
    let mut lo = f32::NEG_INFINITY;
    let mut hi = f32::INFINITY;
    for ids in rows {
        let mut cards: Vec<(&'a str, f32, f32)> = Vec::new(); // (id, x, width)
        for id in ids {
            let Some(&x) = spread.get(id) else {
                continue;
            };
            let mut w = widths[id];
            for partner in data.partners_of(id) {
                if !visible(&partner.id) && shown_partners.contains(partner.id.as_str()) {
                    if let Some(&px) = spread.get(partner.id.as_str()) {
                        let p_w = widths.get(partner.id.as_str()).copied().unwrap_or(215.0);
                        // Partner-Pseudokarte wird rechts neben der Person
                        // gezeichnet; die gemeinsame Box erstreckt sich bis
                        // zum rechten Rand der Partnerkarte.
                        w = (px + p_w / 2.0) - (x - w / 2.0);
                    }
                }
            }
            cards.push((id, x, w));
        }
        cards.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)));
        for i in 0..cards.len() {
            let (id, x, w) = cards[i];
            let Some(&s) = scale.get(id) else {
                continue;
            };
            if s <= 0.0 {
                // Gegenläufig bewegte Karte (eigener manueller Versatz wirkt
                // entgegen): nicht als blockende Karte behandeln, aber mitziehen.
                continue;
            }
            // Linker Nachbar fest? Dann gilt: x - w/2 >= nbx + nbw/2 + gap.
            if i > 0 {
                let (nb, nbx, nbw) = cards[i - 1];
                if !scale.contains_key(nb) {
                    let cand = (nbx + nbw / 2.0 + gap + w / 2.0 - x) / s + current;
                    lo = lo.max(cand);
                }
            }
            // Rechter Nachbar fest? Dann gilt: x + w/2 <= nbx - nbw/2 - gap.
            if i + 1 < cards.len() {
                let (nb, nbx, nbw) = cards[i + 1];
                if !scale.contains_key(nb) {
                    let cand = (nbx - nbw / 2.0 - gap - w / 2.0 - x) / s + current;
                    hi = hi.min(cand);
                }
            }
        }
    }
    // Offset in die erlaubte Spanne klemmen (stabil: klebt an der Grenze).
    let target = if current < lo {
        lo
    } else if current > hi {
        hi
    } else {
        return;
    };
    let delta = target - current;
    // Mitbewegten Zweig um `delta * s` verschieben (nur dieser; Nachbarn fest).
    for (id, s) in &scale {
        if let Some(value) = spread.get_mut(id) {
            *value += delta * s;
        }
        // Auch Partner-Pseudokarten des Zweigs mitschieben.
        for partner in data.partners_of(id) {
            if !visible(&partner.id)
                && shown_partners.contains(partner.id.as_str())
                && spread.contains_key(partner.id.as_str())
            {
                if let Some(value) = spread.get_mut(partner.id.as_str()) {
                    *value += delta * s;
                }
            }
        }
    }
    if let Some(offset) = manual_offsets.get_mut(drag_root) {
        *offset = target;
    }
}

/// Abstoßungspass, ebenenweise: Pro Ebene (Zeile) zuerst die Karten
/// **innerhalb** der Container auseinanderdrücken, dann die **Container**
/// gegeneinander (starre Blöcke, monotone Rechts-Platzierung). Pro Ebene
/// wird das Paar [innen, außen] bis zu 6× wiederholt, bis sich nichts mehr
/// bewegt. Läuft in jeder Verhandlungsiteration und final NACH den
/// manuellen Verschiebungen (`manual_offsets`), damit geschobene Teilbäume
/// andere Gruppen nicht überlagern.
fn repel_pass<'a>(
    spread: &mut HashMap<&'a str, f32>,
    rows: &[Vec<&'a str>],
    row_order: &[usize],
    data: &'a TreeData,
    levels: &HashMap<&'a str, usize>,
    view: TreeView,
    orientation: TreeOrientation,
    card_h: f32,
    widths: &HashMap<&'a str, f32>,
    gap: f32,
    couple_gap: f32,
    group_of: &HashMap<&'a str, &'a str>,
    shown_partners: &HashSet<&'a str>,
) {
    let visible = |id: &str| levels.contains_key(id);
    // Ausdehnung entlang der Verteilungsachse (Kartenhöhe im horizontalen,
    // Kartenbreite im vertikalen Baum).
    let own_extent = |_row: usize, id: &str| -> f32 {
        if orientation == TreeOrientation::Horizontal {
            card_h
        } else {
            widths[id]
        }
    };
    let partner_extent = |_row: usize, id: &str| -> f32 {
        if orientation == TreeOrientation::Horizontal {
            card_h
        } else {
            widths[id]
        }
    };
    let left_offset = |id: &str| -> f32 {
        own_extent(0, id) / 2.0
    };
    let right_offset = |id: &str| -> f32 {
        let mut r = own_extent(0, id) / 2.0;
        for partner in data.partners_of(id) {
            if !visible(&partner.id) && shown_partners.contains(partner.id.as_str()) {
                r += partner_extent(0, &partner.id) + couple_gap;
            }
        }
        r
    };
    // Breite des TEILBAUMS je Person: eigener Footprint + alle sichtbaren
    // Nachkommen. Die "breiteste Stelle" bleibt bei Kollisionen stehen,
    // schmalere Gruppen weichen aus.
    let mut branch_width: HashMap<&str, f32> = HashMap::new();
    {
        let mut all: Vec<&str> = levels.keys().copied().collect();
        all.sort_by(|a, b| levels[b].cmp(&levels[a])); // tiefste zuerst
        for id in all {
            let mut width = own_extent(0, id);
            for partner in data.partners_of(id) {
                if !visible(&partner.id) && shown_partners.contains(partner.id.as_str()) {
                    width += partner_extent(0, &partner.id) + couple_gap;
                }
            }
            for child in data.children_of(id) {
                if let Some(child_width) = branch_width.get(child.id.as_str()) {
                    width += *child_width + gap;
                }
            }
            branch_width.insert(id, width);
        }
    }
    // Zeilen von der BREITESTEN zur schmalsten verhandeln.
    for &row in row_order {
        let ids = &rows[row];
        if view == TreeView::Fan {
            continue;
        }
        let mut members: Vec<(&str, f32)> = ids
            .iter()
            .map(|id| {
                let mut footprint = own_extent(row, id);
                for partner in data.partners_of(id) {
                    if !visible(&partner.id) && shown_partners.contains(partner.id.as_str()) {
                        footprint += partner_extent(row, &partner.id) + couple_gap;
                    }
                }
                (*id, footprint)
            })
            .collect();
        // Gruppenanker: Kinder immer am Eltern-Junction (Kind mittig
        // zwischen seinen Vorfahren); im Vorfahrenbaum zusätzlich die
        // Elternpaare am Kinder-Mittelwert. Die Mitglieder werden nach
        // diesem Anker sortiert, damit jede Familie als ZUSAMMENHÄNGENDER
        // Block liegt und Container nur die eigenen Kinder umschließen –
        // sonst "verweben" Geschwister verschiedener Familien.
        let mut anchors: HashMap<&str, f32> = HashMap::new();
        for family in &data.families {
            let children: Vec<&str> = family
                .children
                .iter()
                .filter(|child| spread.contains_key(child.as_str()))
                .map(|child| child.as_str())
                .collect();
            if children.is_empty() {
                continue;
            }
            let parents: Vec<&str> = [&family.parent_a, &family.parent_b]
                .into_iter()
                .flatten()
                .filter(|parent| spread.contains_key(parent.as_str()))
                .map(|parent| parent.as_str())
                .collect();
            if parents.is_empty() {
                continue;
            }
            let junction = parents.iter().map(|p| spread[*p]).sum::<f32>() / parents.len() as f32;
            for &child in &children {
                anchors.insert(child, junction);
            }
            if view != TreeView::Descendants {
                let child_mean =
                    children.iter().map(|c| spread[*c]).sum::<f32>() / children.len() as f32;
                for parent in parents {
                    anchors.insert(parent, child_mean);
                }
            }
        }
        let birth_key = |id: &'a str| -> (i32, i32, i32, i32, &'a str) {
            if let Some(p) = data.find(id) {
                if let Some((y, m, d)) = crate::model::parse_birth_date(&p.birth) {
                    (0, y, m, d, p.id.as_str())
                } else {
                    (1, 0, 0, 0, p.id.as_str())
                }
            } else {
                (1, 0, 0, 0, id)
            }
        };
        members.sort_by(|a, b| {
            let anchor_a = anchors.get(a.0).copied().unwrap_or(a.1);
            let anchor_b = anchors.get(b.0).copied().unwrap_or(b.1);
            anchor_a.total_cmp(&anchor_b).then_with(|| {
                let key_a = birth_key(a.0);
                let key_b = birth_key(b.0);
                key_a.cmp(&key_b)
            })
        });
        fn group_key<'b>(group_of: &'b HashMap<&'b str, &'b str>, id: &'b str) -> &'b str {
            group_of.get(id).copied().unwrap_or(id)
        }
        let mut groups: Vec<Vec<(&str, f32)>> = Vec::new();
        for member in members {
            let key = group_key(group_of, member.0);
            match groups.last_mut() {
                Some(last) if group_key(group_of, last[0].0) == key => last.push(member),
                _ => groups.push(vec![member]),
            }
        }
        // Ebenenweise verhandeln: [innen → außen] wiederholen, bis stabil (max 24 Versuche).
        for _ in 0..24 {
            let mut moved = false;
            // 1) Innen: Karten einer Gruppe (Container) auseinanderdrücken.
            for group in &groups {
                let mut inner: Vec<(&str, f32)> = group
                    .iter()
                    .map(|(id, _)| (*id, spread[*id]))
                    .collect();
                inner.sort_by(|a, b| {
                    let key_a = birth_key(a.0);
                    let key_b = birth_key(b.0);
                    key_a.cmp(&key_b)
                });
                for window in inner.windows(2) {
                    let need = right_offset(window[0].0) + left_offset(window[1].0) + gap;
                    let actual = window[1].1 - window[0].1;
                    if actual < need {
                        // Nur nach rechts schieben: monotone Platzierung,
                        // dadurch keine neu erzeugten Überlappungen links.
                        let deficit = need - actual;
                        if let Some(value) = spread.get_mut(window[1].0) {
                            *value += deficit;
                            moved = true;
                        }
                    }
                }
            }
            // 2) Außen: ganze Container als starre Blöcke gegeneinander.
            type GroupSpan<'b> = (Vec<&'b str>, f32, f32, f32);
            let mut spans: Vec<GroupSpan> = groups
                .iter()
                .map(|group| {
                    let start = group
                        .iter()
                        .map(|(id, _)| spread[*id] - left_offset(id))
                        .fold(f32::MAX, f32::min);
                    let end = group
                        .iter()
                        .map(|(id, _)| spread[*id] + right_offset(id))
                        .fold(f32::MIN, f32::max);
                    // Teilbaum-Breite entscheidet, WER bei Kollision weicht.
                    let branch: f32 = group
                        .iter()
                        .map(|(id, footprint)| branch_width.get(id).copied().unwrap_or(*footprint))
                        .sum();
                    (
                        group.iter().map(|(id, _)| *id).collect(),
                        start,
                        end,
                        branch,
                    )
                })
                .collect();
            spans.sort_by(|a, b| a.1.total_cmp(&b.1));
            for window in spans.windows(2) {
                let need = gap;
                let actual = window[1].1 - window[0].2; // right_start - left_end
                if actual < need {
                    let deficit = need - actual;
                    // Die SCHMÄLERE Gruppe (kleinere Teilbaum-Breite) weicht
                    // aus; die breitere bleibt stehen.
                    if window[1].3 <= window[0].3 {
                        // rechte Gruppe weicht nach rechts.
                        for id in &window[1].0 {
                            if let Some(value) = spread.get_mut(*id) {
                                *value += deficit;
                                moved = true;
                            }
                        }
                    } else {
                        // linke Gruppe weicht nach links.
                        for id in &window[0].0 {
                            if let Some(value) = spread.get_mut(*id) {
                                *value -= deficit;
                                moved = true;
                            }
                        }
                    }
                }
            }
            if !moved {
                break;
            }
        }
    }
}

/// Aufwärtspfeil-Badge (Nachfahrensicht): setzt den Vater – Fallback
/// Mutter – der Karte als Referenzperson. Position: im vertikalen Baum
/// über der Karte, im horizontalen Baum links daneben. Der Pfeil zeigt
/// IMMER nach oben. Eingeblendet nur beim Hovern über der Karte.
fn draw_up_badge(
    painter: &egui::Painter,
    card: Rect,
    zoom: f32,
    orientation: TreeOrientation,
    action: &mut Option<TreeAction>,
    target_id: &str,
) {
    let (badge_at, _arrow_vec) = if orientation == TreeOrientation::Vertical {
        (
            Pos2::new(card.center().x, card.top() - 13.0 * zoom),
            Vec2::new(0.0, -9.0 * zoom),
        )
    } else {
        (
            Pos2::new(card.left() - 13.0 * zoom, card.center().y),
            Vec2::new(-9.0 * zoom, 0.0),
        )
    };
    let badge_r = 9.0 * zoom;
    let badge_rect = Rect::from_center_size(badge_at, Vec2::splat(badge_r * 2.0));
    // Hover-Zone: Karte ∪ Badge (Badge liegt teils außerhalb der Karte).
    let hovered = painter.ctx().input(|i| {
        i.pointer
            .interact_pos()
            .is_some_and(|q| card.contains(q) || badge_rect.contains(q))
    });
    if !hovered {
        return;
    }
    painter.circle_filled(badge_at, badge_r, Color32::from_rgb(24, 40, 48));
    painter.circle_stroke(
        badge_at,
        badge_r,
        Stroke::new(1.0, Color32::from_rgb(120, 170, 160)),
    );
    // Richtung immer nach oben (zur vorherigen Generation).
    painter.arrow(
        badge_at + Vec2::new(0.0, 4.5 * zoom),
        Vec2::new(0.0, -9.0 * zoom),
        Stroke::new(1.5, Color32::from_rgb(158, 213, 199)),
    );
    let clicked = painter.ctx().input(|i| {
        i.pointer.any_click()
            && i.pointer
                .interact_pos()
                .is_some_and(|q| badge_rect.contains(q))
    });
    if clicked {
        *action = Some(TreeAction::Reference(target_id.to_string()));
    }
}

/// Benötigte Kartenbreite für Name + Geburtszeile (Layout-Einheiten).
fn card_width_for(person: &crate::model::Person, painter: &egui::Painter) -> f32 {
    let name = painter
        .layout_no_wrap(
            person.display_name(),
            FontId::proportional(13.0),
            Color32::WHITE,
        )
        .size()
        .x;
    let birth_text = if person.birth.is_empty() {
        "Unbekannt".to_string()
    } else {
        person.birth.clone()
    };
    let birth = painter
        .layout_no_wrap(birth_text, FontId::proportional(12.0), Color32::WHITE)
        .size()
        .x;
    (64.0 + name.max(birth) + 20.0).max(215.0)
}

/// Eine Personenkarte zeichnen; Rückgabe true bei Klick auf die Karte.
/// `muted` = Partner-Pseudokarte (dezentere Umrandung).
fn draw_person_card(
    painter: &egui::Painter,
    person: &crate::model::Person,
    at: Pos2,
    width: f32,
    zoom: f32,
    selected_now: bool,
    muted: bool,
    media_base: &std::path::Path,
    photo_cache: &mut HashMap<String, TextureHandle>,
) -> bool {
    // Höhe 78.0 entspricht `card_h` im Layout oben.
    let size = Vec2::new(width, 78.0) * zoom;
    let card = Rect::from_center_size(at, size);
    let fill = match (person.gender, selected_now) {
        (_, true) => Color32::from_rgb(43, 121, 113),
        (crate::model::Gender::Female, _) => Color32::from_rgb(112, 66, 72),
        (crate::model::Gender::Male, _) => Color32::from_rgb(50, 70, 108),
        _ => Color32::from_rgb(56, 65, 75),
    };
    let stroke_color = if muted {
        Color32::from_rgb(108, 138, 138)
    } else {
        Color32::from_rgb(158, 213, 199)
    };
    painter.rect(
        card,
        10. * zoom,
        fill,
        Stroke::new(
            if selected_now {
                2.5
            } else if muted {
                0.8
            } else {
                1.0
            },
            stroke_color,
        ),
        egui::StrokeKind::Outside,
    );
    let avatar_size = 48. * zoom;
    let avatar = Rect::from_center_size(
        card.left_center() + Vec2::new(35. * zoom, 0.),
        Vec2::splat(avatar_size),
    );
    if let Some(texture) =
        round_avatar_texture_cached(painter.ctx(), person, photo_cache, media_base)
    {
        painter.circle_filled(
            avatar.center(),
            avatar_size / 2.,
            Color32::from_black_alpha(24),
        );
        painter.image(
            texture.id(),
            avatar,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.circle_stroke(
            avatar.center(),
            avatar_size / 2.,
            Stroke::new(1.0, Color32::from_white_alpha(45)),
        );
    } else {
        painter.circle_filled(
            avatar.center(),
            avatar_size / 2.,
            Color32::from_rgb(55, 91, 101),
        );
        painter.text(
            avatar.center(),
            Align2::CENTER_CENTER,
            initials(person),
            FontId::proportional(13. * zoom),
            Color32::WHITE,
        );
    }
    painter.text(
        card.left_top() + Vec2::new(64. * zoom, 21. * zoom),
        Align2::LEFT_CENTER,
        person.display_name(),
        FontId::proportional(13. * zoom),
        Color32::WHITE,
    );
    painter.text(
        card.left_bottom() + Vec2::new(64. * zoom, -17. * zoom),
        Align2::LEFT_CENTER,
        if person.birth.is_empty() {
            "Unbekannt"
        } else {
            &person.birth
        },
        FontId::proportional(12. * zoom),
        Color32::from_rgb(202, 222, 221),
    );
    painter.ctx().input(|i| {
        i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|q| card.contains(q))
    })
}

/// Berechnet rekursiv ein absolut überschneidungsfreies, mathematisch perfektes Vorfahren-Layout (Binärbaum).
/// - Väter stehen immer links, Mütter immer rechts.
/// - Das Kind steht genau zentriert im Abstand zwischen Vater und Mutter.
/// - Subtree-Abstände werden ebenenweise verhandelt, damit sich auch entfernte Zweige niemals überlagern.
fn layout_ancestors<'a>(
    id: &'a str,
    data: &'a TreeData,
    levels: &HashMap<&str, usize>,
    widths: &HashMap<&str, f32>,
    gap: f32,
) -> HashMap<&'a str, f32> {
    let mut layout = HashMap::new();
    layout.insert(id, 0.0);

    // Wenn der Knoten nicht sichtbar ist (Limit erreicht), sind wir fertig.
    if !levels.contains_key(id) {
        return layout;
    }

    // Finde Väter (männlich/links) und Mütter (weiblich/rechts) im sichtbaren Baum
    let parents = data.parents_of(id);
    let mut father = None;
    let mut mother = None;
    for p in parents {
        if levels.contains_key(p.id.as_str()) {
            if p.gender == crate::model::Gender::Male {
                father = Some(p.id.as_str());
            } else if p.gender == crate::model::Gender::Female {
                mother = Some(p.id.as_str());
            } else if father.is_none() {
                father = Some(p.id.as_str());
            } else {
                mother = Some(p.id.as_str());
            }
        }
    }

    match (father, mother) {
        (Some(f), Some(m)) => {
            // Rekursiv die Layouts für Vater- und Mutter-Teilbäume berechnen (jeweils mit 0.0 als lokales Zentrum)
            let f_layout = layout_ancestors(f, data, levels, widths, gap);
            let m_layout = layout_ancestors(m, data, levels, widths, gap);

            // Bestimme den minimalen Abstand, den wir zwischen dem Vater-Teilbaum und dem Mutter-Teilbaum brauchen,
            // damit sich auf KEINER Ebene (Generation) die Karten überlappen.
            let mut min_distance = 0.0f32;

            // Sammle alle Generationenebenen (Level), die in beiden Teilbäumen vorkommen
            let mut levels_in_subtrees: std::collections::HashSet<usize> =
                std::collections::HashSet::new();
            for &fid in f_layout.keys() {
                if let Some(&lvl) = levels.get(fid) {
                    levels_in_subtrees.insert(lvl);
                }
            }
            for &mid in m_layout.keys() {
                if let Some(&lvl) = levels.get(mid) {
                    levels_in_subtrees.insert(lvl);
                }
            }

            for lvl in levels_in_subtrees {
                // Maximale rechte Position im Vater-Teilbaum auf dieser Ebene finden
                let mut max_f_right = None;
                for (&fid, &f_offset) in &f_layout {
                    if levels.get(fid) == Some(&lvl) {
                        let w = widths.get(fid).copied().unwrap_or(215.0);
                        let right = f_offset + w / 2.0;
                        if max_f_right.is_none() || right > max_f_right.unwrap() {
                            max_f_right = Some(right);
                        }
                    }
                }

                // Minimale linke Position im Mutter-Teilbaum auf dieser Ebene finden
                let mut min_m_left = None;
                for (&mid, &m_offset) in &m_layout {
                    if levels.get(mid) == Some(&lvl) {
                        let w = widths.get(mid).copied().unwrap_or(215.0);
                        let left = m_offset - w / 2.0;
                        if min_m_left.is_none() || left < min_m_left.unwrap() {
                            min_m_left = Some(left);
                        }
                    }
                }

                if let (Some(f_right), Some(m_left)) = (max_f_right, min_m_left) {
                    // Der benötigte Abstand auf dieser Ebene: rechter Rand Vater + Mindestabstand - linker Rand Mutter
                    let needed_sep = f_right - m_left + gap;
                    if needed_sep > min_distance {
                        min_distance = needed_sep;
                    }
                }
            }

            // Standardabstand aus Kartenbreiten, falls keine gemeinsamen Ebenen vorliegen
            let wf = widths.get(f).copied().unwrap_or(215.0);
            let wm = widths.get(m).copied().unwrap_or(215.0);
            let default_sep = (wf + wm) / 2.0 + gap;
            let sep = min_distance.max(default_sep);

            // Zentrierung relativ zur Lücke (Zwischenraum) zwischen den beiden Eltern:
            // (c1.r + c2.l)/2 = d1.c -> shift_f + shift_m = (wm - wf) / 2
            let shift_f = -sep / 2.0 + (wm - wf) / 4.0;
            let shift_m = sep / 2.0 + (wm - wf) / 4.0;

            for (fid, f_offset) in f_layout {
                layout.insert(fid, f_offset + shift_f);
            }
            for (mid, m_offset) in m_layout {
                layout.insert(mid, m_offset + shift_m);
            }
        }
        (Some(p_id), None) | (None, Some(p_id)) => {
            // Nur ein Elternteil vorhanden -> Direkt zentriert darüber platzieren
            let p_layout = layout_ancestors(p_id, data, levels, widths, gap);
            for (id, offset) in p_layout {
                layout.insert(id, offset);
            }
        }
        (None, None) => {}
    }

    layout
}
