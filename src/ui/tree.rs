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

use std::collections::{HashMap, HashSet, VecDeque};

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, TextureHandle, Vec2};

use crate::media::{cover_uv_to, initials, photo_card_texture, round_avatar_texture_cached};
use crate::model::{Person, TreeData};
use crate::ui::CardLayout;

/// Fester Mindestabstand zwischen Karten. Der einstellbare Baum-Abstand ist
/// der Default (Zielabstand) der automatischen Platzierung; darunter dürfen
/// Karten nie rücken, damit sie sich nicht berühren.
const MIN_CARD_GAP: f32 = 16.0;

/// Standardwert für den Extra-Abstand zwischen Nachbarkarten ohne
/// Partner-Verbindung (keine gemeinsame Familie als Eltern — Paare mit
/// gemeinsamen Kindern bleiben kompakt). Einstellbar: `non_partner_gap`,
/// 0 = kein Extra.
pub(crate) const UNMARRIED_GAP_EXTRA: f32 = 30.0;

/// Für einen Layoutdurchlauf vorbereitete Beziehungen. Das vermeidet, dass
/// `children_of`/`parents_of`/`partners_of` in jedem Kollisionspass erneut
/// alle Familien und Personen linear durchsuchen. Auch für Detailansichten
/// (z. B. Schnellerfassung) wiederverwendet.
pub(crate) struct TreeRelations<'a> {
    people: HashMap<&'a str, &'a Person>,
    children: HashMap<&'a str, Vec<&'a Person>>,
    parents: HashMap<&'a str, Vec<&'a Person>>,
    partners: HashMap<&'a str, Vec<&'a Person>>,
    couple_children: HashMap<&'a str, HashMap<&'a str, Vec<&'a str>>>,
}

impl<'a> TreeRelations<'a> {
    pub(crate) fn new(data: &'a TreeData) -> Self {
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
        // Sortierschlüssel einmalig je Person/Paar vorberechnen (statt
        // Datumsparsen pro Vergleich im Sortiertaufruf).
        let birth_keys: HashMap<&str, (i32, i32, i32, i32, &str)> = people
            .iter()
            .map(|(id, person)| (*id, birth_key(person)))
            .collect();
        for list in children.values_mut() {
            list.sort_by(|a, b| birth_keys[a.id.as_str()].cmp(&birth_keys[b.id.as_str()]));
        }
        for (id, list) in &mut partners {
            let Some(person) = people.get(id).copied() else {
                continue;
            };
            let manual = data.partner_order.get(*id);
            let keys: HashMap<&str, (u8, (i32, i32, i32), (i32, i32, i32), usize, &str)> = list
                .iter()
                .map(|partner| {
                    (
                        partner.id.as_str(),
                        partner_key(data, person, partner, manual),
                    )
                })
                .collect();
            list.sort_by(|a, b| keys[a.id.as_str()].cmp(&keys[b.id.as_str()]));
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

    pub(crate) fn children_of(&self, id: &str) -> &[&'a Person] {
        self.children.get(id).map(Vec::as_slice).unwrap_or_default()
    }

    pub(crate) fn parents_of(&self, id: &str) -> &[&'a Person] {
        self.parents.get(id).map(Vec::as_slice).unwrap_or_default()
    }

    pub(crate) fn partners_of(&self, id: &str) -> &[&'a Person] {
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

fn collect_visible_levels<'a>(
    root: &'a str,
    relations: &TreeRelations<'a>,
    ancestors: bool,
    generation_limit: usize,
    expanded: &HashSet<String>,
    person_limit: Option<usize>,
) -> (HashMap<&'a str, usize>, bool) {
    let budget = person_limit.map(|limit| limit.max(1));
    let mut levels = HashMap::from([(root, 0usize)]);
    let mut frontier = VecDeque::from([root]);
    let mut limited = false;
    while let Some(id) = frontier.pop_front() {
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
            let relative_id = relative.id.as_str();
            if levels.contains_key(relative_id) {
                continue;
            }
            if budget.is_some_and(|limit| levels.len() >= limit) {
                limited = true;
                continue;
            }
            levels.insert(relative_id, level + 1);
            frontier.push_back(relative_id);
        }
    }
    (levels, limited)
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
    /// Person zur Ansicht/Bearbeitung öffnen (normaler Klick, löst Mehrfachauswahl auf).
    View(String),
    /// Als Referenzperson setzen (Shift+Klick, Doppelklick, langer Touch).
    Reference(String),
    /// Mehrfachauswahl umschalten (Strg+Klick): zuletzt Hinzugefügte bleibt
    /// aktive Auswahl (grün gefüllt, links angezeigt), Rest grün umrandet.
    ToggleMulti(String),
    /// Weitere Generationen an diesem Knoten aus-/einklappen.
    ToggleExpand(String),
    /// Partner-Pseudokarten per Shift+Ziehen tauschen (nur freie Partner;
    /// `direction` = -1 nach vorn, +1 nach hinten).
    SwapPartner {
        person_id: String,
        partner_id: String,
        direction: i32,
    },
    /// Zu einer bestimmten Layout-Position scrollen.
    PanTo(Vec2),
}

/// Ein Segment einer Zeile in der Nachfahren-Anordnung: eine Geschwistergruppe
/// (oder eine einzelne Karte) mit starren relativen Abständen.
struct DescendantBlock<'a> {
    key: String,
    members: Vec<&'a str>,
    offsets: Vec<f32>,
    half_left: f32,
    half_right: f32,
    pad: f32,
    target: Option<f32>,
    base: f32,
}

fn fit_descendant_block_starts(blocks: &[DescendantBlock<'_>], gap: f32) -> Vec<f32> {
    let widths: Vec<f32> = blocks
        .iter()
        .map(|block| block.half_left + block.half_right)
        .collect();
    let desired_start = |index: usize| {
        blocks[index].target.unwrap_or(blocks[index].base) - widths[index] / 2.0
    };

    let mut starts = vec![0.0f32; blocks.len()];
    let mut previous_end: Option<f32> = None;
    let mut previous_pad = 0.0;
    for (index, block) in blocks.iter().enumerate() {
        let required_start = previous_end
            .map(|end| end + gap + previous_pad + block.pad)
            .unwrap_or(f32::NEG_INFINITY);
        starts[index] = desired_start(index).max(required_start);
        previous_end = Some(starts[index] + widths[index]);
        previous_pad = block.pad;
    }

    let forward_starts = starts.clone();
    for index in (0..blocks.len()).rev() {
        let block = &blocks[index];
        let lower = if index == 0 {
            f32::NEG_INFINITY
        } else {
            forward_starts[index - 1]
                + widths[index - 1]
                + gap
                + blocks[index - 1].pad
                + block.pad
        };
        let upper = if index + 1 == blocks.len() {
            f32::INFINITY
        } else {
            starts[index + 1]
                - widths[index]
                - gap
                - block.pad
                - blocks[index + 1].pad
        };
        if lower <= upper {
            starts[index] = desired_start(index)
                .min(forward_starts[index])
                .clamp(lower, upper);
        }
    }
    starts
}

#[cfg(test)]
mod descendant_layout_tests {
    use std::collections::{HashMap, HashSet};

    use crate::model::{Family, Gender, TreeData, person};

    use super::{
        DescendantBlock, TreeRelations, collect_visible_levels, fit_descendant_block_starts,
        tidy_descendant_spread_vertical,
    };

    fn tidy_positions<'a>(
        data: &'a TreeData,
        rows: &'a [Vec<&'a str>],
        levels: &'a HashMap<&'a str, usize>,
        widths: &'a HashMap<&'a str, f32>,
        group_of: &'a HashMap<&'a str, &'a str>,
    ) -> HashMap<&'a str, f32> {
        let relations = TreeRelations::new(data);
        let shown = HashSet::new();
        let mut spread: HashMap<&str, f32> =
            levels.keys().map(|id| (*id, 0.0)).collect();
        tidy_descendant_spread_vertical(
            rows[0][0], &mut spread, rows, &relations, data, levels, widths, group_of, &shown,
            48.0, 12.0, 4.0,
        );
        spread
    }

    fn min_gap(spread: &HashMap<&str, f32>, ids: &[&str], half: f32) -> f32 {
        let mut order: Vec<f32> = ids.iter().map(|id| spread[*id]).collect();
        order.sort_by(|a, b| a.total_cmp(b));
        order
            .windows(2)
            .map(|w| (w[1] - half) - (w[0] + half))
            .fold(f32::INFINITY, f32::min)
    }

    fn mean(spread: &HashMap<&str, f32>, ids: &[&str]) -> f32 {
        ids.iter().map(|id| spread[*id]).sum::<f32>() / ids.len() as f32
    }

    fn block(
        key: &str,
        half_left: f32,
        half_right: f32,
        target: f32,
        pad: f32,
    ) -> DescendantBlock<'static> {
        DescendantBlock {
            key: key.into(),
            members: vec!["person"],
            offsets: vec![0.0],
            half_left,
            half_right,
            pad,
            target: Some(target),
            base: 0.0,
        }
    }

    #[test]
    fn centers_asymmetric_descendant_block_on_target() {
        let blocks = [block("family", 20.0, 60.0, 100.0, 0.0)];
        let starts = fit_descendant_block_starts(&blocks, 16.0);

        assert_eq!(starts, vec![60.0]);
        assert_eq!(starts[0] + (20.0 + 60.0) / 2.0, 100.0);
    }

    #[test]
    fn keeps_gap_and_container_padding_between_blocks() {
        let blocks = [
            block("left", 50.0, 50.0, 0.0, 12.0),
            block("right", 50.0, 50.0, 0.0, 12.0),
        ];
        let starts = fit_descendant_block_starts(&blocks, 16.0);

        assert_eq!(starts[1] - (starts[0] + 100.0), 40.0);
    }

    #[test]
    fn limits_breadth_first_traversal_in_deterministic_batches() {
        let mut data = TreeData::default();
        data.people
            .push(person("root", "Root", "Person", "", Gender::Unknown));
        let mut children = Vec::new();
        for index in 0..100 {
            let id = format!("child-{index:03}");
            data.people
                .push(person(&id, &format!("Child {index}"), "Person", "", Gender::Unknown));
            children.push(id);
        }
        data.families.push(Family {
            id: "family".into(),
            parent_a: Some("root".into()),
            parent_b: None,
            children,
        });
        let relations = TreeRelations::new(&data);
        let expanded = HashSet::new();

        let (first, first_limited) =
            collect_visible_levels("root", &relations, false, 0, &expanded, Some(60));
        let (repeated, _) =
            collect_visible_levels("root", &relations, false, 0, &expanded, Some(60));
        let (second, second_limited) =
            collect_visible_levels("root", &relations, false, 0, &expanded, Some(120));

        assert_eq!(first, repeated);
        assert_eq!(first.len(), 60);
        assert!(first_limited);
        assert_eq!(second.len(), 101);
        assert!(!second_limited);
    }

    #[test]
    fn parents_spread_for_many_children_and_stay_vertical() {
        let mut data = TreeData::default();
        data.people.push(person("r", "R", "Root", "", Gender::Unknown));
        data.people.push(person("a", "A", "Root", "", Gender::Unknown));
        data.people.push(person("b", "B", "Root", "", Gender::Unknown));
        let mut kids = Vec::new();
        for index in 1..=6 {
            let id = format!("k{index}");
            data.people.push(person(
                &id,
                &format!("Kid {index}"),
                "Root",
                "",
                Gender::Unknown,
            ));
            kids.push(id);
        }
        data.people.push(person("c1", "C", "Root", "", Gender::Unknown));
        data.families.push(Family {
            id: "f0".into(),
            parent_a: Some("r".into()),
            parent_b: None,
            children: vec!["a".into(), "b".into()],
        });
        data.families.push(Family {
            id: "f1".into(),
            parent_a: Some("a".into()),
            parent_b: None,
            children: kids,
        });
        data.families.push(Family {
            id: "f2".into(),
            parent_a: Some("b".into()),
            parent_b: None,
            children: vec!["c1".into()],
        });
        let levels: HashMap<&str, usize> = HashMap::from([
            ("r", 0),
            ("a", 1),
            ("b", 1),
            ("k1", 2),
            ("k2", 2),
            ("k3", 2),
            ("k4", 2),
            ("k5", 2),
            ("k6", 2),
            ("c1", 2),
        ]);
        let rows: Vec<Vec<&str>> = vec![
            vec!["r"],
            vec!["a", "b"],
            vec!["k1", "k2", "k3", "k4", "k5", "k6", "c1"],
        ];
        let widths: HashMap<&str, f32> = levels.keys().map(|id| (*id, 100.0)).collect();
        let group_of: HashMap<&str, &str> = HashMap::from([
            ("a", "f0"),
            ("b", "f0"),
            ("k1", "f1"),
            ("k2", "f1"),
            ("k3", "f1"),
            ("k4", "f1"),
            ("k5", "f1"),
            ("k6", "f1"),
            ("c1", "f2"),
        ]);
        let spread = tidy_positions(&data, &rows, &levels, &widths, &group_of);
        let kids_a = ["k1", "k2", "k3", "k4", "k5", "k6"];
        // Sechs Kinder brauchen 6*100 + 5*48 + 2*12 = 864 Breite: Die Eltern
        // müssen entsprechend auseinanderrücken (ohne Reservierung ~148).
        assert!((spread["b"] - spread["a"]) >= 529.0);
        // Kinderblock exakt senkrecht unter der Eltern-Junction.
        assert!((mean(&spread, &kids_a) - spread["a"]).abs() <= 1.0);
        assert!((spread["c1"] - spread["b"]).abs() <= 1.0);
        // Keine Überlappungen in allen Reihen.
        assert!(min_gap(&spread, &["a", "b"], 50.0) >= 47.0);
        assert!(
            min_gap(
                &spread,
                &["k1", "k2", "k3", "k4", "k5", "k6", "c1"],
                50.0
            ) >= 47.0
        );
        // Deterministisch bei Wiederholung.
        let repeat = tidy_positions(&data, &rows, &levels, &widths, &group_of);
        assert_eq!(spread, repeat);
    }

    #[test]
    fn three_partners_reserve_summed_space_without_overlap() {
        let mut data = TreeData::default();
        data.people.push(person("r", "R", "Root", "", Gender::Unknown));
        data.people.push(person("p", "P", "Root", "", Gender::Unknown));
        data.people.push(person("s", "S", "Root", "", Gender::Unknown));
        for (family, index) in ["m", "n", "o"].iter().zip(1..=3) {
            for member in 1..=2 {
                let id = format!("{family}{member}");
                data.people.push(person(
                    &id,
                    &format!("Kid {family}{member}"),
                    "Root",
                    "",
                    Gender::Unknown,
                ));
            }
            data.families.push(Family {
                id: format!("f{index}"),
                parent_a: Some("p".into()),
                parent_b: None,
                children: vec![format!("{family}1"), format!("{family}2")],
            });
        }
        data.families.push(Family {
            id: "f0".into(),
            parent_a: Some("r".into()),
            parent_b: None,
            children: vec!["p".into(), "s".into()],
        });
        let levels: HashMap<&str, usize> = HashMap::from([
            ("r", 0),
            ("p", 1),
            ("s", 1),
            ("m1", 2),
            ("m2", 2),
            ("n1", 2),
            ("n2", 2),
            ("o1", 2),
            ("o2", 2),
        ]);
        let rows: Vec<Vec<&str>> = vec![
            vec!["r"],
            vec!["p", "s"],
            vec!["m1", "m2", "n1", "n2", "o1", "o2"],
        ];
        let widths: HashMap<&str, f32> = levels.keys().map(|id| (*id, 100.0)).collect();
        let group_of: HashMap<&str, &str> = HashMap::from([
            ("p", "f0"),
            ("s", "f0"),
            ("m1", "f1"),
            ("m2", "f1"),
            ("n1", "f2"),
            ("n2", "f2"),
            ("o1", "f3"),
            ("o2", "f3"),
        ]);
        let spread = tidy_positions(&data, &rows, &levels, &widths, &group_of);
        // 3 * (2*100 + 48 + 2*12) = 816 reservierte Breite um P.
        assert!((spread["s"] - spread["p"]) >= 505.0);
        // Erste Familie steht senkrecht unter P, der Rest kollisionsfrei.
        assert!((mean(&spread, &["m1", "m2"]) - spread["p"]).abs() <= 1.0);
        assert!(
            min_gap(
                &spread,
                &["m1", "m2", "n1", "n2", "o1", "o2"],
                50.0
            ) >= 47.0
        );
        let repeat = tidy_positions(&data, &rows, &levels, &widths, &group_of);
        assert_eq!(spread, repeat);
    }
}

