//! Datenmodell von MiniGramps.
//!
//! Verdrahtung:
//! - `import` erzeugt `TreeData` beim Laden (GEDCOM / Gramps-XML / JSON).
//! - `ui::MiniGramps::data` hält die aktuelle Instanz; das Speichern
//!   (`MiniGramps::save`) serialisiert genau diese Struktur.
//! - `ui` benutzt die Abfragefunktionen (`find`, `children_of`, `parents_of`,
//!   `siblings_of`, `partners_of`) für Seitenleiste/Profil und den
//!   Beziehungspicker (`link_partner`, `link_child`, `link_child_to`).
//! - `ui::tree` traversiert dieselben Abfragen für die Generations-Berechnung
//!   und liest `Person`-Felder für die Karten (Name, Geburtsjahr, Foto).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct PhotoCrop {
    /// Verschiebung des Ausschnitts in [-1, 1] relativ zum Spielraum.
    pub x: f32,
    pub y: f32,
    /// Zoom des Ausschnitts (1 = Bild füllt den Rahmen, > 1 = herangezoomt).
    pub zoom: f32,
}

impl Default for PhotoCrop {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Person {
    pub id: String,
    #[serde(default)]
    pub given_name: String,
    #[serde(default)]
    pub family_name: String,
    /// Legacy-Kombifeld alter Projektdateien ("Vorname Nachname"). Wird beim
    /// Laden in `given_name`/`family_name` überführt und nicht mehr
    /// geschrieben (siehe `import::normalize_names`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub birth: String,
    #[serde(default)]
    pub birth_place: String,
    pub death: String,
    #[serde(default)]
    pub death_place: String,
    pub gender: Gender,
    // Relativer Pfad unter dem Medien-Basisordner ("media/<hash>.<ext>",
    // Auflösung siehe `media::photo_texture`).
    #[serde(default)]
    pub photo: Option<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub source: String,
    // Galerie-Pfade, gleiche Regel wie `photo`.
    #[serde(default)]
    pub gallery: Vec<String>,
    /// Gewählter Bildausschnitt für das Profilfoto (kein Verzerren —
    /// Cover-Beschnitt mit Zoom/Verschiebung, siehe `media::cover_uv`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub photo_crop: Option<PhotoCrop>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Gender {
    Male,
    Female,
    #[default]
    Unknown,
}

impl Person {
    /// Anzeigename ("Vorname Nachname"); bei fehlenden Nachnamen nur der
    /// Vorname, als letzter Fallback das Legacy-Kombifeld.
    pub fn display_name(&self) -> String {
        let combined = format!("{} {}", self.given_name, self.family_name);
        let combined = combined.trim();
        if !combined.is_empty() {
            combined.to_string()
        } else {
            self.name.clone()
        }
    }
}

/// Art der Partnerschaft (Partner-Zeile im Beziehungseditor). Alte
/// bool-Einträge (`partner_relations`) werden ignoriert.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum PartnerRelation {
    #[default]
    Unknown,
    Married,
    Divorced,
    Partnered,
}

