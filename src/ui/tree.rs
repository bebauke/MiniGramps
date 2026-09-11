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

use crate::media::{cover_uv_to, initials, photo_preview_texture, round_avatar_texture_cached};
use crate::model::{Person, TreeData};
use crate::ui::CardLayout;

const CARD_DETAIL_MIN_ZOOM: f32 = 0.4;

/// Fester Mindestabstand zwischen Karten. Der einstellbare Baum-Abstand ist
/// der Default (Zielabstand) der automatischen Platzierung; darunter dürfen
/// Karten nie rücken, damit sie sich nicht berühren.
const MIN_CARD_GAP: f32 = 16.0;

/// Für einen Layoutdurchlauf vorbereitete Beziehungen. Das vermeidet, dass
/// `children_of`/`parents_of`/`partners_of` in jedem Kollisionspass erneut
/// alle Familien und Personen linear durchsuchen.
struct TreeRelations<'a> {
    people: HashMap<&'a str, &'a Person>,
    children: HashMap<&'a str, Vec<&'a Person>>,
    parents: HashMap<&'a str, Vec<&'a Person>>,
    partners: HashMap<&'a str, Vec<&'a Person>>,
    couple_children: HashMap<&'a str, HashMap<&'a str, Vec<&'a str>>>,
}

impl<'a> TreeRelations<'a> {
    fn new(data: &'a TreeData) -> Self {
        let people: HashMap<&str, &Person> = data
            .people
            .iter()
            .map(|person| (person.id.as_str(), person))
            .collect();
        let mut children: HashMap<&str, Vec<&Person>> = HashMap::new();
        let mut parents: HashMap<&str, Vec<&Person>> = HashMap::new();
        let mut partners: HashMap<&str, Vec<&Person>> = HashMap::new();
        let mut couple_children: HashMap<&str, HashMap<&str, Vec<&str>>> = HashMap::new();
        for family in &data.families {
            let parent_ids: Vec<&str> = [&family.parent_a, &family.parent_b]
                .into_iter()
                .flatten()
                .map(String::as_str)
                .collect();
            for parent_id in &parent_ids {
                for child_id in &family.children {
                    if let Some(child) = people.get(child_id.as_str()) {
                        children.entry(parent_id).or_default().push(*child);
                    }
                }
            }
            for child_id in &family.children {
                for parent_id in &parent_ids {
                    if let Some(parent) = people.get(parent_id) {
                        parents.entry(child_id).or_default().push(*parent);
                    }
                }
            }
            if let (Some(a), Some(b)) = (family.parent_a.as_deref(), family.parent_b.as_deref()) {
                let (first, second) = couple_key(a, b);
                couple_children
                    .entry(first)
                    .or_default()
                    .entry(second)
                    .or_default()
                    .extend(family.children.iter().map(String::as_str));
                if let Some(person) = people.get(b) {
                    partners.entry(a).or_default().push(*person);
                }
                if let Some(person) = people.get(a) {
                    partners.entry(b).or_default().push(*person);
                }
            }
        }
        for list in children.values_mut() {
            list.sort_by(|a, b| birth_key(a).cmp(&birth_key(b)));
        }
        for (id, list) in &mut partners {
            let Some(person) = people.get(id).copied() else {
                continue;
            };
            let manual = data.partner_order.get(*id);
            list.sort_by(|a, b| {
                partner_key(data, person, a, manual)
                    .cmp(&partner_key(data, person, b, manual))
            });
        }
        Self {
            people,
            children,
            parents,
            partners,
            couple_children,
        }
    }

    fn find(&self, id: &str) -> Option<&'a Person> {
        self.people.get(id).copied()
    }

    fn children_of(&self, id: &str) -> &[&'a Person] {
        self.children.get(id).map(Vec::as_slice).unwrap_or_default()
    }

    fn parents_of(&self, id: &str) -> &[&'a Person] {
        self.parents.get(id).map(Vec::as_slice).unwrap_or_default()
    }

    fn partners_of(&self, id: &str) -> &[&'a Person] {
        self.partners.get(id).map(Vec::as_slice).unwrap_or_default()
    }

    fn children_of_couple(&self, parent_a: &str, parent_b: &str) -> &[&'a str] {
        let (first, second) = couple_key(parent_a, parent_b);
        self.couple_children
            .get(first)
            .and_then(|children| children.get(second))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

fn couple_key<'a>(parent_a: &'a str, parent_b: &'a str) -> (&'a str, &'a str) {
    if parent_a <= parent_b {
        (parent_a, parent_b)
    } else {
        (parent_b, parent_a)
    }
}

fn birth_key(person: &Person) -> (i32, i32, i32, i32, &str) {
    if let Some((year, month, day)) = crate::model::parse_birth_date(&person.birth) {
        (0, year, month, day, person.id.as_str())
    } else {
        (1, 0, 0, 0, person.id.as_str())
    }
}

fn partner_key<'a>(
    data: &TreeData,
    person: &Person,
    partner: &'a Person,
    manual: Option<&Vec<String>>,
) -> (u8, (i32, i32, i32), (i32, i32, i32), usize, &'a str) {
    let (together, marriage) = data.get_partnership_dates(person, partner);
    let (priority, first, second) = if let Some(date) = together {
        (0, date, marriage.unwrap_or((9999, 12, 31)))
    } else if let Some(date) = marriage {
        (1, date, (9999, 12, 31))
    } else {
        (2, (9999, 12, 31), (9999, 12, 31))
    };
    let manual_position = manual
        .and_then(|order| order.iter().position(|id| id == &partner.id))
        .unwrap_or(usize::MAX);
    (priority, first, second, manual_position, partner.id.as_str())
}

/// Ansichtsmodus (Schalter in der Stammbaum-Werkzeugleiste, `ui`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeView {
    Descendants,
    Ancestors,
    Fan,
}