#[cfg(test)]
mod ancestor_layout_tests {
    use std::collections::{HashMap, HashSet};

    use crate::model::{Family, Gender, TreeData, person};

    use super::{
        TreeRelations, ancestor_drag_offsets, block_ancestor_occ_drag, collect_ancestor_occs,
        layout_ancestor_occs, occ_neighbor_gap, persons_are_partners, UNMARRIED_GAP_EXTRA,
    };

    fn ancestor_tree() -> TreeData {
        let mut data = TreeData::default();
        data.people.push(person("r", "R", "Root", "", Gender::Unknown));
        data.people.push(person("f", "F", "Vater", "", Gender::Male));
        data.people.push(person("m", "M", "Mutter", "", Gender::Female));
        data.people.push(person("gf", "G", "Grossvater", "", Gender::Male));
        data.people.push(person("gm", "G", "Grossmutter", "", Gender::Female));
        data.families.push(Family {
            id: "f0".into(),
            parent_a: Some("f".into()),
            parent_b: Some("m".into()),
            children: vec!["r".into()],
        });
        data.families.push(Family {
            id: "f1".into(),
            parent_a: Some("gf".into()),
            parent_b: Some("gm".into()),
            children: vec!["f".into()],
        });
        data
    }

    fn ancestor_layout(data: &TreeData) -> (Vec<(String, String)>, HashMap<String, f32>) {
        let relations = TreeRelations::new(data);
        let expanded = HashSet::new();
        let (occs, _) = collect_ancestor_occs("r", &relations, 0, &expanded, None);
        let occ_index: HashMap<&str, usize> = occs
            .iter()
            .enumerate()
            .map(|(index, occ)| (occ.occ_id.as_str(), index))
            .collect();
        let widths: HashMap<&str, f32> = [
            ("r", 215.0),
            ("f", 215.0),
            ("m", 215.0),
            ("gf", 215.0),
            ("gm", 215.0),
        ]
        .into_iter()
        .collect();
        let root_occ = occs[0].occ_id.clone();
        let spread =
            layout_ancestor_occs(&root_occ, &occs, &occ_index, &relations, data, &widths, 48.0, UNMARRIED_GAP_EXTRA);
        let pairs: Vec<(String, String)> = occs
            .iter()
            .map(|occ| (occ.person.id.clone(), occ.occ_id.clone()))
            .collect();
        (pairs, spread)
    }

    fn occ_center(
        pairs: &[(String, String)],
        spread: &HashMap<String, f32>,
        person: &str,
    ) -> f32 {
        let (_, occ_id) = pairs.iter().find(|(id, _)| id == person).unwrap();
        spread[occ_id]
    }

    #[test]
    fn children_stay_centered_between_ancestors() {
        let data = ancestor_tree();
        let (pairs, spread) = ancestor_layout(&data);
        // Wurzel in der Mitte, Eltern symmetrisch darum.
        let (_, root_occ) = &pairs[0];
        assert_eq!(spread[root_occ], 0.0);
        let father = occ_center(&pairs, &spread, "f");
        let mother = occ_center(&pairs, &spread, "m");
        assert!((father + mother).abs() < 1e-3);
        assert!(father < 0.0 && mother > 0.0);
        // Großeltern symmetrisch um den Vater.
        let gf = occ_center(&pairs, &spread, "gf");
        let gm = occ_center(&pairs, &spread, "gm");
        assert!(((gf + gm) / 2.0 - father).abs() < 1.0);
    }

    fn drag_offsets(data: &TreeData, manual: &[(&str, f32)]) -> HashMap<String, f32> {
        let relations = TreeRelations::new(data);
        let expanded = HashSet::new();
        let (occs, _) = collect_ancestor_occs("r", &relations, 0, &expanded, None);
        let manual_offsets: HashMap<String, f32> = manual
            .iter()
            .map(|(id, val)| ((*id).to_string(), *val))
            .collect();
        let eff = ancestor_drag_offsets(&occs, &manual_offsets);
        occs.iter()
            .map(|occ| (occ.person.id.clone(), eff.get(&occ.occ_id).copied().unwrap_or(0.0)))
            .collect()
    }

    #[test]
    fn dragged_father_halves_to_child_and_keeps_partner() {
        let data = ancestor_tree();
        let eff = drag_offsets(&data, &[("f", 100.0)]);
        // Gezogener Vater voll, Mutter (Partnerin) bleibt stehen, Kind mittig.
        assert_eq!(eff["f"], 100.0);
        assert_eq!(eff["m"], 0.0);
        assert_eq!(eff["r"], 50.0);
        // Vorfahren des gezogenen Vaters folgen starr.
        assert_eq!(eff["gf"], 100.0);
        assert_eq!(eff["gm"], 100.0);
    }

    #[test]
    fn dragged_root_moves_ancestors_along() {
        let data = ancestor_tree();
        let eff = drag_offsets(&data, &[("r", 80.0)]);
        for person in ["r", "f", "m", "gf", "gm"] {
            assert_eq!(eff[person], 80.0);
        }
    }

    #[test]
    fn neighbor_gap_follows_common_family() {
        // f und m je eigene Familie (kein Paar) → Extra-Abstand.
        let mut data = TreeData::default();
        data.people.push(person("r", "R", "Root", "", Gender::Unknown));
        data.people.push(person("f", "F", "Vater", "", Gender::Male));
        data.people.push(person("m", "M", "Mutter", "", Gender::Female));
        data.link_child("f", "r");
        data.link_child("m", "r");
        assert!(!persons_are_partners("f", "m", &data));
        let gap_of = |data: &TreeData| -> f32 {
            let relations = TreeRelations::new(data);
            let expanded = HashSet::new();
            let (occs, _) = collect_ancestor_occs("r", &relations, 0, &expanded, None);
            let occ_index: HashMap<&str, usize> = occs
                .iter()
                .enumerate()
                .map(|(index, occ)| (occ.occ_id.as_str(), index))
                .collect();
            let occ_of = |person: &str| {
                occs.iter()
                    .find(|occ| occ.person.id == person)
                    .unwrap()
                    .occ_id
                    .clone()
            };
            let (father, mother) = (occ_of("f"), occ_of("m"));
            occ_neighbor_gap(
                &father,
                &mother,
                &occs,
                &occ_index,
                data,
                48.0,
                UNMARRIED_GAP_EXTRA,
            )
        };
        assert_eq!(gap_of(&data), 78.0);
        // Per link_partner zur gemeinsamen Familie → enges Paar.
        data.link_partner("f", "m");
        assert!(persons_are_partners("f", "m", &data));
        assert_eq!(gap_of(&data), 48.0);
    }