impl PartnerRelation {
    pub fn label(self) -> &'static str {
        match self {
            PartnerRelation::Unknown => "Unbekannt",
            PartnerRelation::Married => "Verheiratet",
            PartnerRelation::Divorced => "Geschieden",
            PartnerRelation::Partnered => "Partnerschaft",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Family {
    pub id: String,
    pub parent_a: Option<String>,
    pub parent_b: Option<String>,
    pub children: Vec<String>,
}

/// Beziehungsart eines Kindes zu seinen Eltern (wie in Gramps: leiblich,
/// adoptiert, Stiefkind, Pflegekind). Gespeichert in
/// `TreeData::child_relations` mit Schlüssel "<familie>/<kind>".
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ChildRelation {
    #[default]
    Birth,
    Adopted,
    Step,
    Foster,
}

impl ChildRelation {
    pub fn label(self) -> &'static str {
        match self {
            ChildRelation::Birth => "Leiblich",
            ChildRelation::Adopted => "Adoptiert",
            ChildRelation::Step => "Stiefkind",
            ChildRelation::Foster => "Pflegekind",
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct TreeData {
    /// MiniGramps-spezifische Eigenschaften; fremde Importformate erhalten
    /// hier automatisch neutrale Standardwerte.
    #[serde(default)]
    pub project: ProjectMetadata,
    pub people: Vec<Person>,
    pub families: Vec<Family>,
    #[serde(default)]
    pub child_relations: HashMap<String, ChildRelation>,
    /// Partnerschaftsart je Familie (Schlüssel = Familien-ID); fehlender
    /// Eintrag = Unbekannt.
    #[serde(default)]
    pub partner_relations: HashMap<String, PartnerRelation>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ProjectMetadata {
    #[serde(default = "default_project_name")]
    pub name: String,
    #[serde(default = "default_project_version")]
    pub format_version: u32,
}

impl Default for ProjectMetadata {
    fn default() -> Self {
        Self {
            name: default_project_name(),
            format_version: default_project_version(),
        }
    }
}

fn default_project_name() -> String {
    "Unbenanntes Projekt".into()
}

const fn default_project_version() -> u32 {
    1
}

impl TreeData {
    pub fn demo() -> Self {
        Self {
            project: ProjectMetadata {
                name: "Beispielbaum".into(),
                ..Default::default()
            },
            people: vec![
                person("p1", "Helena", "Bergmann", "1948", Gender::Female),
                person("p2", "Martin", "Bergmann", "1945", Gender::Male),
                person("p3", "Clara", "Bergmann", "1974", Gender::Female),
                person("p4", "Jonas", "Adler", "1971", Gender::Male),
                person("p5", "Mila", "Adler", "2001", Gender::Female),
                person("p6", "Noah", "Adler", "2005", Gender::Male),
            ],
            families: vec![
                Family {
                    id: "f1".into(),
                    parent_a: Some("p1".into()),
                    parent_b: Some("p2".into()),
                    children: vec!["p3".into()],
                },
                Family {
                    id: "f2".into(),
                    parent_a: Some("p3".into()),
                    parent_b: Some("p4".into()),
                    children: vec!["p5".into(), "p6".into()],
                },
            ],
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
        }
    }
    pub fn find(&self, id: &str) -> Option<&Person> {
        self.people.iter().find(|p| p.id == id)
    }
    pub fn children_of(&self, id: &str) -> Vec<&Person> {
        self.families
            .iter()
            .filter(|f| f.parent_a.as_deref() == Some(id) || f.parent_b.as_deref() == Some(id))
            .flat_map(|f| f.children.iter())
            .filter_map(|id| self.find(id))
            .collect()
    }
    pub fn parents_of(&self, id: &str) -> Vec<&Person> {
        self.families
            .iter()
            .filter(|f| f.children.iter().any(|c| c == id))
            .flat_map(|f| [&f.parent_a, &f.parent_b])
            .filter_map(|id| id.as_deref())
            .filter_map(|id| self.find(id))
            .collect()
    }
    pub fn siblings_of(&self, id: &str) -> Vec<&Person> {
        self.families
            .iter()
            .filter(|f| f.children.iter().any(|child| child == id))
            .flat_map(|f| f.children.iter())
            .filter(|sibling| sibling.as_str() != id)
            .filter_map(|sibling| self.find(sibling))
            .collect()
    }
    pub fn partners_of(&self, id: &str) -> Vec<&Person> {
        self.families
            .iter()
            .filter_map(|f| {
                if f.parent_a.as_deref() == Some(id) {
                    f.parent_b.as_deref()
                } else if f.parent_b.as_deref() == Some(id) {
                    f.parent_a.as_deref()
                } else {
                    None
                }
            })
            .filter_map(|partner| self.find(partner))
            .collect()
    }
    /// Verknüpft zwei Personen als Partner. Nutzt eine offene Ein-Elternteil-
    /// Familie, sonst eine neue. Aufgerufen aus dem Beziehungspicker
    /// (`ui::MiniGramps::relation_suggestions`).
    pub fn link_partner(&mut self, person_id: &str, partner_id: &str) {
        if person_id == partner_id
            || self
                .partners_of(person_id)
                .iter()
                .any(|person| person.id == partner_id)
        {
            return;
        }
        if let Some(family) = self.families.iter_mut().find(|family| {
            (family.parent_a.as_deref() == Some(person_id) && family.parent_b.is_none())
                || (family.parent_b.as_deref() == Some(person_id) && family.parent_a.is_none())
        }) {
            if family.parent_a.as_deref() == Some(person_id) {
                family.parent_b = Some(partner_id.into());
            } else {
                family.parent_a = Some(partner_id.into());
            }
        } else {
            self.families.push(Family {
                id: format!("f{}", self.families.len() + 1),
                parent_a: Some(person_id.into()),
                parent_b: Some(partner_id.into()),
                children: Vec::new(),
            });
        }
    }
    /// Einfaches Kinder-Zuordnen (Vorfahren-/Nachfahrenbaum-Verbindungen und
    /// Geschwister-Verknüpfung aus dem Beziehungspicker).
    pub fn link_child(&mut self, parent_id: &str, child_id: &str) {
        if parent_id == child_id {
            return;
        }
        if let Some(family) = self.families.iter_mut().find(|family| {
            family.parent_a.as_deref() == Some(parent_id)
                || family.parent_b.as_deref() == Some(parent_id)
        }) {
            if !family.children.iter().any(|child| child == child_id) {
                family.children.push(child_id.into());
            }
        } else {
            self.families.push(Family {
                id: format!("f{}", self.families.len() + 1),
                parent_a: Some(parent_id.into()),
                parent_b: None,
                children: vec![child_id.into()],
            });
        }
    }
    /// Kinder-Zuordnen mit zweitem Elternteil und Beziehungsart; wird vom
    /// `+ KIND`-Picker mit den Inline-Selektoren verwendet.
    pub fn link_child_to(
        &mut self,
        parent_a: Option<&str>,
        parent_b: Option<&str>,
        child_id: &str,
        relation: ChildRelation,
    ) {
        if Some(child_id) == parent_a || Some(child_id) == parent_b {
            return;
        }
        // Familien-Match REIHENFOLDERUNEMPFINDLICH: (a,b) und (b,a) sind
        // dasselbe Paar — sonst entsteht beim Anlegen eines Kindes eine
        // ZWEITE Familie für dasselbe Paar und damit ein Doppeleiner-
        // Partner-Eintrag im Profil.
        let existing = self
            .families
            .iter()
            .position(|family| {
                let a = family.parent_a.as_deref();
                let b = family.parent_b.as_deref();
                (a == parent_a && b == parent_b) || (a == parent_b && b == parent_a)
            })
            .map(|index| self.families[index].id.clone());
        let family_id = existing.unwrap_or_else(|| {
            let id = format!("f{}", self.families.len() + 1);
            self.families.push(Family {
                id: id.clone(),
                parent_a: parent_a.map(str::to_string),
                parent_b: parent_b.map(str::to_string),
                children: Vec::new(),
            });
            id
        });
        if let Some(family) = self
            .families
            .iter_mut()
            .find(|family| family.id == family_id)
        {
            if !family.children.iter().any(|child| child == child_id) {
                family.children.push(child_id.to_string());
            }
        }
        self.child_relations
            .insert(format!("{family_id}/{child_id}"), relation);
    }
    /// Beziehungsart eines Kindes in der Kinderliste des Profils
    /// (`ui::MiniGramps` rechte Seitenleiste, Abschnitt KINDER).
    pub fn relation_of_child(&self, parent_id: &str, child_id: &str) -> ChildRelation {
        self.families
            .iter()
            .filter(|family| {
                (family.parent_a.as_deref() == Some(parent_id)
                    || family.parent_b.as_deref() == Some(parent_id))
                    && family.children.iter().any(|child| child == child_id)
            })
            .find_map(|family| {
                self.child_relations
                    .get(&format!("{}/{child_id}", family.id))
            })
            .copied()
            .unwrap_or(ChildRelation::Birth)
    }

    /// Beziehungsart eines Kindes setzen (Expansion im Beziehungseditor,
    /// siehe `picker::relation_options`). Die Familie muss beide Personen
    /// verbinden (Elternteil + Kind).
    pub fn set_child_relation(&mut self, parent_id: &str, child_id: &str, relation: ChildRelation) {
        let family_ids: Vec<String> = self
            .families
            .iter()
            .filter(|family| {
                (family.parent_a.as_deref() == Some(parent_id)
                    || family.parent_b.as_deref() == Some(parent_id))
                    && family.children.iter().any(|child| child == child_id)
            })
            .map(|family| family.id.clone())
            .collect();
        for family_id in family_ids {
            self.child_relations
                .insert(format!("{family_id}/{child_id}"), relation);
        }
    }

    /// Beziehungsart eines Kindes in der Familie, die zwei Geschwister
    /// verbindet (reserviert fuer kuenftige Geschwister-Optionen).
    #[allow(dead_code)]
    pub fn set_sibling_relation(
        &mut self,
        person_id: &str,
        sibling_id: &str,
        relation: ChildRelation,
    ) {
        let family_ids: Vec<String> = self
            .families
            .iter()
            .filter(|family| {
                family.children.iter().any(|child| child == person_id)
                    && family.children.iter().any(|child| child == sibling_id)
            })
            .map(|family| family.id.clone())
            .collect();
        for family_id in family_ids {
            self.child_relations
                .insert(format!("{family_id}/{sibling_id}"), relation);
        }
    }

    /// Partnerschaft-Beziehung zweier Personen setzen (Partner-Zeile im
    /// Beziehungseditor).
    pub fn set_partner_relation(
        &mut self,
        person_id: &str,
        partner_id: &str,
        relation: PartnerRelation,
    ) {
        let family_ids: Vec<String> = self
            .families
            .iter()
            .filter(|family| {
                (family.parent_a.as_deref() == Some(person_id)
                    && family.parent_b.as_deref() == Some(partner_id))
                    || (family.parent_a.as_deref() == Some(partner_id)
                        && family.parent_b.as_deref() == Some(person_id))
            })
            .map(|family| family.id.clone())
            .collect();
        for family_id in family_ids {
            self.partner_relations.insert(family_id, relation);
        }
    }

    /// Art der Partnerschaft eines Paares; fehlender Eintrag = Unbekannt.
    pub fn partner_relation_of(&self, person_id: &str, partner_id: &str) -> PartnerRelation {
        self.families
            .iter()
            .filter(|family| {
                (family.parent_a.as_deref() == Some(person_id)
                    && family.parent_b.as_deref() == Some(partner_id))
                    || (family.parent_a.as_deref() == Some(partner_id)
                        && family.parent_b.as_deref() == Some(person_id))
            })
            .find_map(|family| self.partner_relations.get(&family.id))
            .copied()
            .unwrap_or(PartnerRelation::Unknown)
    }

    // --- Beziehungen lösen (✕-Buttons im Beziehungseditor) ------------------

    /// Partner-Beziehung trennen. Die Familie bleibt für gemeinsame Kinder
    /// bestehen (ein Elternteil bleibt); leere Familien verschwinden.
    pub fn unlink_partner(&mut self, person_id: &str, partner_id: &str) {
        for family in &mut self.families {
            let pair = (family.parent_a.as_deref() == Some(person_id)
                && family.parent_b.as_deref() == Some(partner_id))
                || (family.parent_a.as_deref() == Some(partner_id)
                    && family.parent_b.as_deref() == Some(person_id));
            if pair {
                if family.parent_a.as_deref() == Some(partner_id) {
                    family.parent_a = None;
                } else {
                    family.parent_b = None;
                }
            }
        }
        self.cleanup_empty_families();
    }

    /// Kind aus den Familien eines Elternteils entfernen.
    pub fn unlink_child(&mut self, parent_id: &str, child_id: &str) {
        for family in &mut self.families {
            let is_parent = family.parent_a.as_deref() == Some(parent_id)
                || family.parent_b.as_deref() == Some(parent_id);
            if is_parent {
                family.children.retain(|child| child != child_id);
            }
        }
        self.cleanup_empty_families();
    }

    /// Geschwister aus den gemeinsamen Familien entfernen.
    pub fn unlink_sibling(&mut self, person_id: &str, sibling_id: &str) {
        for family in &mut self.families {
            let shared = family.children.iter().any(|child| child == person_id)
                && family.children.iter().any(|child| child == sibling_id);
            if shared {
                family.children.retain(|child| child != sibling_id);
            }
        }
        self.cleanup_empty_families();
    }

    /// Elternteil aus den Familien der Person entfernen.
    pub fn unlink_parent(&mut self, person_id: &str, parent_id: &str) {
        for family in &mut self.families {
            if family.children.iter().any(|child| child == person_id) {
                if family.parent_a.as_deref() == Some(parent_id) {
                    family.parent_a = None;
                }
                if family.parent_b.as_deref() == Some(parent_id) {
                    family.parent_b = None;
                }
            }
        }
        self.cleanup_empty_families();
    }

    /// Familien ohne Eltern UND ohne Kinder auflösen; verwaiste
    /// Beziehungsart-Einträge aufräumen.
    fn cleanup_empty_families(&mut self) {
        self.families
            .retain(|family| family.parent_a.is_some() || family.parent_b.is_some());
        let ids: std::collections::HashSet<&str> = self
            .families
            .iter()
            .map(|family| family.id.as_str())
            .collect();
        self.child_relations
            .retain(|key, _| ids.contains(key.split('/').next().unwrap_or("")));
        self.partner_relations
            .retain(|key, _| ids.contains(key.as_str()));
    }
}

pub fn person(
    id: &str,
    given_name: &str,
    family_name: &str,
    birth: &str,
    gender: Gender,
) -> Person {
    Person {
        id: id.into(),
        given_name: given_name.into(),
        family_name: family_name.into(),
        name: String::new(),
        birth: birth.into(),
        birth_place: String::new(),
        death: String::new(),
        death_place: String::new(),
        gender,
        photo: None,
        notes: String::new(),
        source: String::new(),
        gallery: Vec::new(),
        photo_crop: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_siblings_in_shared_family() {
        let data = TreeData::demo();
        assert_eq!(data.siblings_of("p5")[0].id, "p6");
        assert!(data.parents_of("p1").is_empty());
    }

    #[test]
    fn creates_family_links_without_duplicates() {
        let mut data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("a", "Alex", "", "", Gender::Unknown),
                person("b", "Bea", "", "", Gender::Unknown),
                person("c", "Chris", "", "", Gender::Unknown),
            ],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
        };
        data.link_partner("a", "b");
        data.link_child("a", "c");
        data.link_child("a", "c");
        assert_eq!(data.partners_of("a")[0].id, "b");
        assert_eq!(data.parents_of("c")[0].id, "a");
        assert_eq!(data.families[0].children.len(), 1);
    }

    #[test]
    fn stores_child_relation_with_family() {
        let mut data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("a", "Alex", "", "", Gender::Unknown),
                person("b", "Bea", "", "", Gender::Unknown),
                person("c", "Chris", "", "", Gender::Unknown),
            ],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
        };
        data.link_child_to(Some("a"), Some("b"), "c", ChildRelation::Adopted);
        data.link_child_to(Some("a"), Some("b"), "c", ChildRelation::Adopted);
        assert_eq!(data.families.len(), 1);
        assert_eq!(data.parents_of("c").len(), 2);
        assert_eq!(data.relation_of_child("a", "c"), ChildRelation::Adopted);
        assert_eq!(data.relation_of_child("b", "c"), ChildRelation::Adopted);
    }
}