/// Ausrichtung der Generationsachse (Schalter in der Werkzeugleiste).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
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
    /// Partner-Pseudokarten per Shift+Ziehen tauschen (nur freie Partner;
    /// `direction` = -1 nach vorn, +1 nach hinten).
    SwapPartner {
        person_id: String,
        partner_id: String,
        direction: i32,
    },
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
    swap_latch: &mut bool,
    card_drag: &mut Option<(String, Vec<String>)>,
    frame_drag: &mut bool,
    generation_limit: usize,
    layout_gap: f32,
    manual_offsets: &mut HashMap<String, f32>,
    media_base: &std::path::Path,
    photo_cache: &mut HashMap<String, TextureHandle>,
    view: TreeView,
    orientation: TreeOrientation,
    card_layout: CardLayout,
    zoom: f32,
    pan: Vec2,
    drag_started: bool,
    drag_ended: bool,
    log_layout: bool,
) -> Rect {
    let relations = TreeRelations::new(data);
    let root: &str = match reference.filter(|id| relations.find(id).is_some()) {
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
            relations.parents_of(id)
        } else {
            relations.children_of(id)
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
                relations.parents_of(prev_id)
            } else {
                relations.children_of(prev_id)
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
        for partner in relations.partners_of(&person.id) {
            if visible(&partner.id) || shown_partners.contains(partner.id.as_str()) {
                continue;
            }
            if view != TreeView::Ancestors
                || relations
                    .children_of_couple(&person.id, &partner.id)
                    .iter()
                    .any(|child| levels.contains_key(*child))
            {
                shown_partners.insert(partner.id.as_str());
            }
        }
    }
    // Layout-Konstanten (Kartenhöhe auch in `draw_person_card`).
    let card_h = card_layout.height();
    // Paarabstand: im horizontalen Baum stapeln Paare grundsätzlich OHNE
    // Zwischenraum (Verschieben regelt der Benutzer per Shift+Ziehen),
    // im vertikalen Baum knapp getrennt.
    let couple_gap = if orientation == TreeOrientation::Horizontal {
        0.0f32
    } else {
        4.0f32
    };
    let gap = layout_gap;
    // Geschwister-Container werden beim Zeichnen je Seite um 12 Einheiten
    // erweitert. Der Gruppenabstand berücksichtigt diese Außenpolster, damit
    // sich benachbarte Container sichtbar abstoßen statt nur zu berühren.
    let sibling_container_padding = 12.0f32;
    let gen_gap = 36.0f32;
    let widths: HashMap<&str, f32> = levels
        .keys()
        .copied()
        .chain(shown_partners.iter().copied())
        .filter_map(|id| relations.find(id))
        .map(|person| {
            (
                person.id.as_str(),
                card_width_for(person, painter, card_layout),
            )
        })
        .collect();
    let row_widths: Vec<f32> = rows
        .iter()
        .map(|ids| {
            // Basisbreite je Kartenlayout (Portrait darf schmaler sein als
            // Compact); sonst würden im Horizontalen alle Karten auf 215
            // gestreckt.
            let mut width = match card_layout {
                CardLayout::Compact => 215.0f32,
                CardLayout::Portrait => 160.0f32,
            };
            for id in ids {
                width = width.max(widths[id]);
                for partner in relations.partners_of(id) {
                    if !visible(&partner.id) && shown_partners.contains(partner.id.as_str()) {
                        width = width.max(widths[partner.id.as_str()]);
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
    // Diagnose: Zeilenreihenfolge je Layout-Stufe ausgeben (nur bei log_layout).
    let dump_stage = |stage: &str, spread: &HashMap<&str, f32>| {
        if !log_layout {
            return;
        }
        for (row, ids) in rows.iter().enumerate() {
            let mut items: Vec<(&str, f32)> = ids
                .iter()
                .filter_map(|id| spread.get(*id).map(|x| (*id, *x)))
                .collect();
            items.sort_by(|a, b| a.1.total_cmp(&b.1));
            let order = items
                .iter()
                .map(|(id, x)| format!("{}@{:.0}", id, x))
                .collect::<Vec<_>>()
                .join(" ");
            log::debug!("STAGE_{} row={} [{}]", stage, row, order);
        }
    };
    // Diagnose: große Einzelverschiebungen (>1000) je Verhandlungsteilschritt.
    let log_big_moves = |prev: &HashMap<&str, f32>, spread: &HashMap<&str, f32>, stage: &str| {
        if !log_layout {
            return;
        }
        let mut moves: Vec<(&str, f32, f32)> = spread
            .iter()
            .filter_map(|(id, after)| {
                let before = prev.get(id).copied().unwrap_or(*after);
                ((after - before).abs() > 1000.0).then_some((*id, before, *after))
            })
            .collect();
        moves.sort_by(|a, b| (b.2 - b.1).abs().total_cmp(&(a.2 - a.1).abs()));
        for (id, before, after) in moves.into_iter().take(20) {
            log::debug!(
                "NEG_MOVE stage={} id={} from={:.0} to={:.0} d={:.0}",
                stage,
                id,
                before,
                after,
                after - before
            );
        }
    };
    if view == TreeView::Ancestors {
        // Berechne das perfekte, überschneidungsfreie Vorfahren-Layout rekursiv!
        spread = layout_ancestors(root, &relations, &levels, &widths, gap);

        // Partner-Pseudokarten einmalig an ihre Person koppeln.
        for (row, ids) in rows.iter().enumerate() {
            for id in ids {
                if orientation == TreeOrientation::Vertical {
                    if let Some(&sx) = spread.get(id) {
                        let mut px = sx + card_w(row, id) / 2.0 + couple_gap;
                        for partner in relations.partners_of(id) {
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
                        for partner in relations.partners_of(id) {
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
                for partner in relations.partners_of(id) {
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
        dump_stage("PACK", &spread);
        for _ in 0..12 {
            // 1) Partner-Pseudokarten an ihre Person koppeln.
            let prev_partner = spread.clone();
            for (row, ids) in rows.iter().enumerate() {
                if view == TreeView::Fan {
                    continue;
                }
                for id in ids {
                    if orientation == TreeOrientation::Vertical {
                        let mut px = spread[*id] + card_w(row, id) / 2.0 + couple_gap;
                        for partner in relations.partners_of(id) {
                            if visible(&partner.id) || !shown_partners.contains(partner.id.as_str())
                            {
                                continue;
                            }
                            spread.insert(partner.id.as_str(), px + card_w(row, &partner.id) / 2.0);
                            px += card_w(row, &partner.id) + couple_gap;
                        }
                    } else {
                        let mut py = spread[*id] + card_h / 2.0 + couple_gap;
                        for partner in relations.partners_of(id) {
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
            log_big_moves(&prev_partner, &spread, "partner");
            let prev_attr = spread.clone();
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
            log_big_moves(&prev_attr, &spread, "attr");
            let prev_repel = spread.clone();
            // 3) Abstoßung: innerhalb der Gruppen (Karten) und zwischen den
            //    Gruppen (Container) als starre Blöcke (siehe `repel` oben).
            repel_pass(
                &mut spread,
                &rows,
                &row_order,
                data,
                &relations,
                &levels,
                view,
                orientation,
                card_h,
                &widths,
                gap,
                sibling_container_padding,
                couple_gap,
                &group_of,
                &shown_partners,
            );
            log_big_moves(&prev_repel, &spread, "repel");
        }
    }

    dump_stage("NEG", &spread);

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
    dump_stage("OFF", &spread);
    if view != TreeView::Ancestors {
        // Finaler Abstoßungspass NACH den manuellen Versätzen: nur noch
        // Überlappungen auflösen (MIN_CARD_GAP), damit selbst eng zusammen
        // geschobene Karten so stehen bleiben – der Baum-Abstand ist hier
        // bewusst kein erzwungenes Minimum.
        repel_pass(
            &mut spread,
            &rows,
            &row_order,
            data,
            &relations,
            &levels,
            view,
            orientation,
            card_h,
            &widths,
            MIN_CARD_GAP,
            sibling_container_padding,
            couple_gap,
            &group_of,
            &shown_partners,
        );
    } else if let Some((drag_root, _)) = card_drag {
        block_ancestor_drag(
            &mut spread,
            &rows,
            data,
            &relations,
            &levels,
            &widths,
            &shown_partners,
            drag_root,
            manual_offsets,
            gap,
            view,
        );
    }

    dump_stage("REPEL", &spread);

    // Finale Zentrierung: Gruppen werden minimal (kollisionsfrei) in
    // Richtung ihrer Junction verschoben — Kinder hängen so weit wie möglich
    // senkrecht unter dem Elternpaar, Eltern-Paare über ihrem Kind. Manuell
    // verschobene Personen blockieren ihre Gruppe (bleibt, wo hingeschoben).
    if view != TreeView::Ancestors {
        let manual_ids: HashSet<&str> = manual_offsets.keys().map(|key| key.as_str()).collect();
        let mut group_key_of: HashMap<&str, &str> = HashMap::new();
        for family in &data.families {
            for child in &family.children {
                if spread.contains_key(child.as_str()) {
                    group_key_of.insert(child, family.id.as_str());
                }
            }
        }
        // Von Eltern zu Kindern arbeiten. Vor jeder Generation werden die
        // Anker aus den bereits finalisierten Elternpositionen neu berechnet.
        for row in 0..rows.len() {
            let ids = &rows[row];
            if view == TreeView::Fan || ids.is_empty() {
                continue;
            }
            let mut anchors: HashMap<&str, f32> = HashMap::new();
            for family in &data.families {
                let children: Vec<&str> = family
                    .children
                    .iter()
                    .filter(|child| levels.get(child.as_str()) == Some(&row))
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
                // Exakte Position einer Partner-Pseudokarte aus derselben
                // Reihenfolge und denselben Footprints wie `positions` unten.
                let pseudo_partner_anchor = |owner: &str, wanted: &str| -> Option<f32> {
                    if levels.contains_key(wanted) {
                        return spread.get(wanted).copied();
                    }
                    let owner_row = levels.get(owner).copied()?;
                    let mut cursor = spread.get(owner).copied()?
                        + own_extent(owner_row, owner) / 2.0
                        + couple_gap;
                    for partner in relations.partners_of(owner) {
                        if visible(&partner.id)
                            || !shown_partners.contains(partner.id.as_str())
                        {
                            continue;
                        }
                        let partner_center =
                            cursor + partner_extent(owner_row, &partner.id) / 2.0;
                        if partner.id == wanted {
                            return Some(partner_center);
                        }
                        cursor += partner_extent(owner_row, &partner.id) + couple_gap;
                    }
                    None
                };
                let pair_anchor = if parents.len() == 2 {
                    let a_path = path_nodes.contains(parents[0]);
                    let b_path = path_nodes.contains(parents[1]);
                    if a_path && !b_path {
                        pseudo_partner_anchor(parents[0], parents[1])
                    } else if b_path && !a_path {
                        pseudo_partner_anchor(parents[1], parents[0])
                    } else {
                        None
                    }
                } else {
                    None
                };
                let anchor = pair_anchor.unwrap_or_else(|| {
                    parents.iter().map(|parent| spread[*parent]).sum::<f32>()
                        / parents.len() as f32
                });
                for child in children {
                    anchors.insert(child, anchor);
                }
            }
            let mut members: Vec<(&str, f32)> = ids
                .iter()
                .map(|id| {
                    let mut footprint = own_extent(row, id);
                    for partner in relations.partners_of(id) {
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
            let mut prev_end: Option<(f32, f32)> = None;
            for (index, (group, target, manual, _)) in ordered.iter().enumerate() {
                let container_padding = if group.len() > 1 {
                    sibling_container_padding
                } else {
                    0.0
                };
                let start = group
                    .iter()
                    .map(|(id, fp)| spread[*id] - fp / 2.0)
                    .fold(f32::MAX, f32::min);
                let end = group
                    .iter()
                    .map(|(id, fp)| spread[*id] + fp / 2.0)
                    .fold(f32::MIN, f32::max);
                let next = ordered.get(index + 1).and_then(|(next, _, _, _)| {
                    let start = next.iter().map(|(id, fp)| spread[*id] - fp / 2.0).fold(
                        None::<f32>,
                        |acc, value| {
                            Some(match acc {
                                Some(best) => best.min(value),
                                None => value,
                            })
                        },
                    )?;
                    let padding = if next.len() > 1 {
                        sibling_container_padding
                    } else {
                        0.0
                    };
                    Some((start, padding))
                });
                if let Some(target) = target {
                    if !*manual {
                        let min_d = prev_end
                            .map(|(prev, padding)| {
                                prev + MIN_CARD_GAP + padding + container_padding - start
                            })
                            .unwrap_or(f32::NEG_INFINITY);
                        let max_d = next
                            .map(|(next, padding)| {
                                next - MIN_CARD_GAP - container_padding - padding - end
                            })
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
                    Some(previous) if previous.0 > end_after => previous,
                    _ => (end_after, container_padding),
                });
            }
        }
    }

    let mut positions: HashMap<&str, (f32, f32)> = HashMap::new();
    dump_stage("CENTER", &spread);
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
                for partner in relations.partners_of(id) {
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
                for partner in relations.partners_of(id) {
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
    if log_layout {
        log_layout_diagnostics(
            root,
            data,
            &relations,
            view,
            orientation,
            zoom,
            &rows,
            &levels,
            &positions,
            &widths,
            &shown_partners,
            manual_offsets,
            &eff_owned,
            &group_of,
            card_h,
            gap,
            content_bounds,
        );
    }
    let pos = |id: &str| {
        positions
            .get(id)
            .map(|(x, y)| center + Vec2::new(*x * zoom, *y * zoom))
    };
    let viewport = painter.clip_rect().expand(32.0);
    let line_on_screen = |a: Pos2, b: Pos2| {
        viewport.intersects(Rect::from_two_pos(a, b).expand(4.0))
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
            for partner in relations.partners_of(&person.id) {
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
                if viewport.intersects(outer) {
                    painter.rect_filled(outer, 12.0 * zoom, Color32::from_black_alpha(25));
                    painter.rect_stroke(
                        outer,
                        12.0 * zoom,
                        Stroke::new(1.5, Color32::from_rgb(201, 170, 96)),
                        egui::StrokeKind::Inside,
                    );
                }
                // Rahmen ziehen (Klick+Drag, ohne Shift): der ganze Zweig
                // folgt — Person + sichtbare Partner erhalten den Versatz,
                // die Kinder erben ihn über die Junction-Vererbung.
                let frame_hit = painter.ctx().input(|i| {
                    i.pointer.primary_down()
                        && i.modifiers.shift
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
                        for partner in relations.partners_of(&person.id) {
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
            let mut block_children: Vec<&str> = Vec::new();
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
                for partner in relations.partners_of(child) {
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
                block_children.push(child.as_str());
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
                let container_rect = bounds.expand(sibling_container_padding * zoom);
                if viewport.intersects(container_rect) {
                    painter.rect_filled(container_rect, 10.0 * zoom, Color32::from_black_alpha(25));
                    painter.rect_stroke(
                        container_rect,
                        10.0 * zoom,
                        Stroke::new(1.0, Color32::from_rgb(70, 105, 110)),
                        egui::StrokeKind::Outside,
                    );
                }
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
            if log_layout && blocks.len() == 1 {
                let child = block_children.first().copied().unwrap_or("?");
                let sideways = if orientation == TreeOrientation::Vertical {
                    target.x - origin.x
                } else {
                    target.y - origin.y
                };
                if sideways.abs() > 0.5 {
                    let child_position = pos(child).unwrap_or(Pos2::ZERO);
                    let parent_a_position = family
                        .parent_a
                        .as_deref()
                        .and_then(&pos)
                        .unwrap_or(Pos2::ZERO);
                    let parent_b_position = family
                        .parent_b
                        .as_deref()
                        .and_then(&pos)
                        .unwrap_or(Pos2::ZERO);
                    log::debug!(
                        "LAYOUT_SINGLE_CHILD_DIAGONAL family={} child={} name=\"{}\" sideways_px={:.1} origin=({:.1},{:.1}) target=({:.1},{:.1}) child_pos=({:.1},{:.1}) parent_a={} pos=({:.1},{:.1}) parent_b={} pos=({:.1},{:.1}) raw={:.1} effective={:.1} block=({:.1},{:.1})-({:.1},{:.1})",
                        family.id,
                        child,
                        relations.find(child).map(Person::display_name).unwrap_or_default(),
                        sideways,
                        origin.x,
                        origin.y,
                        target.x,
                        target.y,
                        child_position.x,
                        child_position.y,
                        family.parent_a.as_deref().unwrap_or("-"),
                        parent_a_position.x,
                        parent_a_position.y,
                        family.parent_b.as_deref().unwrap_or("-"),
                        parent_b_position.x,
                        parent_b_position.y,
                        manual_offsets.get(child).copied().unwrap_or_default(),
                        eff_owned.get(child).copied().unwrap_or_default(),
                        blocks[0].left(),
                        blocks[0].top(),
                        blocks[0].right(),
                        blocks[0].bottom(),
                    );
                }
            }
            if line_on_screen(origin, target) {
                painter.line_segment(
                    [origin, target],
                    Stroke::new(2.0, Color32::from_rgb(74, 111, 119)),
                );
            }
            continue;
        }
        // Vorfahren-/Fächer-Sicht: Paarlinie zwischen sichtbaren Eltern,
        // Linien vom Junction zu jedem Kind.
        if let (Some(a), Some(b)) = (pa, pb) {
            if line_on_screen(a, b) {
                painter.line_segment(
                    [a, b],
                    Stroke::new(2.0, Color32::from_rgb(120, 170, 160)),
                );
            }
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
                if line_on_screen(junction, c) {
                    painter.line_segment(
                        [junction, c],
                        Stroke::new(2.0, Color32::from_rgb(74, 111, 119)),
                    );
                }
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
        let card_on_screen = viewport.intersects(card);
        let card_clicked = draw_person_card(
            painter,
            person,
            at,
            width,
            card_layout,
            zoom,
            viewed == Some(person.id.as_str()),
            false,
            media_base,
            photo_cache,
        );
        // Ausklapp-Abzeichen: Grenzknoten mit nicht sichtbaren Verwandten.
        let has_more = {
            let relatives = if ancestors {
                relations.parents_of(&person.id)
            } else {
                relations.children_of(&person.id)
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
        if card_on_screen
            && view != TreeView::Fan
            && card_hovered
            && (has_more || expanded.contains(&person.id))
        {
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
                    badge_at - Vec2::new(0.0, 4.5 * zoom),
                    Vec2::new(0.0, 9.0 * zoom),
                    Stroke::new(1.5, Color32::from_rgb(158, 213, 199)),
                );
            } else {
                painter.arrow(
                    badge_at + Vec2::new(0.0, 4.5 * zoom),
                    Vec2::new(0.0, -9.0 * zoom),
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
        if card_on_screen && view == TreeView::Descendants && person.id == root {
            let parents = relations.parents_of(&person.id);
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
            let long_pressed = card_on_screen
                && painter.ctx().input(|i| {
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
            let is_active = card_drag
                .as_ref()
                .is_some_and(|(id, _)| id == person.id.as_str());
            if !card_on_screen && !is_active {
                return (0.0, false);
            }
            if !(i.pointer.primary_down() && i.modifiers.shift) {
                return (0.0, false);
            }
            let pressed_here = i.pointer.press_origin().is_some_and(|q| card.contains(q));
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
            for partner in relations.partners_of(&person.id) {
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
                if log_layout && view == TreeView::Ancestors {
                    let offsets: Vec<_> = move_set.iter()
                        .map(|id| format!("{}:{:.1}", id, manual_offsets.get(id.as_str()).copied().unwrap_or(0.0)))
                        .collect();
                    log::debug!("DRAG_PICK person={} move_set=[{}] zoom={:.3} layout_delta={:.1}",
                        person.id, offsets.join(", "), zoom, layout_delta);
                }
                *card_drag = Some((person.id.clone(), move_set));
            }
        }
        if view == TreeView::Fan {
            continue;
        }
        // Partner-Pseudokarten (Partner nicht selbst im Graphen).
        for partner in relations.partners_of(&person.id) {
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
                if line_on_screen(line_a, line_b) {
                    painter.line_segment(
                        [line_a, line_b],
                        Stroke::new(2.0, Color32::from_rgb(120, 170, 160)),
                    );
                }
            }
            let partner_card = Rect::from_center_size(pat, Vec2::new(partner_width, card_h) * zoom);
            let partner_on_screen = viewport.intersects(partner_card);
            let partner_clicked = draw_person_card(
                painter,
                partner,
                pat,
                partner_width,
                card_layout,
                zoom,
                viewed == Some(partner.id.as_str()),
                true,
                media_base,
                photo_cache,
            );
            let partner_active = card_drag
                .as_ref()
                .is_some_and(|(id, _)| id == partner.id.as_str());
            if !partner_on_screen && !partner_active {
                continue;
            }
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
            if partner_on_screen && view == TreeView::Descendants && person.id == root {
                let parents = relations.parents_of(&partner.id);
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
            // Partner-Pseudokarte per Shift+Ziehen: NUR die Reihenfolge der
            // Partner tauschen (mehrere freie Partner ohne Kennenlern-/
            // Heiratsdatum). Datierte Partner bleiben chronologisch fix; die
            // Karte verschiebt die Person dabei NICHT mehr.
            let free = {
                let (z, m) = data.get_partnership_dates(person, partner);
                z.is_none() && m.is_none()
            };
            let swap_axis: f32 = painter.ctx().input(|i| {
                if !i.pointer.primary_down() {
                    // Losgelassen: Gestus-Latches räumen.
                    *swap_latch = false;
                    if card_drag
                        .as_ref()
                        .is_some_and(|(id, _)| id == partner.id.as_str())
                    {
                        *card_drag = None;
                    }
                    return 0.0;
                }
                // Nach einem Tausch sperren, bis die Maustaste losgelassen
                // wird – sonst tauscht dasselbe Ziehen jede Frame hin und her.
                if *swap_latch {
                    return 0.0;
                }
                if !(i.modifiers.shift && free) {
                    return 0.0;
                }
                let pressed_here = i
                    .pointer
                    .press_origin()
                    .is_some_and(|q| partner_card.contains(q));
                if pressed_here {
                    *card_drag = Some((partner.id.clone(), Vec::new()));
                }
                if !card_drag
                    .as_ref()
                    .is_some_and(|(id, _)| id == partner.id.as_str())
                {
                    return 0.0;
                }
                let from = i.pointer.press_origin().unwrap_or(partner_card.center());
                let to = i.pointer.latest_pos().unwrap_or(partner_card.center());
                let axis = if orientation == TreeOrientation::Vertical {
                    to.x - from.x
                } else {
                    to.y - from.y
                };
                if axis.abs() >= 20.0 {
                    // Richtung für den Tausch festhalten, Latch lösen (sperrt
                    // weitere Tausche bis zum Loslassen → kein Flicker).
                    *card_drag = None;
                    *swap_latch = true;
                    if axis < 0.0 { -20.0 } else { 20.0 }
                } else {
                    0.0
                }
            });
            if swap_axis.abs() == 20.0 {
                *action = Some(TreeAction::SwapPartner {
                    person_id: person.id.clone(),
                    partner_id: partner.id.clone(),
                    direction: if swap_axis < 0.0 { -1 } else { 1 },
                });
            }
        }
    }
    if log_layout
        && (drag_started || drag_ended)
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
                    log::debug!("  FAM {}: {}({:.0}) <-> {}({:.0}) dist={:.1} offsets=({:.1},{:.1}) eff=({:.1},{:.1})",
                        f.id, f.parent_a.as_deref().unwrap_or("?"), a.x,
                        f.parent_b.as_deref().unwrap_or("?"), b.x, d, oa, ob, ea, eb);
                }
            }
        }
        let avg = if count > 0 { total / count as f32 } else { 0.0 };
        if drag_started {
            log::debug!("DRAG_START avg_parent_dist={:.1} ({} families, view={:?}, zoom={:.3}, root={} gen_limit={} visible={} peak_eff={:.1})",
                avg, count, view, zoom, root, generation_limit, rows.iter().map(|r| r.len()).sum::<usize>(), peak_eff);
        } else {
            log::debug!("DRAG_END   avg_parent_dist={:.1} ({} families, view={:?}, zoom={:.3}, root={} gen_limit={} visible={} peak_eff={:.1})",
                avg, count, view, zoom, root, generation_limit, rows.iter().map(|r| r.len()).sum::<usize>(), peak_eff);
        }
    }
    content_bounds
}

#[allow(clippy::too_many_arguments)]
fn log_layout_diagnostics(
    root: &str,
    data: &TreeData,
    relations: &TreeRelations<'_>,
    view: TreeView,
    orientation: TreeOrientation,
    zoom: f32,
    rows: &[Vec<&str>],
    levels: &HashMap<&str, usize>,
    positions: &HashMap<&str, (f32, f32)>,
    widths: &HashMap<&str, f32>,
    shown_partners: &HashSet<&str>,
    manual_offsets: &HashMap<String, f32>,
    effective_offsets: &HashMap<String, f32>,
    group_of: &HashMap<&str, &str>,
    card_h: f32,
    nominal_gap: f32,
    content_bounds: Rect,
) {
    let root_name = relations
        .find(root)
        .map(Person::display_name)
        .unwrap_or_default();
    let nonzero_manual = manual_offsets
        .values()
        .filter(|value| value.abs() > 0.01)
        .count();
    let zero_manual = manual_offsets.len().saturating_sub(nonzero_manual);
    let max_manual = manual_offsets
        .values()
        .map(|value| value.abs())
        .fold(0.0f32, f32::max);
    let max_effective = effective_offsets
        .values()
        .map(|value| value.abs())
        .fold(0.0f32, f32::max);
    log::debug!(
        "LAYOUT_DIAG root={} name=\"{}\" view={:?} orientation={:?} zoom={:.3} rows={:?} bounds=({:.1}x{:.1}) manual={} nonzero={} zero={} max_manual={:.1} max_effective={:.1}",
        root,
        root_name,
        view,
        orientation,
        zoom,
        rows.iter().map(Vec::len).collect::<Vec<_>>(),
        content_bounds.width(),
        content_bounds.height(),
        manual_offsets.len(),
        nonzero_manual,
        zero_manual,
        max_manual,
        max_effective,
    );

    let mut offset_offenders: Vec<(&str, f32, f32)> = levels
        .keys()
        .filter_map(|id| {
            let raw = manual_offsets.get(*id).copied().unwrap_or_default();
            let effective = effective_offsets.get(*id).copied().unwrap_or_default();
            (raw.abs() > 0.01 || effective.abs() > 0.01).then_some((*id, raw, effective))
        })
        .collect();
    offset_offenders.sort_by(|a, b| {
        b.1.abs()
            .max(b.2.abs())
            .total_cmp(&a.1.abs().max(a.2.abs()))
    });
    for (id, raw, effective) in offset_offenders.into_iter().take(10) {
        let name = relations.find(id).map(Person::display_name).unwrap_or_default();
        log::debug!(
            "LAYOUT_OFFSET id={} name=\"{}\" row={} raw={:.1} effective={:.1}",
            id,
            name,
            levels.get(id).copied().unwrap_or_default(),
            raw,
            effective,
        );
    }

    // Tatsächliche belegte Gruppenspannen einschließlich Partner-Pseudokarten.
    // So wird sichtbar, welche zwei Zweige die große leere Fläche begrenzen.
    let axis = |position: &(f32, f32)| {
        if orientation == TreeOrientation::Vertical {
            position.0
        } else {
            position.1
        }
    };
    let extent = |id: &str| {
        if orientation == TreeOrientation::Vertical {
            widths.get(id).copied().unwrap_or(215.0)
        } else {
            card_h
        }
    };
    let mut gaps: Vec<(f32, usize, &str, &str, f32, f32)> = Vec::new();
    for (row, ids) in rows.iter().enumerate() {
        let mut spans: Vec<(&str, f32, f32)> = Vec::new();
        for id in ids {
            let Some(position) = positions.get(*id) else {
                continue;
            };
            let center = axis(position);
            let mut start = center - extent(id) / 2.0;
            let mut end = center + extent(id) / 2.0;
            for partner in relations.partners_of(id) {
                if levels.contains_key(partner.id.as_str())
                    || !shown_partners.contains(partner.id.as_str())
                {
                    continue;
                }
                if let Some(partner_position) = positions.get(partner.id.as_str()) {
                    let partner_center = axis(partner_position);
                    start = start.min(partner_center - extent(&partner.id) / 2.0);
                    end = end.max(partner_center + extent(&partner.id) / 2.0);
                }
            }
            spans.push((*id, start, end));
        }
        spans.sort_by(|a, b| a.1.total_cmp(&b.1));
        for pair in spans.windows(2) {
            let actual_gap = pair[1].1 - pair[0].2;
            gaps.push((actual_gap, row, pair[0].0, pair[1].0, pair[0].2, pair[1].1));
        }
    }
    gaps.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (actual_gap, row, left, right, left_end, right_start) in gaps
        .into_iter()
        .filter(|gap| gap.0 > nominal_gap * 2.0)
        .take(12)
    {
        let left_name = relations.find(left).map(Person::display_name).unwrap_or_default();
        let right_name = relations.find(right).map(Person::display_name).unwrap_or_default();
        log::debug!(
            "LAYOUT_GAP row={} gap={:.1} nominal={:.1} left={} name=\"{}\" edge={:.1} group={} raw={:.1} effective={:.1} right={} name=\"{}\" edge={:.1} group={} raw={:.1} effective={:.1}",
            row,
            actual_gap,
            nominal_gap,
            left,
            left_name,
            left_end,
            group_of.get(left).copied().unwrap_or("-"),
            manual_offsets.get(left).copied().unwrap_or_default(),
            effective_offsets.get(left).copied().unwrap_or_default(),
            right,
            right_name,
            right_start,
            group_of.get(right).copied().unwrap_or("-"),
            manual_offsets.get(right).copied().unwrap_or_default(),
            effective_offsets.get(right).copied().unwrap_or_default(),
        );
    }

    let mut origins: HashMap<&str, Vec<&str>> = HashMap::new();
    for family in &data.families {
        for child in &family.children {
            if levels.contains_key(child.as_str()) {
                origins
                    .entry(child.as_str())
                    .or_default()
                    .push(family.id.as_str());
            }
        }
    }
    for (child, families) in origins.into_iter().filter(|(_, families)| families.len() > 1) {
        log::debug!(
            "LAYOUT_MULTI_ORIGIN child={} name=\"{}\" families={:?} primary={}",
            child,
            relations.find(child).map(Person::display_name).unwrap_or_default(),
            families,
            group_of.get(child).copied().unwrap_or("-"),
        );
    }

    for id in levels.keys() {
        let mut families_by_partner: HashMap<&str, Vec<&str>> = HashMap::new();
        for family in &data.families {
            let other = if family.parent_a.as_deref() == Some(*id) {
                family.parent_b.as_deref()
            } else if family.parent_b.as_deref() == Some(*id) {
                family.parent_a.as_deref()
            } else {
                None
            };
            if let Some(other) = other {
                families_by_partner
                    .entry(other)
                    .or_default()
                    .push(family.id.as_str());
            }
        }
        for (partner, families) in families_by_partner
            .into_iter()
            .filter(|(_, families)| families.len() > 1)
        {
            log::debug!(
                "LAYOUT_DUP_PARTNER owner={} partner={} families={:?}",
                id, partner, families
            );
        }
    }

    // Zeilen-Reihenfolge (nach Achse sortiert): zeigt, wie die Karten einer
    // Generation verteilt sind – Basis für die Analyse „ab 4 Generationen".
    for (row, ids) in rows.iter().enumerate() {
        let mut items: Vec<(&str, f32)> = ids
            .iter()
            .filter_map(|id| positions.get(*id).map(|p| (*id, axis(p))))
            .collect();
        items.sort_by(|a, b| a.1.total_cmp(&b.1));
        let order = items
            .iter()
            .map(|(id, x)| format!("{}@{:.0}", id, x))
            .collect::<Vec<_>>()
            .join(" ");
        log::debug!("LAYOUT_ROW row={} n={} order=[{}]", row, items.len(), order);
    }

    // Familien: Abstand zwischen Eltern-Junction und dem Mittel der
    // Kindergruppe. Große |delta| zeigen, wo die finale Zentrierung die
    // Gruppe NICHT unter die Eltern ziehen konnte (auseinandergezogenes Layout).
    for family in &data.families {
        let children: Vec<(&str, f32)> = family
            .children
            .iter()
            .filter_map(|child| {
                positions
                    .get(child.as_str())
                    .map(|p| (child.as_str(), axis(p)))
            })
            .collect();
        if children.is_empty() {
            continue;
        }
        let parents: Vec<(&str, f32)> = [&family.parent_a, &family.parent_b]
            .into_iter()
            .flatten()
            .filter_map(|parent| {
                positions
                    .get(parent.as_str())
                    .map(|p| (parent.as_str(), axis(p)))
            })
            .collect();
        if parents.is_empty() {
            continue;
        }
        let junction = parents.iter().map(|(_, x)| *x).sum::<f32>() / parents.len() as f32;
        let child_mean = children.iter().map(|(_, x)| *x).sum::<f32>() / children.len() as f32;
        let delta = child_mean - junction;
        if delta.abs() > card_h {
            let row = children
                .first()
                .and_then(|(id, _)| levels.get(*id).copied())
                .unwrap_or_default();
            let parents_list = parents
                .iter()
                .map(|(id, x)| format!("{}@{:.0}", id, x))
                .collect::<Vec<_>>()
                .join(" ");
            let children_list = children
                .iter()
                .map(|(id, x)| format!("{}@{:.0}", id, x))
                .collect::<Vec<_>>()
                .join(" ");
            log::debug!(
                "LAYOUT_FAMILY id={} row={} delta={:.0} junction={:.0} childmean={:.0} parents=[{}] children=[{}]",
                family.id, row, delta, junction, child_mean, parents_list, children_list,
            );
        }
    }
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
    relations: &TreeRelations<'a>,
    levels: &HashMap<&'a str, usize>,
    widths: &HashMap<&'a str, f32>,
    shown_partners: &HashSet<&'a str>,
    drag_root: &'a str,
    manual_offsets: &mut HashMap<String, f32>,
    _gap: f32,
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
            for partner in relations.partners_of(id) {
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
            // Linker Nachbar fest? Dann gilt: x - w/2 >= nbx + nbw/2 + MIN_CARD_GAP
            // (fester Mindestabstand; der Baum-Abstand bleibt ein Default).
            if i > 0 {
                let (nb, nbx, nbw) = cards[i - 1];
                if !scale.contains_key(nb) {
                    let cand =
                        (nbx + nbw / 2.0 + MIN_CARD_GAP + w / 2.0 - x) / s + current;
                    lo = lo.max(cand);
                }
            }
            // Rechter Nachbar fest? Dann gilt: x + w/2 <= nbx - nbw/2 - MIN_CARD_GAP.
            if i + 1 < cards.len() {
                let (nb, nbx, nbw) = cards[i + 1];
                if !scale.contains_key(nb) {
                    let cand =
                        (nbx - nbw / 2.0 - MIN_CARD_GAP - w / 2.0 - x) / s + current;
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
        for partner in relations.partners_of(id) {
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
    relations: &TreeRelations<'a>,
    levels: &HashMap<&'a str, usize>,
    view: TreeView,
    orientation: TreeOrientation,
    card_h: f32,
    widths: &HashMap<&'a str, f32>,
    gap: f32,
    sibling_container_padding: f32,
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
    // Statische Kartengrößen einmalig vorbereiten (ändern sich während der
    // Verhandlung nicht): linker/rechter Karten-Offset inkl. Partner-Pseudo-
    // karten sowie der Geburtsdatum-Sortierschlüssel je sichtbarer Person.
    let birth_key = |id: &'a str| -> (i32, i32, i32, i32, &'a str) {
        if let Some(p) = relations.find(id) {
            if let Some((y, m, d)) = crate::model::parse_birth_date(&p.birth) {
                (0, y, m, d, p.id.as_str())
            } else {
                (1, 0, 0, 0, p.id.as_str())
            }
        } else {
            (1, 0, 0, 0, id)
        }
    };
    let mut left_offsets: HashMap<&str, f32> = HashMap::new();
    let mut right_offsets: HashMap<&str, f32> = HashMap::new();
    let mut birth_keys: HashMap<&str, (i32, i32, i32, i32, &str)> = HashMap::new();
    for &id in levels.keys() {
        birth_keys.insert(id, birth_key(id));
        let own = own_extent(0, id);
        let mut right = own / 2.0;
        for partner in relations.partners_of(id) {
            if !visible(&partner.id) && shown_partners.contains(partner.id.as_str()) {
                right += partner_extent(0, &partner.id) + couple_gap;
            }
        }
        left_offsets.insert(id, own / 2.0);
        right_offsets.insert(id, right);
    }
    // Breite des TEILBAUMS je Person: eigener Footprint + alle sichtbaren
    // Nachkommen. Die "breiteste Stelle" bleibt bei Kollisionen stehen,
    // schmalere Gruppen weichen aus.
    let mut branch_width: HashMap<&str, f32> = HashMap::new();
    {
        let mut all: Vec<&str> = levels.keys().copied().collect();
        all.sort_by(|a, b| levels[b].cmp(&levels[a])); // tiefste zuerst
        for id in all {
            let mut width = own_extent(0, id);
            for partner in relations.partners_of(id) {
                if !visible(&partner.id) && shown_partners.contains(partner.id.as_str()) {
                    width += partner_extent(0, &partner.id) + couple_gap;
                }
            }
            for child in relations.children_of(id) {
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
                for partner in relations.partners_of(id) {
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
            if let Some(p) = relations.find(id) {
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
                    let key_a = birth_keys[a.0];
                    let key_b = birth_keys[b.0];
                    key_a.cmp(&key_b)
                });
                for window in inner.windows(2) {
                    // In der AUTOMATIK gilt der konfigurierte Baum-Abstand als
                    // Kartenabstand (`gap`). Beim finalen Pass nach manuellen
                    // Versätzen wird stattdessen nur noch `MIN_CARD_GAP`
                    // übergeben, damit selbst verschobene Karten eng stehen
                    // dürfen.
                    let need =
                        right_offsets[window[0].0] + left_offsets[window[1].0] + gap;
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
                        .map(|(id, _)| spread[*id] - left_offsets[id])
                        .fold(f32::MAX, f32::min);
                    let end = group
                        .iter()
                        .map(|(id, _)| spread[*id] + right_offsets[id])
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
                // Container als Blöcke nur mit festem Mindestabstand + Rand
                // trennen. Würde hier der (große) Baum-Abstand erzwungen, zöge
                // der Pass die Zeilen auseinander und Kindergruppen ließen sich
                // nicht mehr unter ihre Eltern zurückholen.
                let need = MIN_CARD_GAP
                    + if window[0].0.len() > 1 {
                        sibling_container_padding
                    } else {
                        0.0
                    }
                    + if window[1].0.len() > 1 {
                        sibling_container_padding
                    } else {
                        0.0
                    };
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
fn card_width_for(
    person: &crate::model::Person,
    painter: &egui::Painter,
    card_layout: CardLayout,
) -> f32 {
    let name = painter
        .layout_no_wrap(
            person.display_name_short(),
            FontId::proportional(13.0),
            Color32::WHITE,
        )
        .size()
        .x;
    let given = painter
        .layout_no_wrap(
            person.given_short(),
            FontId::proportional(13.0),
            Color32::WHITE,
        )
        .size()
        .x;
    let family = if person.family_name.is_empty() {
        0.0
    } else {
        painter
            .layout_no_wrap(
                person.family_name.clone(),
                FontId::proportional(11.0),
                Color32::WHITE,
            )
            .size()
            .x
    };
    let birth_text = person.birth_short();
    let birth = painter
        .layout_no_wrap(birth_text, FontId::proportional(12.0), Color32::WHITE)
        .size()
        .x;
    match card_layout {
        CardLayout::Compact => (64.0 + name.max(birth) + 20.0).max(215.0),
        CardLayout::Portrait => (given.max(family).max(birth) + 24.0).max(160.0),
    }
}

/// Eine Personenkarte zeichnen; Rückgabe true bei Klick auf die Karte.
/// `muted` = Partner-Pseudokarte (dezentere Umrandung).
fn draw_person_card(
    painter: &egui::Painter,
    person: &crate::model::Person,
    at: Pos2,
    width: f32,
    card_layout: CardLayout,
    zoom: f32,
    selected_now: bool,
    muted: bool,
    media_base: &std::path::Path,
    photo_cache: &mut HashMap<String, TextureHandle>,
) -> bool {
    let size = Vec2::new(width, card_layout.height()) * zoom;
    let card = Rect::from_center_size(at, size);
    if !painter.clip_rect().expand(32.0).intersects(card) {
        return false;
    }
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
    if zoom < CARD_DETAIL_MIN_ZOOM {
        // Statt leerer Farbfläche füllt das Profilbild die Karte, ohne sie zu
        // verzerren: Cover-Beschnitt mit dem KARTEN-Seitenverhältnis, sodass
        // die Box vollständig und ungestreckt gefüllt ist. Ohne Foto stehen
        // dezente Initialen in der Kartenmitte.
        if let Some(texture) =
            photo_preview_texture(painter.ctx(), person, photo_cache, media_base)
        {
            let tv = texture.size_vec2();
            let card_aspect = card.width() / card.height().max(1.0);
            let uv = cover_uv_to(
                tv.x / tv.y.max(1.0),
                card_aspect,
                person.photo_crop.as_ref(),
            );
            painter
                .with_clip_rect(card)
                .image(texture.id(), card, uv, Color32::WHITE);
            painter.rect(
                card,
                10. * zoom,
                Color32::TRANSPARENT,
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
        } else {
            painter.text(
                card.center(),
                Align2::CENTER_CENTER,
                initials(person),
                FontId::proportional((card.height() * 0.32).max(5.0)),
                Color32::from_rgba_unmultiplied(225, 232, 232, 130),
            );
        }
        return painter.ctx().input(|i| {
            i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|q| card.contains(q))
        });
    }
    let (avatar_size, avatar_center, name_at, name_align, name2_at, birth_at, birth_align) =
        match card_layout {
            CardLayout::Compact => (
                48.0 * zoom,
                card.left_center() + Vec2::new(35.0 * zoom, 0.0),
                card.left_top() + Vec2::new(64.0 * zoom, 21.0 * zoom),
                Align2::LEFT_CENTER,
                Pos2::ZERO,
                card.left_bottom() + Vec2::new(64.0 * zoom, -17.0 * zoom),
                Align2::LEFT_CENTER,
            ),
            CardLayout::Portrait => (
                78.0 * zoom,
                card.center_top() + Vec2::new(0.0, 48.0 * zoom),
                card.center_top() + Vec2::new(0.0, 106.0 * zoom),
                Align2::CENTER_CENTER,
                card.center_top() + Vec2::new(0.0, 126.0 * zoom),
                card.center_top() + Vec2::new(0.0, 145.0 * zoom),
                Align2::CENTER_CENTER,
            ),
        };
    let avatar = Rect::from_center_size(avatar_center, Vec2::splat(avatar_size));
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
            // Proportional zur Avatar-Größe: Kompakt (48) behält 13, das
            // große Foto (78) skaliert die Initialen entsprechend mit.
            FontId::proportional(avatar_size * 13.0 / 48.0),
            Color32::WHITE,
        );
    }
    match card_layout {
        CardLayout::Compact => {
            painter.text(
                name_at,
                name_align,
                person.display_name_short(),
                FontId::proportional(13. * zoom),
                Color32::WHITE,
            );
        }
        CardLayout::Portrait => {
            painter.text(
                name_at,
                name_align,
                person.given_short(),
                FontId::proportional(13. * zoom),
                Color32::WHITE,
            );
            if !person.family_name.is_empty() {
                painter.text(
                    name2_at,
                    Align2::CENTER_CENTER,
                    person.family_name.clone(),
                    FontId::proportional(13. * zoom),
                    Color32::WHITE,
                );
            }
        }
    }
    painter.text(
        birth_at,
        birth_align,
        person.birth_short(),
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
    relations: &TreeRelations<'a>,
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
    let parents = relations.parents_of(id);
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
            let f_layout = layout_ancestors(f, relations, levels, widths, gap);
            let m_layout = layout_ancestors(m, relations, levels, widths, gap);

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
                    // Benötigter Abstand: rechter Rand Vater - linker Rand
                    // Mutter + Baum-Abstand. Im Vorfahren-Automatiklayout
                    // liegt damit der konfigurierte `gap` zwischen den
                    // Kartengruppen (Default-Abstand); nur der finale Pass
                    // nach manuellen Versätzen kappt auf `MIN_CARD_GAP`.
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
            let p_layout = layout_ancestors(p_id, relations, levels, widths, gap);
            for (id, offset) in p_layout {
                layout.insert(id, offset);
            }
        }
        (None, None) => {}
    }

    layout
}