    /// Kleinster Kantenabstand (Mitte-zu-Mitte minus halbe Breiten) über alle Ebenen.
    fn min_edge_gap(
        spread: &HashMap<String, f32>,
        rows: &[Vec<String>],
        occs: &[super::AncestorOcc<'_>],
        occ_index: &HashMap<&str, usize>,
        widths: &HashMap<&str, f32>,
    ) -> f32 {
        let mut min = f32::INFINITY;
        for ids in rows {
            let mut centers: Vec<(f32, f32)> = ids
                .iter()
                .map(|id| {
                    let width = occ_index
                        .get(id.as_str())
                        .map(|&idx| {
                            widths
                                .get(occs[idx].person.id.as_str())
                                .copied()
                                .unwrap_or(215.0)
                        })
                        .unwrap_or(215.0);
                    (spread.get(id).copied().unwrap_or(0.0), width)
                })
                .collect();
            centers.sort_by(|a, b| a.0.total_cmp(&b.0));
            for pair in centers.windows(2) {
                min = min.min(pair[1].0 - pair[0].0 - (pair[0].1 + pair[1].1) / 2.0);
            }
        }
        min
    }

    #[test]
    fn unlinked_parents_keep_extra_gap_in_layout() {
        // Eltern ohne gemeinsame Familie (je eigene Familie) → kein Paar.
        let mut data = TreeData::default();
        data.people.push(person("r", "R", "Root", "", Gender::Unknown));
        data.people.push(person("f", "F", "Vater", "", Gender::Male));
        data.people.push(person("m", "M", "Mutter", "", Gender::Female));
        data.link_child("f", "r");
        data.link_child("m", "r");
        assert!(!persons_are_partners("f", "m", &data));
        let relations = TreeRelations::new(&data);
        let expanded = HashSet::new();
        let (occs, _) = collect_ancestor_occs("r", &relations, 0, &expanded, None);
        let occ_index: HashMap<&str, usize> = occs
            .iter()
            .enumerate()
            .map(|(index, occ)| (occ.occ_id.as_str(), index))
            .collect();
        let widths: HashMap<&str, f32> =
            [("r", 215.0), ("f", 215.0), ("m", 215.0)].into_iter().collect();
        let root = occs[0].occ_id.clone();
        let spread = layout_ancestor_occs(
            &root,
            &occs,
            &occ_index,
            &relations,
            &data,
            &widths,
            48.0,
            UNMARRIED_GAP_EXTRA,
        );
        let center = |person: &str| {
            let occ = occs.iter().find(|occ| occ.person.id == person).unwrap();
            spread[&occ.occ_id]
        };
        // Mittenabstand = Breite + Baum-Abstand (48) + Extra (30).
        assert!((center("m") - center("f") - (215.0 + 48.0 + 30.0)).abs() < 1e-3);
    }

    #[test]
    fn subtree_boundaries_keep_extra_gap_for_non_partners() {
        let mut data = ancestor_tree();
        data.people.push(person("mf", "M", "Muttervater", "", Gender::Male));
        data.people.push(person("mm", "M", "Muttermutter", "", Gender::Female));
        data.families.push(Family {
            id: "f2".into(),
            parent_a: Some("mf".into()),
            parent_b: Some("mm".into()),
            children: vec!["m".into()],
        });
        let relations = TreeRelations::new(&data);
        let expanded = HashSet::new();
        let (occs, _) = collect_ancestor_occs("r", &relations, 0, &expanded, None);
        let occ_index: HashMap<&str, usize> = occs
            .iter()
            .enumerate()
            .map(|(index, occ)| (occ.occ_id.as_str(), index))
            .collect();
        let widths: HashMap<&str, f32> = [
            ("r", 215.0),
            ("f", 215.0),
            ("m", 215.0),
            ("gf", 215.0),
            ("gm", 215.0),
            ("mf", 215.0),
            ("mm", 215.0),
        ]
        .into_iter()
        .collect();
        let root = occs[0].occ_id.clone();
        let max_level = occs.iter().map(|occ| occ.level).max().unwrap_or(0);
        let mut rows: Vec<Vec<String>> = vec![Vec::new(); max_level + 1];
        for occ in &occs {
            rows[occ.level].push(occ.occ_id.clone());
        }
        let spread_of = |extra: f32| {
            layout_ancestor_occs(
                &root,
                &occs,
                &occ_index,
                &relations,
                &data,
                &widths,
                48.0,
                extra,
            )
        };
        let boundary_gap = |spread: &HashMap<String, f32>| {
            let center = |person: &str| {
                let occ = occs.iter().find(|occ| occ.person.id == person).unwrap();
                spread[&occ.occ_id]
            };
            (center("mf") - 107.5) - (center("gm") + 107.5)
        };
        // Grenze zwischen den Zweigen (gm | mf, keine gemeinsame Familie):
        // Baum-Abstand (48) + Extra (30) — und mit Extra 0 der reine
        // Baum-Abstand. Paare bleiben kompakt (Minimum 48).
        let spread = spread_of(UNMARRIED_GAP_EXTRA);
        assert!((boundary_gap(&spread) - 78.0).abs() < 1e-3);
        assert!((min_edge_gap(&spread, &rows, &occs, &occ_index, &widths) - 48.0).abs() < 1e-3);
        let spread = spread_of(0.0);
        assert!((boundary_gap(&spread) - 48.0).abs() < 1e-3);
    }

    #[test]
    fn implex_duplicate_next_to_spouse_stays_compact() {
        // Ahnenverlust: gf ist Vater von f (mit gm) und von m (mit mm2).
        let mut data = ancestor_tree();
        data.people.push(person("mm2", "M", "Muttermutter2", "", Gender::Female));
        data.families.push(Family {
            id: "f2".into(),
            parent_a: Some("gf".into()),
            parent_b: Some("mm2".into()),
            children: vec!["m".into()],
        });
        let relations = TreeRelations::new(&data);
        let expanded = HashSet::new();
        let (occs, _) = collect_ancestor_occs("r", &relations, 0, &expanded, None);
        // gf kommt doppelt vor (kanonisch + Duplikat im m-Zweig).
        assert_eq!(occs.iter().filter(|occ| occ.person.id == "gf").count(), 2);
        let occ_index: HashMap<&str, usize> = occs
            .iter()
            .enumerate()
            .map(|(index, occ)| (occ.occ_id.as_str(), index))
            .collect();
        let widths: HashMap<&str, f32> = [
            ("r", 215.0),
            ("f", 215.0),
            ("m", 215.0),
            ("gf", 215.0),
            ("gm", 215.0),
            ("mm2", 215.0),
        ]
        .into_iter()
        .collect();
        let root = occs[0].occ_id.clone();
        let spread =
            layout_ancestor_occs(&root, &occs, &occ_index, &relations, &data, &widths, 48.0, UNMARRIED_GAP_EXTRA);
        // Jedes Vorkommen liegt im kanonischen Spread (kein Fallback nötig).
        for occ in &occs {
            assert!(spread.contains_key(&occ.occ_id), "{}", occ.occ_id);
        }
        let max_level = occs.iter().map(|occ| occ.level).max().unwrap_or(0);
        let mut rows: Vec<Vec<String>> = vec![Vec::new(); max_level + 1];
        for occ in &occs {
            rows[occ.level].push(occ.occ_id.clone());
        }
        // Das Duplikat steht neben seiner Partnerin (gemeinsame Familie f1):
        // eng, trotz Extra-Einstellung.
        assert!((min_edge_gap(&spread, &rows, &occs, &occ_index, &widths) - 48.0).abs() < 1e-3);
    }

    #[test]
    fn slider_leaves_pairs_compact() {
        // Paar mit gemeinsamer Familie (f0) bleibt kompakt — egal, wie groß
        // das Nicht-Partner-Extra eingestellt ist (der gemeldete Fehler).
        let data = ancestor_tree();
        assert!(persons_are_partners("f", "m", &data));
        let relations = TreeRelations::new(&data);
        let expanded = HashSet::new();
        let (occs, _) = collect_ancestor_occs("r", &relations, 0, &expanded, None);
        let occ_index: HashMap<&str, usize> = occs
            .iter()
            .enumerate()
            .map(|(index, occ)| (occ.occ_id.as_str(), index))
            .collect();
        let widths: HashMap<&str, f32> = [
            ("r", 215.0),
            ("f", 215.0),
            ("m", 215.0),
            ("gf", 215.0),
            ("gm", 215.0),
        ]
        .into_iter()
        .collect();
        let root = occs[0].occ_id.clone();
        let distance = |extra: f32| {
            let spread = layout_ancestor_occs(
                &root,
                &occs,
                &occ_index,
                &relations,
                &data,
                &widths,
                48.0,
                extra,
            );
            let father = occs.iter().find(|occ| occ.person.id == "f").unwrap();
            let mother = occs.iter().find(|occ| occ.person.id == "m").unwrap();
            spread[&mother.occ_id] - spread[&father.occ_id]
        };
        // Mittenabstand = Breite + Baum-Abstand — das Extra fasst Paare
        // grundsätzlich nicht an.
        assert!((distance(0.0) - 263.0).abs() < 1e-3);
        assert!((distance(60.0) - 263.0).abs() < 1e-3);
    }

    #[test]
    fn dragged_card_stops_before_collision() {
        let data = ancestor_tree();
        let relations = TreeRelations::new(&data);
        let expanded = HashSet::new();
        let (occs, _) = collect_ancestor_occs("r", &relations, 0, &expanded, None);
        let occ_index: HashMap<&str, usize> = occs
            .iter()
            .enumerate()
            .map(|(index, occ)| (occ.occ_id.as_str(), index))
            .collect();
        let widths: HashMap<&str, f32> = [
            ("r", 215.0),
            ("f", 215.0),
            ("m", 215.0),
            ("gf", 215.0),
            ("gm", 215.0),
        ]
        .into_iter()
        .collect();
        let max_level = occs.iter().map(|occ| occ.level).max().unwrap_or(0);
        let mut rows: Vec<Vec<String>> = vec![Vec::new(); max_level + 1];
        for occ in &occs {
            rows[occ.level].push(occ.occ_id.clone());
        }
        let root = occs[0].occ_id.clone();
        let base =
            layout_ancestor_occs(&root, &occs, &occ_index, &relations, &data, &widths, 48.0, UNMARRIED_GAP_EXTRA);
        // Ruhig gestellt: kein Überlapp im Basislayout.
        assert!(min_edge_gap(&base, &rows, &occs, &occ_index, &widths) >= 48.0 - 1e-3);
        // Kräftiger Ruck am Vater nach rechts: ohne Blockade führe er in die
        // stehende Mutter (Kind käme mittig mit).
        let mut manual: HashMap<String, f32> =
            [("f".to_string(), 200.0)].into_iter().collect();
        let eff = ancestor_drag_offsets(&occs, &manual);
        let mut spread = base.clone();
        for (id, val) in &eff {
            *spread.get_mut(id).unwrap() += *val;
        }
        assert!(min_edge_gap(&spread, &rows, &occs, &occ_index, &widths) < 16.0);
        block_ancestor_occ_drag(&mut spread, &rows, &occs, &occ_index, &widths, &mut manual, "f");
        // Geklemmt: Versatz deutlich kleiner, überall MIN_CARD_GAP Luft.
        assert!(manual["f"] < 200.0);
        assert!(manual["f"] > 0.0);
        assert!(min_edge_gap(&spread, &rows, &occs, &occ_index, &widths) >= 16.0 - 1e-3);
        // Kleiner legaler Ruck bleibt unverändert.
        let mut manual: HashMap<String, f32> =
            [("f".to_string(), 10.0)].into_iter().collect();
        let eff = ancestor_drag_offsets(&occs, &manual);
        let mut spread = base.clone();
        for (id, val) in &eff {
            *spread.get_mut(id).unwrap() += *val;
        }
        block_ancestor_occ_drag(&mut spread, &rows, &occs, &occ_index, &widths, &mut manual, "f");
        assert_eq!(manual["f"], 10.0);
    }

    #[test]
    fn same_gender_parents_keep_both_slots() {
        let mut data = TreeData::default();
        data.people.push(person("r", "R", "Root", "", Gender::Unknown));
        data.people.push(person("a", "A", "Vater", "", Gender::Male));
        data.people.push(person("b", "B", "Vater", "", Gender::Male));
        data.families.push(Family {
            id: "f0".into(),
            parent_a: Some("a".into()),
            parent_b: Some("b".into()),
            children: vec!["r".into()],
        });
        let relations = TreeRelations::new(&data);
        let expanded = HashSet::new();
        let (occs, _) = collect_ancestor_occs("r", &relations, 0, &expanded, None);
        let child = occs.iter().find(|occ| occ.person.id == "r").unwrap();
        // Beide Väter verlinkt (kein Überschreiben), kein Zusammenbruch.
        let father = child.father_occ.clone().unwrap();
        let mother = child.mother_occ.clone().unwrap();
        assert_ne!(father, mother);
        let widths: HashMap<&str, f32> =
            [("r", 215.0), ("a", 215.0), ("b", 215.0)].into_iter().collect();
        let occ_index: HashMap<&str, usize> = occs
            .iter()
            .enumerate()
            .map(|(index, occ)| (occ.occ_id.as_str(), index))
            .collect();
        let spread =
            layout_ancestor_occs(&occs[0].occ_id.clone(), &occs, &occ_index, &relations, &data, &widths, 48.0, UNMARRIED_GAP_EXTRA);
        assert!(spread.contains_key(&father));
        assert!(spread.contains_key(&mother));
        assert!(spread[&father] < spread[&mother]);
    }
}

/// Hängt nicht sichtbare Partner-Pseudokarten starr an ihre sichtbare Person an.
fn attach_descendant_partners<'a>(
    spread: &mut HashMap<&'a str, f32>,
    relations: &TreeRelations<'a>,
    levels: &HashMap<&'a str, usize>,
    widths: &HashMap<&'a str, f32>,
    shown_partners: &HashSet<&'a str>,
    couple_gap: f32,
    owner: &'a str,
) {
    let Some(&center) = spread.get(owner) else {
        return;
    };
    let mut cursor = center + widths.get(owner).copied().unwrap_or(215.0) / 2.0 + couple_gap;
    for partner in relations.partners_of(owner) {
        if levels.contains_key(partner.id.as_str())
            || !shown_partners.contains(partner.id.as_str())
        {
            continue;
        }
        let width = widths
            .get(partner.id.as_str())
            .copied()
            .unwrap_or(215.0);
        spread.insert(partner.id.as_str(), cursor + width / 2.0);
        cursor += width + couple_gap;
    }
}

fn descendant_sort_key<'a>(
    relations: &'a TreeRelations<'a>,
    id: &'a str,
) -> (i32, i32, i32, i32, &'a str) {
    relations.find(id).map(birth_key).unwrap_or((1, 0, 0, 0, id))
}

/// Neue Methode: ankerbasierte Einpass-Packung für den vertikalen Nachfahrenbaum.
///
/// Statt Attraktion und Abstoßung zwölfmal gegeneinander laufen zu lassen,
/// wird jede Generation genau einmal von links nach rechts und einmal zurück
/// durchgegangen:
///
/// 1. Jede Geschwistergruppe erhält aus dem bereits fertigen Elternpaar einen
///    Zielmittelpunkt (deren Junction), damit Verbindungslinien nicht kreuzen.
/// 2. Innerhalb einer Gruppe stehen die Karten im konfigurierten `gap`.
/// 3. Gruppen werden deterministisch so weit wie nötig nach rechts geschoben,
///    bis Kante zu Kante mindestens `gap` plus sichtbare Container-Ränder
///    bleibt.
/// 4. Ein einziger Rückwärtslauf zieht Gruppen nach links auf ihr Ziel
///    zurück, ohne eine einmal hergestellte Kollisionsfreiheit zu brechen.
///
/// Damit gibt es keine sich wiederholende Reparatur alter, tiefer
/// Überlappungen in einem einzigen Schritt mehr.
#[allow(clippy::too_many_arguments)]
fn tidy_descendant_spread_vertical<'a>(
    root: &str,
    spread: &mut HashMap<&'a str, f32>,
    rows: &[Vec<&'a str>],
    relations: &TreeRelations<'a>,
    data: &TreeData,
    levels: &HashMap<&'a str, usize>,
    widths: &HashMap<&'a str, f32>,
    group_of: &HashMap<&'a str, &'a str>,
    shown_partners: &HashSet<&'a str>,
    gap: f32,
    container_padding: f32,
    couple_gap: f32,
) {
    if rows.is_empty() {
        return;
    }
    let visible = |id: &str| levels.contains_key(id);
    let mut half_left: HashMap<&str, f32> = HashMap::new();
    let mut half_right: HashMap<&str, f32> = HashMap::new();
    for &id in widths.keys() {
        half_left.insert(id, widths[id] / 2.0);
        let mut right = widths[id] / 2.0;
        for partner in relations.partners_of(id) {
            if !visible(&partner.id) && shown_partners.contains(partner.id.as_str()) {
                right += widths
                    .get(partner.id.as_str())
                    .copied()
                    .unwrap_or(215.0)
                    + couple_gap;
            }
        }
        half_right.insert(id, right);
    }

    // Bottom-up-Breitenreservierung: Jede Familie meldet die Breite ihres
    // sichtbaren Nachkommenblocks nach oben, damit Elternpaare weit genug
    // auseinanderrücken und Verbindungslinien senkrecht bleiben. Eine Person
    // mit mehreren Familien (z. B. drei Partner mit Kindern) reserviert die
    // SUMME aller Familienbreiten: Nur eine Familie kann exakt senkrecht
    // unter ihr stehen, die übrigen brauchen trotzdem kollisionsfreien Raum.
    // Alle Reserven wachsen monoton, daher konvergiert die Relaxierung über
    // die Generationen deterministisch.
    let mut raw_footprint: HashMap<&str, f32> = HashMap::new();
    for &id in widths.keys() {
        raw_footprint.insert(id, half_left[id] + half_right[id]);
    }
    // Repräsentative sichtbare Kinder je Familie (wie Zeichnung und Blöcke:
    // nur die primäre Familie trägt den Zweig).
    fn rep_key<'x>(
        relations: &TreeRelations<'x>,
        id: &'x str,
    ) -> (i32, i32, i32, i32, &'x str) {
        relations.find(id).map(birth_key).unwrap_or((1, 0, 0, 0, id))
    }
    let mut rep_children: HashMap<&str, Vec<&str>> = HashMap::new();
    for family in &data.families {
        let mut kids: Vec<&str> = family
            .children
            .iter()
            .map(String::as_str)
            .filter(|child| {
                visible(child) && group_of.get(child).copied() == Some(family.id.as_str())
            })
            .collect();
        kids.sort_by(|left, right| rep_key(relations, left).cmp(&rep_key(relations, right)));
        rep_children.insert(family.id.as_str(), kids);
    }
    let mut person_req: HashMap<&str, f32> = raw_footprint.clone();
    for _ in 0..rows.len().max(1) {
        let mut family_sum: HashMap<&str, f32> = HashMap::new();
        for family in &data.families {
            let kids: &[&str] = rep_children
                .get(family.id.as_str())
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let mut pair_width = 0.0f32;
            let mut visible_parents = 0u32;
            for parent in [&family.parent_a, &family.parent_b].into_iter().flatten() {
                let pid = parent.as_str();
                if visible(pid) {
                    pair_width += raw_footprint.get(pid).copied().unwrap_or(0.0);
                    visible_parents += 1;
                }
            }
            if visible_parents > 1 {
                pair_width += gap * (visible_parents - 1) as f32;
            }
            let mut child_span = 0.0f32;
            for (index, child) in kids.iter().enumerate() {
                if index > 0 {
                    child_span += gap;
                }
                child_span += person_req.get(child).copied().unwrap_or(0.0);
            }
            if kids.len() > 1 {
                // Sichtbarer Container-Rand je Seite (wie in der Packung).
                child_span += 2.0 * container_padding;
            }
            if visible_parents > 0 {
                let share = pair_width.max(child_span) / visible_parents as f32;
                for parent in [&family.parent_a, &family.parent_b].into_iter().flatten() {
                    let pid = parent.as_str();
                    if visible(pid) {
                        *family_sum.entry(pid).or_insert(0.0) += share;
                    }
                }
            }
        }
        for (pid, sum) in family_sum {
            let raw = raw_footprint.get(pid).copied().unwrap_or(0.0);
            person_req.insert(pid, raw.max(sum));
        }
    }
    // Zusatzbreite symmetrisch um die Kartenmitte legen, damit
    // Paar-Junctions über ihren Nachkommenblöcken zentriert bleiben.
    for (&id, &req) in person_req.iter() {
        let raw = raw_footprint.get(id).copied().unwrap_or(0.0);
        if req > raw {
            let extra = (req - raw) / 2.0;
            if let Some(v) = half_left.get_mut(id) {
                *v += extra;
            }
            if let Some(v) = half_right.get_mut(id) {
                *v += extra;
            }
        }
    }

    let mut path_nodes: HashSet<&str> = HashSet::from([root]);
    for family in &data.families {
        for child in &family.children {
            if levels.contains_key(child.as_str()) {
                path_nodes.insert(child.as_str());
            }
        }
    }

    // PACK-Positionen als stabile Fallback-Ziele für Verknüpfungen ohne
    // bereits fertige Elterngeneration.
    let pack_positions: HashMap<&str, f32> = spread.clone();

    for (row, ids) in rows.iter().enumerate() {
        if ids.is_empty() {
            continue;
        }

        // Geschwister strikt in ihrer Herkunftsfamilie zusammenfassen. Die
        // Reihenfolge der Zeile aus dem BFS bleibt dabei erhalten; Details
        // sortiert der Geburtsdatenschlüssel.
        let mut ordered: Vec<(String, Vec<&'a str>)> = Vec::new();
        for &id in ids {
            let key = group_of.get(id).copied().unwrap_or(id).to_string();
            match ordered.iter_mut().find(|(existing, _)| *existing == key) {
                Some((_, members)) => members.push(id),
                None => ordered.push((key, vec![id])),
            }
        }

        let mut blocks: Vec<DescendantBlock<'a>> = Vec::with_capacity(ordered.len());
        for (key, members) in ordered {
            let mut members = members;
            members.sort_by(|left, right| {
                descendant_sort_key(relations, left).cmp(&descendant_sort_key(relations, right))
            });
            let mut offsets = vec![0.0f32; members.len()];
            for index in 1..members.len() {
                let previous = members[index - 1];
                let current = members[index];
                offsets[index] = offsets[index - 1]
                    + half_right[previous]
                    + gap
                    + half_left[current];
            }
            let half_left = half_left[members[0]];
            let half_right = offsets[members.len() - 1] + half_right[members[members.len() - 1]];
            let pad = if members.len() > 1 {
                container_padding
            } else {
                0.0
            };
            let base = members
                .iter()
                .map(|id| pack_positions.get(id).copied().unwrap_or(0.0))
                .sum::<f32>()
                / members.len() as f32;
            let target = if row == 0 {
                if members.iter().any(|id| *id == root) {
                    Some(0.0)
                } else {
                    None
                }
            } else {
                let family = data.families.iter().find(|family| family.id == key);
                family.and_then(|family| {
                    let mut placed: Vec<(&str, f32)> = Vec::new();
                    for parent in [&family.parent_a, &family.parent_b] {
                        let Some(parent_id) = parent else {
                            continue;
                        };
                        let parent_id = parent_id.as_str();
                        if let Some(&parent_row) = levels.get(parent_id) {
                            if parent_row < row
                                && let Some(&center) = spread.get(parent_id)
                            {
                                placed.push((parent_id, center));
                            }
                        } else {
                            for blood in relations.partners_of(parent_id) {
                                let owner = blood.id.as_str();
                                if let Some(&owner_row) = levels.get(owner) {
                                    if owner_row < row
                                        && let Some(&center) = spread.get(parent_id)
                                    {
                                        placed.push((parent_id, center));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    if placed.is_empty() {
                        return None;
                    }
                    if placed.len() == 2 {
                        let first_main = path_nodes.contains(placed[0].0);
                        let second_main = path_nodes.contains(placed[1].0);
                        if first_main && !second_main {
                            return Some(placed[1].1);
                        }
                        if second_main && !first_main {
                            return Some(placed[0].1);
                        }
                    }
                    Some(
                        placed.iter().map(|(_, center)| *center).sum::<f32>()
                            / placed.len() as f32,
                    )
                })
            };
            blocks.push(DescendantBlock {
                key,
                members,
                offsets,
                half_left,
                half_right,
                pad,
                target,
                base,
            });
        }

        // Die Familienblöcke nach ihrer Zielkoordinate (bzw. ihrer ursprünglichen Basisposition bei fehlendem Ziel) sortieren.
        // Das stellt sicher, dass die Blöcke exakt der horizontalen Ordnung ihrer Eltern folgen und sich Verbindungslinien niemals kreuzen.
        blocks.sort_by(|left, right| {
            let left_coord = left.target.unwrap_or(left.base);
            let right_coord = right.target.unwrap_or(right.base);
            left_coord
                .total_cmp(&right_coord)
                .then_with(|| left.key.cmp(&right.key))
        });

        // Ein Vorwärts- und ein Rückwärtslauf richten die Blöcke an ihrer
        // echten Mitte aus und halten dabei beide Kollisionsgrenzen ein.
        let starts = fit_descendant_block_starts(&blocks, gap);

        for (index, block) in blocks.iter().enumerate() {
            for (member_index, id) in block.members.iter().enumerate() {
                spread.insert(
                    *id,
                    starts[index] + block.half_left + block.offsets[member_index],
                );
            }
            for &id in &block.members {
                attach_descendant_partners(
                    spread,
                    relations,
                    levels,
                    widths,
                    shown_partners,
                    couple_gap,
                    id,
                );
            }
        }
    }
}

#[derive(Clone)]
struct AncestorOcc<'a> {
    occ_id: String,
    person: &'a Person,
    level: usize,
    is_canonical: bool,
    father_occ: Option<String>,
    mother_occ: Option<String>,
    #[allow(dead_code)]
    child_occ: Option<String>,
}

fn collect_ancestor_occs<'a>(
    root: &'a str,
    relations: &TreeRelations<'a>,
    generation_limit: usize,
    expanded: &HashSet<String>,
    person_limit: Option<usize>,
) -> (Vec<AncestorOcc<'a>>, bool) {
    let mut occs: Vec<AncestorOcc<'a>> = Vec::new();
    let mut occ_pos: HashMap<String, usize> = HashMap::new();
    let mut person_canonical_occ = HashMap::new();
    let mut queue = VecDeque::new();
    queue.push_back((root, 0, None::<String>));
    let mut limited = false;
    let mut occ_counter = 0;
    while let Some((p_id, lvl, child_occ)) = queue.pop_front() {
        if let Some(person) = relations.find(p_id) {
            if let Some(limit) = person_limit {
                if occ_counter >= limit {
                    limited = true;
                    continue;
                }
            }
            let occ_id = format!("occ_{}", occ_counter);
            occ_counter += 1;
            let is_canonical = !person_canonical_occ.contains_key(p_id);
            if is_canonical {
                person_canonical_occ.insert(p_id, occ_id.clone());
            }
            let occ = AncestorOcc {
                occ_id: occ_id.clone(),
                person,
                level: lvl,
                is_canonical,
                father_occ: None,
                mother_occ: None,
                child_occ: child_occ.clone(),
            };
            if let Some(ref c_id) = child_occ {
                if let Some(&child_idx) = occ_pos.get(c_id) {
                    let child_node = &mut occs[child_idx];
                    // Slot nach Geschlecht bevorzugen, bei belegtem Slot auf
                    // den anderen ausweichen (Fallback für fehlendes oder
                    // gleiches Geschlecht — sonst ginge ein Elternteil
                    // verloren und die Zeichenlogik fiele zusammen).
                    let (primary, fallback) = match person.gender {
                        crate::model::Gender::Male => {
                            (&mut child_node.father_occ, &mut child_node.mother_occ)
                        }
                        crate::model::Gender::Female => {
                            (&mut child_node.mother_occ, &mut child_node.father_occ)
                        }
                        crate::model::Gender::Unknown => {
                            (&mut child_node.father_occ, &mut child_node.mother_occ)
                        }
                    };
                    if primary.is_none() {
                        *primary = Some(occ_id.clone());
                    } else if fallback.is_none() {
                        *fallback = Some(occ_id.clone());
                    }
                }
            }
            occ_pos.insert(occ_id.clone(), occs.len());
            occs.push(occ);
            let can_expand = is_canonical && (generation_limit == 0 || lvl + 1 < generation_limit || expanded.contains(p_id));
            if can_expand {
                let parents = relations.parents_of(p_id);
                for parent in parents {
                    queue.push_back((parent.id.as_str(), lvl + 1, Some(occ_id.clone())));
                }
            }
        }
    }
    (occs, limited)
}

fn layout_ancestor_occs<'a>(
    occ_id: &str,
    occs: &[AncestorOcc<'a>],
    occ_index: &HashMap<&str, usize>,
    relations: &TreeRelations<'a>,
    data: &TreeData,
    widths: &HashMap<&str, f32>,
    gap: f32,
    unmarried_extra: f32,
) -> HashMap<String, f32> {
    let mut layout = HashMap::new();
    layout.insert(occ_id.to_string(), 0.0);
    let node = match occ_index.get(occ_id).map(|&idx| &occs[idx]) {
        Some(n) => n,
        None => return layout,
    };
    if !node.is_canonical {
        return layout;
    }
    match (&node.father_occ, &node.mother_occ) {
        (Some(f), Some(m)) => {
            let f_layout = layout_ancestor_occs(
                f,
                occs,
                occ_index,
                relations,
                data,
                widths,
                gap,
                unmarried_extra,
            );
            let m_layout = layout_ancestor_occs(
                m,
                occs,
                occ_index,
                relations,
                data,
                widths,
                gap,
                unmarried_extra,
            );
            let mut min_distance = 0.0f32;
            let mut levels_in_subtrees = HashSet::new();
            for fid in f_layout.keys() {
                if let Some(&idx) = occ_index.get(fid.as_str()) {
                    levels_in_subtrees.insert(occs[idx].level);
                }
            }
            for mid in m_layout.keys() {
                if let Some(&idx) = occ_index.get(mid.as_str()) {
                    levels_in_subtrees.insert(occs[idx].level);
                }
            }
            let parent_level = node.level + 1;
            for lvl in levels_in_subtrees {
                let mut max_f_right: Option<(f32, &str)> = None;
                for (fid, &f_offset) in &f_layout {
                    if let Some(&idx) = occ_index.get(fid.as_str()) {
                        let f_node = &occs[idx];
                        if f_node.level == lvl {
                            let w = widths.get(f_node.person.id.as_str()).copied().unwrap_or(215.0);
                            let right = f_offset + w / 2.0;
                            if max_f_right.is_none_or(|(best, _)| right > best) {
                                max_f_right = Some((right, fid.as_str()));
                            }
                        }
                    }
                }
                let mut min_m_left: Option<(f32, &str)> = None;
                for (mid, &m_offset) in &m_layout {
                    if let Some(&idx) = occ_index.get(mid.as_str()) {
                        let m_node = &occs[idx];
                        if m_node.level == lvl {
                            let w = widths.get(m_node.person.id.as_str()).copied().unwrap_or(215.0);
                            let left = m_offset - w / 2.0;
                            if min_m_left.is_none_or(|(best, _)| left < best) {
                                min_m_left = Some((left, mid.as_str()));
                            }
                        }
                    }
                }
                if let (Some((f_right, f_occ)), Some((m_left, m_occ))) =
                    (max_f_right, min_m_left)
                {
                    let mut needed_sep = f_right - m_left + gap;
                    // Auch an Teilbaum-Grenzen: mehr Platz, wenn zwischen den
                    // Grenzkarten keine Partner-Verbindung besteht. Die
                    // Elternebene selbst ist ausgenommen — dort bilden Vater
                    // und Mutter konstruktionsbedingt immer die Grenze und
                    // ihr Extra steckt bereits in `sep` (kein doppeltes
                    // Zählen; Duplikate liegen stets eine Ebene tiefer).
                    if lvl != parent_level
                        && !occs_are_partners(f_occ, m_occ, occs, occ_index, data)
                    {
                        needed_sep += unmarried_extra;
                    }
                    if needed_sep > min_distance {
                        min_distance = needed_sep;
                    }
                }
            }
            let father_node = &occs[occ_index[f.as_str()]];
            let mother_node = &occs[occ_index[m.as_str()]];
            let wf = widths.get(father_node.person.id.as_str()).copied().unwrap_or(215.0);
            let wm = widths.get(mother_node.person.id.as_str()).copied().unwrap_or(215.0);
            let default_sep = (wf + wm) / 2.0 + gap;
            let extra = if persons_are_partners(
                father_node.person.id.as_str(),
                mother_node.person.id.as_str(),
                data,
            ) {
                0.0
            } else {
                unmarried_extra
            };
            let sep = min_distance.max(default_sep + extra);
            let shift_f = -sep / 2.0 + (wm - wf) / 4.0;
            let shift_m = sep / 2.0 + (wm - wf) / 4.0;
            for (fid, f_offset) in f_layout {
                layout.insert(fid, f_offset + shift_f);
            }
            for (mid, m_offset) in m_layout {
                layout.insert(mid, m_offset + shift_m);
            }
        }
        (Some(p_occ), None) | (None, Some(p_occ)) => {
            let p_layout = layout_ancestor_occs(
                p_occ,
                occs,
                occ_index,
                relations,
                data,
                widths,
                gap,
                unmarried_extra,
            );
            for (id, offset) in p_layout {
                layout.insert(id, offset);
            }
        }
        (None, None) => {}
    }
    layout
}

/// Partner-Verbindung zweier Personen für den Kartenabstand: dieselbe Person
/// oder gemeinsame Elternschaft (Familie mit beiden als Eltern — unabhängig
/// vom Traueintrag, der im Import leer ist). Solche Karten rücken eng
/// zusammen; alle anderen Nachbarn bekommen das Nicht-Partner-Extra.
fn persons_are_partners(left: &str, right: &str, data: &TreeData) -> bool {
    if left == right {
        return true;
    }
    data.families.iter().any(|family| {
        let (first, second) = (family.parent_a.as_deref(), family.parent_b.as_deref());
        (first == Some(left) && second == Some(right))
            || (first == Some(right) && second == Some(left))
    })
}

/// Partner-Verbindung zweier Vorkommen (über ihre Personen).
fn occs_are_partners<'a>(
    left: &str,
    right: &str,
    occs: &[AncestorOcc<'a>],
    occ_index: &HashMap<&str, usize>,
    data: &TreeData,
) -> bool {
    match (occ_index.get(left), occ_index.get(right)) {
        (Some(&li), Some(&ri)) => persons_are_partners(
            occs[li].person.id.as_str(),
            occs[ri].person.id.as_str(),
            data,
        ),
        _ => false,
    }
}

/// Kantenabstand zweier Nachbar-Vorkommen: mit Partner-Verbindung
/// (`persons_are_partners`) eng (`gap`), alle anderen Nachbarn mit
/// Extra-Abstand für Nicht-Partner.
fn occ_neighbor_gap<'a>(
    left: &str,
    right: &str,
    occs: &[AncestorOcc<'a>],
    occ_index: &HashMap<&str, usize>,
    data: &TreeData,
    gap: f32,
    unmarried_extra: f32,
) -> f32 {
    if occs_are_partners(left, right, occs, occ_index, data) {
        return gap;
    }
    gap + unmarried_extra
}
/// Effektive Ziehversätze je Vorkommen im Vorfahrenbaum:
/// 1. Basis = manueller Versatz der Person (gilt für alle ihre Vorkommen),
/// 2. Aufwärtspass = Vorfahren folgen dem gezogenen Kind starr 1:1,
/// 3. Abwärtspass = Nachfahren gleichen aus und bleiben mittig — jedes Kind
///    übernimmt den Mittelwert der fertigen Elternversätze (Vater gezogen,
///    Mutter fest → Kind halb). Der Partner bleibt stehen (eigener Versatz 0
///    und keine Ausgleichsvererbung seitlich).
fn ancestor_drag_offsets<'a>(
    occs: &[AncestorOcc<'a>],
    manual_offsets: &HashMap<String, f32>,
) -> HashMap<String, f32> {
    let mut eff: HashMap<String, f32> = HashMap::new();
    for occ in occs {
        let manual = manual_offsets
            .get(occ.person.id.as_str())
            .copied()
            .unwrap_or(0.0);
        if manual != 0.0 {
            *eff.entry(occ.occ_id.clone()).or_insert(0.0) += manual;
        }
    }
    // Aufwärtspass in BFS-Reihenfolge (Kinder stehen vor ihren Eltern).
    for occ in occs {
        let val = eff.get(&occ.occ_id).copied().unwrap_or(0.0);
        if val != 0.0 {
            if let Some(ref father) = occ.father_occ {
                *eff.entry(father.clone()).or_insert(0.0) += val;
            }
            if let Some(ref mother) = occ.mother_occ {
                *eff.entry(mother.clone()).or_insert(0.0) += val;
            }
        }
    }
    // Abwärtspass in umgekehrter Reihenfolge (Eltern vor ihren Kindern):
    // Vorkommen ohne Elternslots (Wurzel, Duplikate ohne Eltern) behalten
    // ihren Aufwärtswert, alle anderen zentrieren sich neu.
    for occ in occs.iter().rev() {
        let mut sum = 0.0f32;
        let mut count = 0u32;
        for parent in occ.father_occ.iter().chain(occ.mother_occ.iter()) {
            sum += eff.get(parent).copied().unwrap_or(0.0);
            count += 1;
        }
        if count > 0 {
            eff.insert(occ.occ_id.clone(), sum / count as f32);
        }
    }
    eff
}

/// Kollisions-Blockade für den Vorfahrenbaum (Occurrence-Pfad): Nach allen
/// Ziehversätzen wird je Ebene geprüft, ob mitbewegte Vorkommen (gezogene
/// Person + starr folgende Vorfahren + mittig ausgleichende Nachfahren) in
/// fest stehende Nachbarkarten ragen würden. Statt die Nachbarn zu
/// verschieben, wird der manuelle Versatz der Ziehperson auf die erlaubte
/// Spanne geklemmt — die Karte bleibt stehen, sobald sie selbst, eines ihrer
/// Kinder oder ein Elternteil anstoßen würde. Mindestmaß ist `MIN_CARD_GAP`.
/// Sortiert wird nach der Basislage (ohne Versatz), damit die Nachbarschaft
/// auch bei großen Sprüngen stabil bleibt.
fn block_ancestor_occ_drag<'a>(
    spread: &mut HashMap<String, f32>,
    rows: &[Vec<String>],
    occs: &[AncestorOcc<'a>],
    occ_index: &HashMap<&str, usize>,
    widths: &HashMap<&str, f32>,
    manual_offsets: &mut HashMap<String, f32>,
    drag_person: &str,
) {
    let current = manual_offsets.get(drag_person).copied().unwrap_or(0.0);
    if current == 0.0 {
        return;
    }
    // Bewegungs-Steigung je Vorkommen: effektive Versätze aktuell und mit
    // simuliertem +1.0-Offset an der Ziehperson (`ancestor_drag_offsets` ist
    // linear, die Differenz ist exakt).
    let base_eff = ancestor_drag_offsets(occs, manual_offsets);
    let mut bumped = manual_offsets.clone();
    bumped.insert(drag_person.to_string(), current + 1.0);
    let bumped_eff = ancestor_drag_offsets(occs, &bumped);
    let mut scale: HashMap<&str, f32> = HashMap::new();
    for occ in occs {
        let shift = bumped_eff.get(&occ.occ_id).copied().unwrap_or(0.0)
            - base_eff.get(&occ.occ_id).copied().unwrap_or(0.0);
        if shift.abs() > 1e-4 {
            scale.insert(occ.occ_id.as_str(), shift);
        }
    }
    if scale.is_empty() {
        return;
    }
    let width_of = |occ_id: &str| -> f32 {
        occ_index
            .get(occ_id)
            .map(|&idx| {
                widths
                    .get(occs[idx].person.id.as_str())
                    .copied()
                    .unwrap_or(215.0)
            })
            .unwrap_or(215.0)
    };
    let mut lo = f32::NEG_INFINITY;
    let mut hi = f32::INFINITY;
    for ids in rows {
        // (Vorkommen, Mitte aktuell, Mitte Basis, Breite).
        let mut cards: Vec<(&str, f32, f32, f32)> = Vec::new();
        for id in ids {
            if let Some(&x) = spread.get(id) {
                let base = x - base_eff.get(id).copied().unwrap_or(0.0);
                cards.push((id.as_str(), x, base, width_of(id)));
            }
        }
        cards.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.3.total_cmp(&b.3)));
        for (index, &(id, x, _, w)) in cards.iter().enumerate() {
            let Some(&s) = scale.get(id) else {
                continue;
            };
            if s <= 0.0 {
                continue;
            }
            // Linker Nachbar fest? Dann gilt: x - w/2 >= nbx + nbw/2 + MIN_CARD_GAP.
            if index > 0 {
                let (nb, nbx, _, nbw) = cards[index - 1];
                if !scale.contains_key(nb) {
                    let cand = (nbx + nbw / 2.0 + MIN_CARD_GAP + w / 2.0 - x) / s + current;
                    lo = lo.max(cand);
                }
            }
            // Rechter Nachbar fest? Dann gilt: x + w/2 <= nbx - nbw/2 - MIN_CARD_GAP.
            if index + 1 < cards.len() {
                let (nb, nbx, _, nbw) = cards[index + 1];
                if !scale.contains_key(nb) {
                    let cand = (nbx - nbw / 2.0 - MIN_CARD_GAP - w / 2.0 - x) / s + current;
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
    for (id, s) in &scale {
        if let Some(value) = spread.get_mut(*id) {
            *value += delta * s;
        }
    }
    if let Some(offset) = manual_offsets.get_mut(drag_person) {
        *offset = target;
    }
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
    card_rects: &mut Vec<(String, Rect)>,
    generation_limit: usize,
    person_limit: usize,
    more_people_available: &mut bool,
    layout_gap: f32,
    non_partner_gap: f32,
    compact_width: f32,
    portrait_width: f32,
    manual_offsets: &mut HashMap<String, f32>,
    media_base: &std::path::Path,
    photo_cache: &mut HashMap<String, TextureHandle>,
    view: TreeView,
    orientation: TreeOrientation,
    card_layout: CardLayout,
    birth_symbol: &str,
    death_symbol: &str,
    zoom: f32,
    pan: Vec2,
    drag_started: bool,
    drag_ended: bool,
    log_layout: bool,
    // true bei offenem Modal: Alle Pointer-Aktionen auf dem Baum sind
    // dann gesperrt (roher Pointer-State umgeht egui-Layer).
    input_blocked: bool,
    // Mehrfachauswahl (Strg+Klick, in Auswahlreihenfolge): nur Anzeige —
    // Aktionen laufen über TreeAction::ToggleMulti.
    multi: &[String],
    // Zoom-Faktoren fürs Umschalten auf die Ganzfoto-Ansicht (Einstellungen):
    // mit Foto / ohne Foto (Initialen).
    photo_full_zoom: f32,
    initials_full_zoom: f32,
) -> Rect {
    // Treffer-Rechtecke für Datei-Drops auf Personenkarten (siehe mod.rs).
    card_rects.clear();
    let relations = TreeRelations::new(data);
    let root: &str = match reference.filter(|id| relations.find(id).is_some()) {
        Some(id) => id,
        None => match data.people.first() {
            Some(p) => p.id.as_str(),
            None => return Rect::ZERO,
        },
    };

    let card_h = card_layout.height();
    let _couple_gap = if orientation == TreeOrientation::Horizontal {
        0.0f32
    } else {
        4.0f32
    };
    let gap = layout_gap;
    let gen_gap = 36.0f32;

    if view == TreeView::Ancestors {
        let automatic_person_limit = (generation_limit == 0).then_some(person_limit);
        let (occs, limited) = collect_ancestor_occs(
            root,
            &relations,
            generation_limit,
            expanded,
            automatic_person_limit,
        );
        *more_people_available = limited;
        if occs.is_empty() {
            return Rect::ZERO;
        }

        let occ_index: HashMap<&str, usize> = occs
            .iter()
            .enumerate()
            .map(|(idx, occ)| (occ.occ_id.as_str(), idx))
            .collect();
        let mut occs_by_person: HashMap<&str, Vec<usize>> = HashMap::new();
        for (idx, occ) in occs.iter().enumerate() {
            occs_by_person
                .entry(occ.person.id.as_str())
                .or_default()
                .push(idx);
        }
        let occ_person_levels: HashSet<(&str, usize)> = occs
            .iter()
            .map(|occ| (occ.person.id.as_str(), occ.level))
            .collect();

        let max_level = occs.iter().map(|o| o.level).max().unwrap_or(0);
        let mut rows: Vec<Vec<String>> = vec![Vec::new(); max_level + 1];
        for occ in &occs {
            rows[occ.level].push(occ.occ_id.clone());
        }
        
        let fixed_w = card_fixed_width(card_layout, compact_width, portrait_width);
        let widths: HashMap<&str, f32> = occs
            .iter()
            .map(|occ| (occ.person.id.as_str(), fixed_w))
            .collect();

        let row_widths: Vec<f32> = rows
            .iter()
            .map(|ids| {
                let mut width = fixed_w;
                for id in ids {
                    let occ = &occs[occ_index[id.as_str()]];
                    width = width.max(widths[occ.person.id.as_str()]);
                }
                width
            })
            .collect();

        let card_w = |row: usize, person_id: &str| -> f32 {
            if orientation == TreeOrientation::Horizontal {
                row_widths[row]
            } else {
                widths.get(person_id).copied().unwrap_or(215.0)
            }
        };
            
        let mut col_x: Vec<f32> = Vec::with_capacity(rows.len());
        let mut acc = 0.0f32;
        for (row, _) in rows.iter().enumerate() {
            if row > 0 {
                acc += row_widths[row - 1] + gen_gap;
            }
            col_x.push(acc);
        }
        
        let root_occ_id = occs[0].occ_id.clone();
        let mut spread = layout_ancestor_occs(
            &root_occ_id,
            &occs,
            &occ_index,
            &relations,
            data,
            &widths,
            gap,
            non_partner_gap,
        );
        
        // Verheiratete Elternpaare (Vorkommen mit gemeinsamem Kind) rücken
        // eng zusammen; alle anderen Nachbarn — auch Duplikate neben
        // fremden Karten — bekommen den Extra-Abstand für Nicht-Partner.
        // Dasselbe Personen-Vorkommen neben sich selbst bleibt eng.
        let pair_gap = |left: &str, right: &str| -> f32 {
            occ_neighbor_gap(left, right, &occs, &occ_index, data, gap, non_partner_gap)
        };

        for ids in &rows {
            let missing: Vec<&String> = ids.iter().filter(|id| !spread.contains_key(*id)).collect();
            if !missing.is_empty() {
                // Anker: rechtseste bereits platzierte Karte derselben Ebene.
                let anchor: Option<&String> = ids
                    .iter()
                    .filter(|id| spread.contains_key(*id))
                    .max_by(|a, b| {
                        let left = spread.get(*a).copied().unwrap_or(0.0);
                        let right = spread.get(*b).copied().unwrap_or(0.0);
                        left.total_cmp(&right)
                    });
                let mut cursor = match anchor {
                    Some(anchor_id) => {
                        let occ = &occs[occ_index[anchor_id.as_str()]];
                        let right = spread.get(anchor_id).copied().unwrap_or(0.0)
                            + widths.get(occ.person.id.as_str()).copied().unwrap_or(215.0) / 2.0;
                        right + pair_gap(anchor_id, missing[0])
                    }
                    None => {
                        let mut total = 0.0f32;
                        for (index, id) in missing.iter().enumerate() {
                            let occ = &occs[occ_index[id.as_str()]];
                            total += widths.get(occ.person.id.as_str()).copied().unwrap_or(215.0);
                            if let Some(next) = missing.get(index + 1) {
                                total += pair_gap(id, next);
                            }
                        }
                        -total / 2.0
                    }
                };
                for (index, id) in missing.iter().enumerate() {
                    let occ = &occs[occ_index[id.as_str()]];
                    let w = widths.get(occ.person.id.as_str()).copied().unwrap_or(215.0);
                    spread.insert((*id).clone(), cursor + w / 2.0);
                    cursor += w;
                    if let Some(next) = missing.get(index + 1) {
                        cursor += pair_gap(id, next);
                    }
                }
            }
        }
        
        let eff_occs = ancestor_drag_offsets(&occs, manual_offsets);
        
        for (occ_id, &val) in &eff_occs {
            if let Some(spread_val) = spread.get_mut(occ_id) {
                *spread_val += val;
            }
        }

        // Kollisions-Blockade: Mitbewegtes (Person, Kinder, Eltern) darf nicht
        // in fest stehende Nachbarkarten ragen — sonst bleibt die Ziehperson
        // an der Grenze stehen.
        if let Some((drag_person, _)) = card_drag.as_ref() {
            block_ancestor_occ_drag(
                &mut spread,
                &rows,
                &occs,
                &occ_index,
                &widths,
                manual_offsets,
                drag_person,
            );
        }
        
        let mut positions: HashMap<String, (f32, f32)> = HashMap::new();
        for occ in &occs {
            if let Some(&s) = spread.get(&occ.occ_id) {
                let y = occ.level as f32 * (card_h + 40.0) * -1.0;
                if orientation == TreeOrientation::Vertical {
                    positions.insert(occ.occ_id.clone(), (s, y));
                } else {
                    let x = (col_x[occ.level] + widths.get(occ.person.id.as_str()).copied().unwrap_or(215.0) / 2.0) * -1.0;
                    positions.insert(occ.occ_id.clone(), (x, s));
                }
            }
        }
        
        let center = rect.center() + pan;
        let viewport = painter.clip_rect().expand(32.0);
        let line_on_screen = |a: Pos2, b: Pos2| {
            viewport.intersects(Rect::from_two_pos(a, b).expand(4.0))
        };
        
        for occ in &occs {
            let pa = occ.father_occ.as_ref().and_then(|id| positions.get(id).copied()).map(|(x, y)| center + Vec2::new(x * zoom, y * zoom));
            let pb = occ.mother_occ.as_ref().and_then(|id| positions.get(id).copied()).map(|(x, y)| center + Vec2::new(x * zoom, y * zoom));
            let child_pos = positions.get(&occ.occ_id).copied().map(|(x, y)| center + Vec2::new(x * zoom, y * zoom));
            
            if pa.is_some() || pb.is_some() {
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
                        let id_a = occ.father_occ.as_ref().unwrap();
                        let id_b = occ.mother_occ.as_ref().unwrap();
                        let parent_a = &occs[occ_index[id_a.as_str()]];
                        let parent_b = &occs[occ_index[id_b.as_str()]];
                        let p_a = parent_a.person.id.as_str();
                        let p_b = parent_b.person.id.as_str();
                        let wa = card_w(parent_a.level, p_a);
                        let wb = card_w(parent_b.level, p_b);
                        
                        let (left_x, left_w, right_x, right_w) = if a.x < b.x {
                            (a.x, wa, b.x, wb)
                        } else {
                            (b.x, wb, a.x, wa)
                        };
                        let jx = (left_x + left_w * zoom / 2.0 + right_x - right_w * zoom / 2.0) / 2.0;
                        Pos2::new(jx, (a.y + b.y) / 2.0)
                    }
                    (Some(a), None) | (None, Some(a)) => a,
                    (None, None) => unreachable!(),
                };
                
                if let Some(c) = child_pos {
                    if line_on_screen(junction, c) {
                        painter.line_segment(
                            [junction, c],
                            Stroke::new(2.0, Color32::from_rgb(74, 111, 119)),
                        );
                    }
                }
            }
        }
        
        for occ in &occs {
            let Some(&(x, y)) = positions.get(&occ.occ_id) else { continue; };
            let at = center + Vec2::new(x * zoom, y * zoom);
            let width = widths.get(occ.person.id.as_str()).copied().unwrap_or(215.0);
            let card = Rect::from_center_size(at, Vec2::new(width, card_h) * zoom);
            let card_on_screen = viewport.intersects(card);
            
            let card_clicked = draw_person_card(
                painter,
                occ.person,
                at,
                width,
                card_layout,
                birth_symbol,
                death_symbol,
                zoom,
                viewed == Some(occ.person.id.as_str()),
                false,
                media_base,
                photo_cache,
                card_rects,
                input_blocked,
                multi,
                photo_full_zoom,
                initials_full_zoom,
            );
            
            let empty_occs: Vec<usize> = Vec::new();
            let person_occs: &Vec<usize> = occs_by_person
                .get(occ.person.id.as_str())
                .unwrap_or(&empty_occs);
            let mut arrow_clicked = false;
            if card_on_screen && person_occs.len() > 1 {
                let occ_idx = occ_index[occ.occ_id.as_str()];
                let current_idx = person_occs
                    .iter()
                    .position(|&idx| idx == occ_idx)
                    .unwrap_or(0);
                let arrow_left_center = card.right_top() + Vec2::new(-28.0 * zoom, 12.0 * zoom);
                let arrow_right_center = card.right_top() + Vec2::new(-10.0 * zoom, 12.0 * zoom);
                let arrow_r = 7.0 * zoom;
                let arrow_left_rect = Rect::from_center_size(arrow_left_center, Vec2::splat(arrow_r * 2.0));
                let arrow_right_rect = Rect::from_center_size(arrow_right_center, Vec2::splat(arrow_r * 2.0));
                
                painter.circle_filled(arrow_left_center, arrow_r, Color32::from_rgb(24, 40, 48));
                painter.circle_stroke(arrow_left_center, arrow_r, Stroke::new(1.0, Color32::from_rgb(120, 170, 160)));
                painter.line_segment(
                    [arrow_left_center + Vec2::new(1.5 * zoom, -2.5 * zoom), arrow_left_center + Vec2::new(-1.5 * zoom, 0.0)],
                    Stroke::new(1.5, Color32::from_rgb(158, 213, 199))
                );
                painter.line_segment(
                    [arrow_left_center + Vec2::new(-1.5 * zoom, 0.0), arrow_left_center + Vec2::new(1.5 * zoom, 2.5 * zoom)],
                    Stroke::new(1.5, Color32::from_rgb(158, 213, 199))
                );
                
                painter.circle_filled(arrow_right_center, arrow_r, Color32::from_rgb(24, 40, 48));
                painter.circle_stroke(arrow_right_center, arrow_r, Stroke::new(1.0, Color32::from_rgb(120, 170, 160)));
                painter.line_segment(
                    [arrow_right_center + Vec2::new(-1.5 * zoom, -2.5 * zoom), arrow_right_center + Vec2::new(1.5 * zoom, 0.0)],
                    Stroke::new(1.5, Color32::from_rgb(158, 213, 199))
                );
                painter.line_segment(
                    [arrow_right_center + Vec2::new(1.5 * zoom, 0.0), arrow_right_center + Vec2::new(-1.5 * zoom, 2.5 * zoom)],
                    Stroke::new(1.5, Color32::from_rgb(158, 213, 199))
                );
                
                let left_clicked = !input_blocked
                    && painter.ctx().input(|i| {
                        i.pointer.any_click()
                            && i.pointer.interact_pos().is_some_and(|q| arrow_left_rect.contains(q) && painter.clip_rect().contains(q))
                    });
                let right_clicked = !input_blocked
                    && painter.ctx().input(|i| {
                        i.pointer.any_click()
                            && i.pointer.interact_pos().is_some_and(|q| arrow_right_rect.contains(q) && painter.clip_rect().contains(q))
                    });
                
                if left_clicked {
                    arrow_clicked = true;
                    let prev_idx = (current_idx + person_occs.len() - 1) % person_occs.len();
                    let prev_occ = &occs[person_occs[prev_idx]];
                    if let Some(&(tx, ty)) = positions.get(&prev_occ.occ_id) {
                        *action = Some(TreeAction::PanTo(Vec2::new(tx, ty)));
                    }
                } else if right_clicked {
                    arrow_clicked = true;
                    let next_idx = (current_idx + 1) % person_occs.len();
                    let next_occ = &occs[person_occs[next_idx]];
                    if let Some(&(tx, ty)) = positions.get(&next_occ.occ_id) {
                        *action = Some(TreeAction::PanTo(Vec2::new(tx, ty)));
                    }
                }
            }
            
            let mut badge_clicked = false;
            let has_more = {
                let relatives = relations.parents_of(&occ.person.id);
                relatives.iter().any(|rel| {
                    !occ_person_levels.contains(&(rel.id.as_str(), occ.level + 1))
                })
            };
            
            if occ.is_canonical {
                let badge_at = if orientation == TreeOrientation::Vertical {
                    Pos2::new(card.center().x, card.top() - 13.0 * zoom)
                } else {
                    Pos2::new(card.left() - 13.0 * zoom, card.center().y)
                };
                let badge_r = 9.0 * zoom;
                let badge_rect = Rect::from_center_size(badge_at, Vec2::splat(badge_r * 2.0));
                let pointer_pos = if input_blocked {
                    None
                } else {
                    painter.ctx().input(|i| i.pointer.interact_pos())
                };
                let canvas = painter.clip_rect();
                let card_hovered = pointer_pos
                    .is_some_and(|q| (card.contains(q) || badge_rect.contains(q)) && canvas.contains(q));
                
                if card_on_screen
                    && card_hovered
                    && (has_more || expanded.contains(&occ.person.id))
                {
                    painter.circle_filled(badge_at, badge_r, Color32::from_rgb(24, 40, 48));
                    painter.circle_stroke(badge_at, badge_r, Stroke::new(1.0, Color32::from_rgb(120, 170, 160)));
                    
                    let arrow_down = expanded.contains(&occ.person.id);
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
                    badge_clicked = !input_blocked
                        && painter.ctx().input(|i| {
                            i.pointer.any_click()
                                && i.pointer.interact_pos().is_some_and(|q| badge_rect.contains(q) && painter.clip_rect().contains(q))
                        });
                    if badge_clicked {
                        *action = Some(TreeAction::ToggleExpand(occ.person.id.clone()));
                    }
                }
            }
            
            if !badge_clicked && !arrow_clicked && card_clicked {
                let shift = painter.ctx().input(|i| i.modifiers.shift);
                let ctrl = painter.ctx().input(|i| i.modifiers.ctrl);
                *action = Some(if ctrl {
                    TreeAction::ToggleMulti(occ.person.id.clone())
                } else if shift {
                    TreeAction::Reference(occ.person.id.clone())
                } else {
                    TreeAction::View(occ.person.id.clone())
                });
            }
            
            if !badge_clicked && !arrow_clicked {
                let long_pressed = !input_blocked
                    && card_on_screen
                    && painter.ctx().input(|i| {
                        i.any_touches()
                        && i.pointer.press_origin().is_some_and(|q| {
                            card.contains(q) && painter.clip_rect().contains(q)
                        })
                        && i.pointer.press_start_time().is_some_and(|t0| i.time - t0 >= 0.6)
                    });
                if long_pressed && !*long_press_used {
                    *action = Some(TreeAction::Reference(occ.person.id.clone()));
                    *long_press_used = true;
                }
            }
            
            if card_on_screen && !badge_clicked && !arrow_clicked && !*long_press_used {
                let outer = card.expand(4.0 * zoom);
                // Nur das erste Vorkommen einer Person wendet den Versatz an —
                // alle Duplikate teilen sich dieselbe Personen-ID und würden
                // sonst mehrfach ziehen.
                let is_first_occ = occs
                    .iter()
                    .filter(|other| other.person.id == occ.person.id)
                    .next()
                    .is_some_and(|first| first.occ_id == occ.occ_id);
                let (occ_delta, occ_start) = painter.ctx().input(|i| {
                    if input_blocked {
                        return (0.0, false);
                    }
                    let is_active = card_drag
                        .as_ref()
                        .is_some_and(|(id, _)| id == occ.person.id.as_str());
                    if !card_on_screen && !is_active {
                        return (0.0, false);
                    }
                    if !(i.pointer.primary_down() && i.modifiers.shift) {
                        return (0.0, false);
                    }
                    let pressed_here = i.pointer.press_origin().is_some_and(|q| {
                        outer.contains(q) && painter.clip_rect().contains(q)
                    });
                    if !(pressed_here || (is_active && is_first_occ)) {
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
                if occ_delta != 0.0 || (occ_start && card_drag.is_none()) {
                    let layout_delta = occ_delta / zoom;
                    if let Some((_, members)) = card_drag {
                        for id in members.clone() {
                            *manual_offsets.entry(id.clone()).or_insert(0.0) += layout_delta;
                        }
                    } else {
                        // Nur die gezogene Person trägt den Versatz; die Vererbung
                        // (Vorfahren starr, Nachfahren mittig) rechnet
                        // `ancestor_drag_offsets` weiter oben aus.
                        let move_set_vec = vec![occ.person.id.clone()];
                        for id in &move_set_vec {
                            *manual_offsets.entry(id.clone()).or_insert(0.0) += layout_delta;
                        }
                        *card_drag = Some((occ.person.id.clone(), move_set_vec));
                    }
                    *frame_drag = true;
                }
            }
        }
        
        let content_bounds = {
            let mut min = Pos2::new(f32::INFINITY, f32::INFINITY);
            let mut max = Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
            for (id, &(x, y)) in positions.iter() {
                let occ = &occs[occ_index[id.as_str()]];
                let half = widths.get(occ.person.id.as_str()).copied().unwrap_or(215.0) / 2.0;
                min = min.min(Pos2::new(x - half, y - card_h / 2.0));
                max = max.max(Pos2::new(x + half, y + card_h / 2.0));
            }
            if min.x.is_finite() {
                Rect::from_min_max(min, max)
            } else {
                Rect::ZERO
            }
        };
        
        return content_bounds;
    }

    let ancestors = view != TreeView::Descendants;
    // Ohne Generationenlimit begrenzt die Vorfahrenansicht die echte BFS
    // deterministisch auf das aktuelle Personenbudget.
    let automatic_person_limit =
        (view == TreeView::Ancestors && generation_limit == 0).then_some(person_limit);
    let (levels, limited) = collect_visible_levels(
        root,
        &relations,
        ancestors,
        generation_limit,
        expanded,
        automatic_person_limit,
    );
    *more_people_available = limited;
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
    let fixed_w = card_fixed_width(card_layout, compact_width, portrait_width);
    let widths: HashMap<&str, f32> = levels
        .keys()
        .copied()
        .chain(shown_partners.iter().copied())
        .filter_map(|id| relations.find(id))
        .map(|person| (person.id.as_str(), fixed_w))
        .collect();
    let row_widths: Vec<f32> = rows
        .iter()
        .map(|ids| {
            // Fixe Basisbreite je Kartenlayout (Einstellungen); sonst würden
            // im Horizontalen alle Karten unterschiedlich breit.
            let mut width = fixed_w;
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
    // Für den vertikalen Nachfahrenbaum ersetzt die neue deterministische
    // Einpass-Packung die zwölfmalige Anziehungs-/Abstoßungs-Verhandlung.
    let tidy_vertical_descendants =
        view == TreeView::Descendants && orientation == TreeOrientation::Vertical;
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
    // Vorfahren kehren im Occurrence-Zweig weiter oben per Return zurück;
    // hier folgen nur Nachfahren und Fächer.
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
    let negotiation_rounds = if tidy_vertical_descendants { 0 } else { 12 };
    for _ in 0..negotiation_rounds {
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
                if log_layout && delta.abs() > 500.0 {
                    log::debug!(
                        "NEG_ATTR_CHILD family={} delta={:.0} junction={:.0} mean={:.0} n={}",
                        family.id,
                        delta,
                        junction,
                        mean,
                        children.len()
                    );
                }
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
            if log_layout && delta.abs() > 500.0 {
                log::debug!(
                    "NEG_ATTR_PARENT family={} delta={:.0} target={:.0} parent_mean={:.0}",
                    family.id,
                    delta,
                    target,
                    parent_mean
                );
            }
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
    if tidy_vertical_descendants {
        tidy_descendant_spread_vertical(
            root,
            &mut spread,
            &rows,
            &relations,
            data,
            &levels,
            &widths,
            &group_of,
            &shown_partners,
            gap,
            sibling_container_padding,
            couple_gap,
        );
        dump_stage("TIDY", &spread);
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
    if view != TreeView::Ancestors && !tidy_vertical_descendants {
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
    if view != TreeView::Ancestors && !tidy_vertical_descendants {
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
                let frame_hit = !input_blocked
                    && painter.ctx().input(|i| {
                        i.pointer.primary_down()
                            && i.modifiers.shift
                            && i.pointer.press_origin().is_some_and(|q| {
                                outer.contains(q) && painter.clip_rect().contains(q)
                            })
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
            // Shift+Ziehen auf dem HINTERGRUND des Geschwister-Containers:
            // die ganze Geschwistergruppe folgt (alle Kinder; Nachfahren
            // erben die Versätze wie beim Karten-Drag). Die Drag-Wurzel ist
            // ein Marker (keine Personen-ID), damit die Kartenschleife den
            // Versatz nicht ein zweites Mal anwendet.
            if let Some(container_rect) = container {
                let marker = format!("container:{}", family.id);
                let is_active = card_drag.as_ref().is_some_and(|(id, _)| *id == marker);
                if viewport.intersects(container_rect) {
                    let (container_delta, container_start) = painter.ctx().input(|i| {
                        if input_blocked {
                            return (0.0, false);
                        }
                        if !(i.pointer.primary_down() && i.modifiers.shift) {
                            return (0.0, false);
                        }
                        let pressed_here = i.pointer.press_origin().is_some_and(|q| {
                            container_rect.contains(q)
                                && painter.clip_rect().contains(q)
                                && !blocks.iter().any(|block| block.contains(q))
                        });
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
                    if container_delta != 0.0 || (container_start && card_drag.is_none()) {
                        let layout_delta = container_delta / zoom;
                        if let Some((_, members)) = card_drag {
                            for id in members.clone() {
                                *manual_offsets.entry(id.clone()).or_insert(0.0) += layout_delta;
                            }
                        } else {
                            let members: Vec<String> =
                                block_children.iter().map(|id| (*id).to_string()).collect();
                            for id in &members {
                                *manual_offsets.entry(id.clone()).or_insert(0.0) += layout_delta;
                            }
                            *card_drag = Some((marker, members));
                        }
                        *frame_drag = true;
                    }
                }
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
            birth_symbol,
            death_symbol,
            zoom,
            viewed == Some(person.id.as_str()),
            false,
            media_base,
            photo_cache,
            card_rects,
            input_blocked,
            multi,
            photo_full_zoom,
            initials_full_zoom,
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
        let pointer_pos = if input_blocked {
            None
        } else {
            painter.ctx().input(|i| i.pointer.interact_pos())
        };
        let canvas = painter.clip_rect();
        let card_hovered = pointer_pos
            .is_some_and(|q| (card.contains(q) || badge_rect.contains(q)) && canvas.contains(q));
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
            badge_clicked = !input_blocked
                && painter.ctx().input(|i| {
                    i.pointer.any_click()
                        && i.pointer
                            .interact_pos()
                            .is_some_and(|q| badge_rect.contains(q) && painter.clip_rect().contains(q))
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
                draw_up_badge(painter, card, zoom, orientation, action, &parent.id, input_blocked);
            }
        }
        if !badge_clicked && card_clicked {
            let shift = painter.ctx().input(|i| i.modifiers.shift);
            let ctrl = painter.ctx().input(|i| i.modifiers.ctrl);
            *action = Some(if ctrl {
                TreeAction::ToggleMulti(person.id.clone())
            } else if shift {
                TreeAction::Reference(person.id.clone())
            } else {
                TreeAction::View(person.id.clone())
            });
        }
        if !badge_clicked {
            // Langer Touch (≥ 0,6 s) setzt die Referenzperson – NUR bei
            // echtem Touch (`any_touches`), nicht bei gedrückter Maustaste;
            // pro Drücken nur einmal (`long_press_used`, Reset in `ui`).
            let long_pressed = !input_blocked
                && card_on_screen
                && painter.ctx().input(|i| {
                    i.any_touches()
                    && i.pointer.press_origin().is_some_and(|q| {
                        card.contains(q) && painter.clip_rect().contains(q)
                    })
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
            if input_blocked {
                return (0.0, false);
            }
            let is_active = card_drag
                .as_ref()
                .is_some_and(|(id, _)| id == person.id.as_str());
            if !card_on_screen && !is_active {
                return (0.0, false);
            }
            if !(i.pointer.primary_down() && i.modifiers.shift) {
                return (0.0, false);
            }
            let pressed_here = i
                .pointer
                .press_origin()
                .is_some_and(|q| card.contains(q) && painter.clip_rect().contains(q));
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
                birth_symbol,
                death_symbol,
                zoom,
                viewed == Some(partner.id.as_str()),
                true,
                media_base,
                photo_cache,
                card_rects,
                input_blocked,
                multi,
                photo_full_zoom,
                initials_full_zoom,
            );
            let partner_active = card_drag
                .as_ref()
                .is_some_and(|(id, _)| id == partner.id.as_str());
            if !partner_on_screen && !partner_active {
                continue;
            }
            let partner_long = !input_blocked
                && painter.ctx().input(|i| {
                    i.any_touches()
                        && i.pointer
                            .press_origin()
                            .is_some_and(|q| partner_card.contains(q) && painter.clip_rect().contains(q))
                        && i.pointer
                            .press_start_time()
                            .is_some_and(|t0| i.time - t0 >= 0.6)
                });
            if partner_clicked {
                let shift = painter.ctx().input(|i| i.modifiers.shift);
                let ctrl = painter.ctx().input(|i| i.modifiers.ctrl);
                *action = Some(if ctrl {
                    TreeAction::ToggleMulti(partner.id.clone())
                } else if shift {
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
                    draw_up_badge(painter, partner_card, zoom, orientation, action, &parent.id, input_blocked);
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
                    // Losgelassen: Gestus-Latches räumen (läuft auch bei
                    // offenem Modal, damit kein Zustand hängen bleibt).
                    *swap_latch = false;
                    if card_drag
                        .as_ref()
                        .is_some_and(|(id, _)| id == partner.id.as_str())
                    {
                        *card_drag = None;
                    }
                    return 0.0;
                }
                // Offenes Modal: kein Tausch-Drag starten/fortsetzen.
                if input_blocked {
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
                    .is_some_and(|q| partner_card.contains(q) && painter.clip_rect().contains(q));
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
        // Familien strikt per Schlüssel gruppieren (NICHT nach benachbarter
        // x-Position): so bleiben Geschwister immer ein starrer Block und
        // können im Repel nicht auseinandergerissen werden.
        let mut groups_map: HashMap<&str, Vec<(&str, f32)>> = HashMap::new();
        for member in members {
            groups_map
                .entry(group_key(group_of, member.0))
                .or_default()
                .push(member);
        }
        let mut groups: Vec<Vec<(&str, f32)>> = groups_map.into_values().collect();
        // Deterministische Reihenfolge: Die `HashMap`-Iteration ändert sich pro
        // `HashMap::new()` und würde das Layout sonst Frame für Frame minimal
        // verschieben ("konstantes Wandern"). Nach linker Position + ID sortieren.
        groups.sort_by(|a, b| {
            let left = |group: &Vec<(&str, f32)>| {
                group
                    .iter()
                    .map(|(id, _)| spread[*id])
                    .fold(f32::MAX, f32::min)
            };
            left(a)
                .total_cmp(&left(b))
                .then_with(|| a[0].0.cmp(b[0].0))
        });
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
                        if deficit > 500.0 {
                            log::debug!(
                                "NEG_REPEL_INNER deficit={:.0} need={:.0} actual={:.0} left={} right={}",
                                deficit,
                                need,
                                actual,
                                window[0].0,
                                window[1].0
                            );
                        }
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
                    if deficit > 500.0 {
                        log::debug!(
                            "NEG_REPEL_BIG deficit={:.0} need={:.0} actual={:.0} left=[{}] right=[{}]",
                            deficit,
                            need,
                            actual,
                            window[0].0.join(","),
                            window[1].0.join(",")
                        );
                    }
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
    // true bei offenem Modal: Hover und Klick sind gesperrt.
    input_blocked: bool,
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
    let hovered = !input_blocked
        && painter.ctx().input(|i| {
            i.pointer.interact_pos().is_some_and(|q| {
                (card.contains(q) || badge_rect.contains(q)) && painter.clip_rect().contains(q)
            })
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
    let clicked = !input_blocked
        && painter.ctx().input(|i| {
            i.pointer.any_click()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|q| badge_rect.contains(q) && painter.clip_rect().contains(q))
        });
    if clicked {
        *action = Some(TreeAction::Reference(target_id.to_string()));
    }
}

fn shorten_family(family_name: &str) -> String {
    let words: Vec<&str> = family_name.split_whitespace().collect();
    if words.len() <= 2 {
        family_name.to_string()
    } else {
        words[0..2].join(" ")
    }
}

fn diagram_display_name_short(person: &Person) -> String {
    let shortened_family = shorten_family(&person.family_name);
    let combined = format!("{} {}", person.given_short(), shortened_family);
    let combined = combined.trim().to_string();
    if !combined.is_empty() {
        combined
    } else {
        person.name.clone()
    }
}

/// Fixe Kartenbreite je Kartenlayout (Layout-Einheiten). Die Breite hängt
/// bewusst NICHT vom Namen ab, damit pro Frame keine Textvermessung nötig
/// ist; zu lange Namen werden beim Zeichnen mit „…" gekürzt.
fn card_fixed_width(card_layout: CardLayout, compact_width: f32, portrait_width: f32) -> f32 {
    match card_layout {
        CardLayout::Compact => compact_width.clamp(120.0, 400.0),
        CardLayout::Portrait => portrait_width.clamp(120.0, 400.0),
    }
}

/// Datum mit vorangestelltem Symbol (Einstellung, z. B. Elhaz-Rune);
/// ohne Symbol nur das Datum.
fn symbolized(symbol: &str, date: &str) -> String {
    if symbol.is_empty() {
        date.to_string()
    } else {
        format!("{symbol} {date}")
    }
}

/// Text auf höchstens `max_chars` Zeichen kürzen (mit „…" als letztem
/// Zeichen), damit er in die fixe Kartenbreite passt.
fn ellipsize(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut result: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    result.push('…');
    result
}

/// Eine Personenkarte zeichnen; Rückgabe true bei Klick auf die Karte.
/// `muted` = Partner-Pseudokarte (dezentere Umrandung).
fn draw_person_card(
    painter: &egui::Painter,
    person: &crate::model::Person,
    at: Pos2,
    width: f32,
    card_layout: CardLayout,
    birth_symbol: &str,
    death_symbol: &str,
    zoom: f32,
    selected_now: bool,
    muted: bool,
    media_base: &std::path::Path,
    photo_cache: &mut HashMap<String, TextureHandle>,
    hit_rects: &mut Vec<(String, Rect)>,
    // true bei offenem Modal: Klicks auf die Karte sind gesperrt.
    input_blocked: bool,
    // Mehrfachauswahl (Auswahlreihenfolge): aktive Auswahl (`selected_now`)
    // bleibt grün gefüllt, weitere Ausgewählte nur dick grün umrandet.
    multi: &[String],
    // Zoom-Faktoren fürs Umschalten auf die Ganzfoto-Ansicht (Einstellungen):
    // mit Foto / ohne Foto (Initialen).
    photo_full_zoom: f32,
    initials_full_zoom: f32,
) -> bool {
    let size = Vec2::new(width, card_layout.height()) * zoom;
    let card = Rect::from_center_size(at, size);
    if !painter.clip_rect().expand(32.0).intersects(card) {
        return false;
    }
    hit_rects.push((person.id.clone(), card));
    let multi_now = !selected_now && multi.iter().any(|id| id == &person.id);
    // Ganzfoto-Schwelle je Fotostatus (Einstellungen): Foto einmal holen,
    // Schwelle wählen, dann rendern.
    let photo = photo_card_texture(painter.ctx(), person, photo_cache, media_base);
    let full_zoom = if photo.is_some() {
        photo_full_zoom
    } else {
        initials_full_zoom
    };
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
            if selected_now || multi_now {
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
    if zoom < full_zoom {
        // Statt leerer Farbfläche füllt das Profilbild die Karte, ohne sie zu
        // verzerren: Cover-Beschnitt mit dem KARTEN-Seitenverhältnis, sodass
        // die Box vollständig und ungestreckt gefüllt ist. Ohne Foto stehen
        // dezente Initialen in der Kartenmitte.
        if let Some(texture) = photo {
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
            // Foto-Ecken stehen über den Border-Radius über (eckiger Clip vs.
            // runder Rahmen): Ecken mit Kartenfüllung abdecken — außerhalb der
            // Rundung, auf die Karte geclippt (Diagonale ≈ 0,41 × Radius).
            painter.with_clip_rect(card).rect(
                card,
                10. * zoom,
                Color32::TRANSPARENT,
                Stroke::new((4.5 * zoom).max(1.0), fill),
                egui::StrokeKind::Outside,
            );
            painter.rect(
                card,
                10. * zoom,
                Color32::TRANSPARENT,
                Stroke::new(
                    if selected_now || multi_now {
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
        let canvas = painter.clip_rect();
        if input_blocked {
            return false;
        }
        return painter.ctx().input(|i| {
            i.pointer.any_click()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|q| card.contains(q) && canvas.contains(q))
        });
    }
    let (avatar_size, avatar_center, name_at, name_align, name2_at, birth_at, death_at, birth_align) =
        match card_layout {
            // Geburts- und Sterbezeile stehen untereinander (Kompakt hat
            // dafür Luft, Portrait-Karten wurden dafür höher).
            CardLayout::Compact => (
                48.0 * zoom,
                card.left_center() + Vec2::new(35.0 * zoom, 0.0),
                card.left_top() + Vec2::new(64.0 * zoom, 21.0 * zoom),
                Align2::LEFT_CENTER,
                Pos2::ZERO,
                card.left_top() + Vec2::new(64.0 * zoom, 41.0 * zoom),
                card.left_top() + Vec2::new(64.0 * zoom, 61.0 * zoom),
                Align2::LEFT_CENTER,
            ),
            CardLayout::Portrait => (
                78.0 * zoom,
                card.center_top() + Vec2::new(0.0, 48.0 * zoom),
                card.center_top() + Vec2::new(0.0, 106.0 * zoom),
                Align2::CENTER_CENTER,
                card.center_top() + Vec2::new(0.0, 126.0 * zoom),
                card.center_top() + Vec2::new(0.0, 145.0 * zoom),
                card.center_top() + Vec2::new(0.0, 165.0 * zoom),
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
    // Maximale Zeichenzahl aus der fixen Kartenbreite ableiten (Schrift 13,
    // ca. 6,5 Einheiten je Zeichen): Kompakt-Text beginnt bei x=64,
    // Portrait-Text ist zentriert mit je 8 Einheiten Rand.
    let max_chars = match card_layout {
        CardLayout::Compact => ((width - 72.0) / 6.5).max(8.0) as usize,
        CardLayout::Portrait => ((width - 16.0) / 6.5).max(8.0) as usize,
    };
    match card_layout {
        CardLayout::Compact => {
            painter.text(
                name_at,
                name_align,
                ellipsize(&diagram_display_name_short(person), max_chars),
                FontId::proportional(13. * zoom),
                Color32::WHITE,
            );
        }
        CardLayout::Portrait => {
            painter.text(
                name_at,
                name_align,
                ellipsize(&person.given_short(), max_chars),
                FontId::proportional(13. * zoom),
                Color32::WHITE,
            );
            if !person.family_name.is_empty() {
                painter.text(
                    name2_at,
                    Align2::CENTER_CENTER,
                    ellipsize(&shorten_family(&person.family_name), max_chars),
                    FontId::proportional(13. * zoom),
                    Color32::WHITE,
                );
            }
        }
    }
    painter.text(
        birth_at,
        birth_align,
        ellipsize(
            &symbolized(birth_symbol, &person.birth_short()),
            max_chars,
        ),
        FontId::proportional(12. * zoom),
        Color32::from_rgb(202, 222, 221),
    );
    let death = person.death_short();
    if !death.is_empty() {
        painter.text(
            death_at,
            birth_align,
            ellipsize(&symbolized(death_symbol, &death), max_chars),
            FontId::proportional(12. * zoom),
            Color32::from_rgb(202, 222, 221),
        );
    }
    let canvas = painter.clip_rect();
    !input_blocked
        && painter.ctx().input(|i| {
            i.pointer.any_click()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|q| card.contains(q) && canvas.contains(q))
        })
}

// Vorfahren laufen über `layout_ancestor_occs` (Occurrence-Modell mit
// kanonischer Expansion, manuellen Versätzen und Extra-Abstand für nicht
// verheiratete Paare) — dort liegt die gepflegte Implementierung.
