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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventKind {
    Birth,
    Death,
    Marriage,
    Divorce,
    Baptism,
    Burial,
    Occupation,
    Residence,
    Immigration,
    Emigration,
    Census,
    Graduation,
    Retirement,
    Custom(String),
}

impl EventKind {
    pub fn label(&self) -> &str {
        match self {
            EventKind::Birth => "Geburt",
            EventKind::Death => "Tod",
            EventKind::Marriage => "Heirat",
            EventKind::Divorce => "Scheidung",
            EventKind::Baptism => "Taufe",
            EventKind::Burial => "Beerdigung",
            EventKind::Occupation => "Beruf",
            EventKind::Residence => "Wohnort",
            EventKind::Immigration => "Einwanderung",
            EventKind::Emigration => "Auswanderung",
            EventKind::Census => "Volkszählung",
            EventKind::Graduation => "Abschluss",
            EventKind::Retirement => "Ruhestand",
            EventKind::Custom(s) => s,
        }
    }

    pub fn from_gramps(s: &str) -> Self {
        match s {
            "Birth" => EventKind::Birth,
            "Death" => EventKind::Death,
            "Marriage" | "Marriages" => EventKind::Marriage,
            "Divorce" | "Divorces" => EventKind::Divorce,
            "Baptism" | "Christening" => EventKind::Baptism,
            "Burial" | "Cremation" => EventKind::Burial,
            "Occupation" => EventKind::Occupation,
            "Residence" => EventKind::Residence,
            "Immigration" => EventKind::Immigration,
            "Emigration" => EventKind::Emigration,
            "Census" => EventKind::Census,
            "Graduation" | "Education" => EventKind::Graduation,
            "Retirement" => EventKind::Retirement,
            other => EventKind::Custom(other.to_string()),
        }
    }

    pub fn from_gedcom(s: &str) -> Self {
        match s {
            "BIRT" => EventKind::Birth,
            "DEAT" => EventKind::Death,
            "MARR" => EventKind::Marriage,
            "DIV" => EventKind::Divorce,
            "BAPM" | "CHR" => EventKind::Baptism,
            "BURI" | "CREM" => EventKind::Burial,
            "OCCU" => EventKind::Occupation,
            "RESI" => EventKind::Residence,
            "IMMI" => EventKind::Immigration,
            "EMIG" => EventKind::Emigration,
            "CENS" => EventKind::Census,
            "GRAD" | "EDUC" => EventKind::Graduation,
            "RETI" => EventKind::Retirement,
            other => EventKind::Custom(other.to_string()),
        }
    }

    /// Alle vordefinierten Ereignisarten für das Anlegen neuer Ereignisse.
    /// Wird vom späteren Ereignis-Editor genutzt.
    #[allow(dead_code)]
    pub fn all_variants() -> &'static [EventKind] {
        &[
            EventKind::Birth,
            EventKind::Death,
            EventKind::Marriage,
            EventKind::Divorce,
            EventKind::Baptism,
            EventKind::Burial,
            EventKind::Occupation,
            EventKind::Residence,
            EventKind::Immigration,
            EventKind::Emigration,
            EventKind::Census,
            EventKind::Graduation,
            EventKind::Retirement,
        ]
    }
}

impl Default for EventKind {
    fn default() -> Self {
        EventKind::Custom(String::new())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub kind: EventKind,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub place: String,
    #[serde(default)]
    pub description: String,
}

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

#[derive(Clone, Serialize, Deserialize, PartialEq)]
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
    // Erweiterte Namensbestandteile (Feldauswahl wie in Gramps). Leere
    // Felder werden nicht serialisiert; Anzeige im Profil, sobald Inhalt
    // vorhanden oder per Rechtsklick aktiviert.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub nick_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub call_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name_prefix: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub surname_prefix: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub suffix: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name_origin: String,
    /// Ereignisse (Geburt, Tod, Heirat, Beruf …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
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

    /// Erste beiden Vornamen (Token) ohne den Nachnamen – für die große
    /// Fotoansicht, wo der Nachname separat darunter steht.
    pub fn given_short(&self) -> String {
        self.given_name
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Kurzname für Baumkarten: höchstens die ersten beiden Vornamen plus
    /// Nachname, damit lange Namensfolgen die Karten nicht aufblähen.
    pub fn display_name_short(&self) -> String {
        let combined = format!("{} {}", self.given_short(), self.family_name);
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

#[derive(Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
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
    /// Manuelle Partnerreihenfolge je Person (Schlüssel = Personen-ID).
    /// Gilt NUR für Partner ohne Kennenlern-/Heiratsdatum; datierte Partner
    /// bleiben chronologisch sortiert (fixe Reihenfolge).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub partner_order: HashMap<String, Vec<String>>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
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
            partner_order: HashMap::new(),
        }
    }
    pub fn find(&self, id: &str) -> Option<&Person> {
        self.people.iter().find(|p| p.id == id)
    }
    pub fn children_of(&self, id: &str) -> Vec<&Person> {
        let mut children: Vec<&Person> = self.families
            .iter()
            .filter(|f| f.parent_a.as_deref() == Some(id) || f.parent_b.as_deref() == Some(id))
            .flat_map(|f| f.children.iter())
            .filter_map(|id| self.find(id))
            .collect();

        children.sort_by(|a, b| {
            let key_a = if let Some((y, m, d)) = parse_birth_date(&a.birth) {
                (0, y, m, d, a.id.as_str())
            } else {
                (1, 0, 0, 0, a.id.as_str())
            };
            let key_b = if let Some((y, m, d)) = parse_birth_date(&b.birth) {
                (0, y, m, d, b.id.as_str())
            } else {
                (1, 0, 0, 0, b.id.as_str())
            };
            key_a.cmp(&key_b)
        });
        children
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

    pub fn get_partnership_dates(&self, p1: &Person, p2: &Person) -> (Option<(i32, i32, i32)>, Option<(i32, i32, i32)>) {
        let mut zus_date = None;
        let mut mar_date = None;
        let is_zus_event = |kind: &EventKind| -> bool {
            match kind {
                EventKind::Custom(s) => {
                    let sl = s.to_lowercase();
                    sl == "partnerschaft" || sl == "zusammenkommen" || sl == "zusammenkunft" || sl == "zusammen"
                }
                _ => false,
            }
        };
        for event in &p1.events {
            if is_zus_event(&event.kind) {
                if let Some(d) = parse_birth_date(&event.date) {
                    if zus_date.is_none() || d < zus_date.unwrap() {
                        zus_date = Some(d);
                    }
                }
            } else if event.kind == EventKind::Marriage {
                if let Some(d) = parse_birth_date(&event.date) {
                    if mar_date.is_none() || d < mar_date.unwrap() {
                        mar_date = Some(d);
                    }
                }
            }
        }
        for event in &p2.events {
            if is_zus_event(&event.kind) {
                if let Some(d) = parse_birth_date(&event.date) {
                    if zus_date.is_none() || d < zus_date.unwrap() {
                        zus_date = Some(d);
                    }
                }
            } else if event.kind == EventKind::Marriage {
                if let Some(d) = parse_birth_date(&event.date) {
                    if mar_date.is_none() || d < mar_date.unwrap() {
                        mar_date = Some(d);
                    }
                }
            }
        }
        (zus_date, mar_date)
    }

    pub fn partners_of(&self, id: &str) -> Vec<&Person> {
        let mut partners: Vec<&Person> = self.families
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
            .collect();

        if let Some(p1) = self.find(id) {
            // Manuelle Reihenfolge (nur für Partner ohne Datum): Position je
            // Partner-ID in der gewünschten Liste als Tiebreaker vor der ID.
            let manual = self.partner_order.get(id);
            let manual_pos = |partner_id: &str| -> usize {
                manual
                    .and_then(|list| list.iter().position(|p| p == partner_id))
                    .unwrap_or(usize::MAX)
            };
            let key = |partner: &Person| -> (u8, (i32, i32, i32), (i32, i32, i32), usize, String) {
                let (z, m) = self.get_partnership_dates(p1, partner);
                let (priority, d1, d2) = if let Some(d) = z {
                    (
                        0,
                        d,
                        m.unwrap_or((9999, 12, 31)),
                    )
                } else if let Some(d) = m {
                    (1, d, (9999, 12, 31))
                } else {
                    (2, (9999, 12, 31), (9999, 12, 31))
                };
                (priority, d1, d2, manual_pos(&partner.id), partner.id.clone())
            };
            partners.sort_by(|a, b| key(a).cmp(&key(b)));
        }
        partners
    }
    /// Partner-Pseudokarten per Ziehen tauschen (nur Partner ohne
    /// Kennenlern-/Heiratsdatum; datierte Partner sind chronologisch fix).
    /// `direction` = -1 (nach links/vorn) oder +1 (nach rechts/hinten) in
    /// der angezeigten Reihenfolge.
    pub fn swap_partner(&mut self, person_id: &str, partner_id: &str, direction: i32) {
        let Some(p1) = self.find(person_id).cloned() else {
            return;
        };
        let is_free = |p: &Person| {
            let (z, m) = self.get_partnership_dates(&p1, p);
            z.is_none() && m.is_none()
        };
        // Angezeigte Reihenfolge der freien Partner (altes Manual berücksichtigt)…
        let old = self.partner_order.get(person_id).cloned().unwrap_or_default();
        let pos = |id: &str| old.iter().position(|p| p == id).unwrap_or(usize::MAX);
        let mut order: Vec<String> = self
            .families
            .iter()
            .filter_map(|f| {
                if f.parent_a.as_deref() == Some(person_id) {
                    f.parent_b.as_deref()
                } else if f.parent_b.as_deref() == Some(person_id) {
                    f.parent_a.as_deref()
                } else {
                    None
                }
            })
            .filter_map(|other| self.find(other))
            .filter(|partner| is_free(partner))
            .map(|partner| partner.id.clone())
            .collect();
        order.sort_by_key(|id| pos(id));
        // …und Tausch mit dem Nachbarn in Ziehrichtung.
        let Some(index) = order.iter().position(|id| id == partner_id) else {
            return;
        };
        let target = index as i32 + direction;
        if target < 0 || target >= order.len() as i32 {
            return;
        }
        let target = target as usize;
        if order[target] == order[index] {
            return;
        }
        order.swap(index, target);
        self.partner_order.insert(person_id.to_string(), order);
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
        title: String::new(),
        nick_name: String::new(),
        call_name: String::new(),
        name_prefix: String::new(),
        surname_prefix: String::new(),
        suffix: String::new(),
        name_type: String::new(),
        name_origin: String::new(),
        events: Vec::new(),
    }
}

/// Hilfsfunktion, um ein beliebiges Geburtsdatums-String in eine vergleichbare
/// Struktur (Jahr, Monat, Tag) zu parsen. Unterstützt Deutsch und Englisch,
/// volle Monatsnamen und Abkürzungen sowie unsichere/ungefähre Angaben
/// (z. B. "um 1944", "~1966", "1966?").
pub fn parse_birth_date(date_str: &str) -> Option<(i32, i32, i32)> {
    let s = date_str.trim().to_lowercase();
    if s.is_empty() || s == "unbekannt" {
        return None;
    }
    // Nach allen nicht-alphanumerischen Zeichen splitten (entfernt ?, ~, [], (), Punkte, etc.)
    let tokens: Vec<&str> = s.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).collect();
    if tokens.is_empty() {
        return None;
    }

    let mut year = None;
    let mut month = None;
    let mut day = None;

    let month_names = [
        vec!["jan", "januar", "january"],
        vec!["feb", "februar", "february"],
        vec!["mar", "mär", "märz", "march"],
        vec!["apr", "april"],
        vec!["mai", "may"],
        vec!["jun", "juni", "june"],
        vec!["jul", "juli", "july"],
        vec!["aug", "august"],
        vec!["sep", "september"],
        vec!["okt", "oct", "oktober", "october"],
        vec!["nov", "november"],
        vec!["dez", "dec", "dezember", "december"],
    ];

    for token in &tokens {
        let mut found_month = false;
        for (idx, aliases) in month_names.iter().enumerate() {
            if aliases.iter().any(|&alias| token.starts_with(alias)) {
                month = Some((idx + 1) as i32);
                found_month = true;
                break;
            }
        }
        if found_month {
            continue;
        }

        if let Ok(num) = token.parse::<i32>() {
            if num >= 100 && num <= 3000 {
                year = Some(num);
            } else if num >= 1 && num <= 31 {
                if day.is_none() {
                    day = Some(num);
                } else if month.is_none() && num <= 12 {
                    month = Some(num);
                }
            }
        }
    }

    if tokens.len() == 3 {
        if let (Ok(num1), Ok(num2), Ok(num3)) = (tokens[0].parse::<i32>(), tokens[1].parse::<i32>(), tokens[2].parse::<i32>()) {
            if num3 >= 100 && num3 <= 3000 {
                year = Some(num3);
                month = Some(num2);
                day = Some(num1);
            } else if num1 >= 100 && num1 <= 3000 {
                year = Some(num1);
                month = Some(num2);
                day = Some(num3);
            }
        }
    } else if tokens.len() == 1 {
        if let Ok(num) = tokens[0].parse::<i32>() {
            if num >= 100 && num <= 3000 {
                year = Some(num);
            }
        }
    }

    if let Some(y) = year {
        Some((y, month.unwrap_or(1), day.unwrap_or(1)))
    } else {
        None
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
            partner_order: HashMap::new(),
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
            partner_order: HashMap::new(),
        };
        data.link_child_to(Some("a"), Some("b"), "c", ChildRelation::Adopted);
        data.link_child_to(Some("a"), Some("b"), "c", ChildRelation::Adopted);
        assert_eq!(data.families.len(), 1);
        assert_eq!(data.parents_of("c").len(), 2);
        assert_eq!(data.relation_of_child("a", "c"), ChildRelation::Adopted);
        assert_eq!(data.relation_of_child("b", "c"), ChildRelation::Adopted);
    }

    #[test]
    fn swaps_only_free_partners() {
        let mut data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("a", "Alex", "", "", Gender::Unknown),
                person("b", "Bea", "", "", Gender::Unknown),
                person("c", "Chris", "", "", Gender::Unknown),
                person("d", "Dana", "", "", Gender::Unknown),
            ],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        data.link_partner("a", "b");
        data.link_partner("a", "c");
        data.link_partner("a", "d");
        fn ids(data: &TreeData) -> Vec<&str> {
            data.partners_of("a").iter().map(|p| p.id.as_str()).collect()
        }
        assert_eq!(ids(&data), vec!["b", "c", "d"]);
        // Freie Partner per Ziehen tauschen: „c“ eine Position nach vorn.
        data.swap_partner("a", "c", -1);
        assert_eq!(ids(&data), vec!["c", "b", "d"]);
        assert_eq!(data.partner_order["a"], vec!["c", "b", "d"]);
        // Randfall: am äußersten Ende ändert sich nichts.
        data.swap_partner("a", "d", 1);
        assert_eq!(ids(&data), vec!["c", "b", "d"]);
        // Folgende Tausche respektieren die gespeicherte Reihenfolge.
        data.swap_partner("a", "b", 1);
        assert_eq!(ids(&data), vec!["c", "d", "b"]);
    }

    #[test]
    fn dated_partners_stay_fixed() {
        let mut c = person("c", "Chris", "", "", Gender::Unknown);
        c.events.push(Event {
            kind: EventKind::Marriage,
            date: "1990".into(),
            place: String::new(),
            description: String::new(),
        });
        let mut d = person("d", "Dana", "", "", Gender::Unknown);
        d.events.push(Event {
            kind: EventKind::Custom("Partnerschaft".into()),
            date: "2001".into(),
            place: String::new(),
            description: String::new(),
        });
        let mut data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("a", "Alex", "", "", Gender::Unknown),
                person("b", "Bea", "", "", Gender::Unknown),
                c,
                d,
            ],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        data.link_partner("a", "b");
        data.link_partner("a", "c");
        data.link_partner("a", "d");
        // Chronologische Sortierung: Partnerschaft (2001) vor Heirat (1990),
        // freier Partner „b“ ans Ende.
        let ids: Vec<&str> = data.partners_of("a").iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["d", "c", "b"]);
        // Datierte Partner sind nicht tauschbar — Reihenfolge bleibt fix.
        data.swap_partner("a", "c", -1);
        data.swap_partner("a", "d", -1);
        data.swap_partner("a", "d", 1);
        let ids: Vec<&str> = data.partners_of("a").iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["d", "c", "b"]);
        assert!(data.partner_order.is_empty());
    }

    #[test]
    fn test_parse_birth_date() {
        assert_eq!(parse_birth_date("2. Juni 1966"), Some((1966, 6, 2)));
        assert_eq!(parse_birth_date("10.04.1957"), Some((1957, 4, 10)));
        assert_eq!(parse_birth_date("1957-04-10"), Some((1957, 4, 10)));
        assert_eq!(parse_birth_date("1971"), Some((1971, 1, 1)));
        assert_eq!(parse_birth_date("12 MAR 1950"), Some((1950, 3, 12)));
        assert_eq!(parse_birth_date("um 1944"), Some((1944, 1, 1)));
        assert_eq!(parse_birth_date("~1966"), Some((1966, 1, 1)));
        assert_eq!(parse_birth_date("1966?"), Some((1966, 1, 1)));
        assert_eq!(parse_birth_date("[1966]"), Some((1966, 1, 1)));
        assert_eq!(parse_birth_date("Unbekannt"), None);
        assert_eq!(parse_birth_date(""), None);
        // Medieval/historical year support
        assert_eq!(parse_birth_date("800"), Some((800, 1, 1)));
    }

    #[test]
    fn test_parse_birth_date_bauke() {
        // Teste alle echten Datums-Formate aus dem BAUKE Stammbaum
        assert_eq!(parse_birth_date("16 Jun 1998"), Some((1998, 6, 16)));
        assert_eq!(parse_birth_date("21 Oktober 1969"), Some((1969, 10, 21)));
        assert_eq!(parse_birth_date("27.7.1974"), Some((1974, 7, 27)));
        assert_eq!(parse_birth_date("3 Juni 1942"), Some((1942, 6, 3)));
        assert_eq!(parse_birth_date("29 September 1952"), Some((1952, 9, 29)));
        assert_eq!(parse_birth_date("19 Aug 1920"), Some((1920, 8, 19)));
        assert_eq!(parse_birth_date("8 Mai 1910"), Some((1910, 5, 8)));
        assert_eq!(parse_birth_date("24 Dezember 1924"), Some((1924, 12, 24)));
        assert_eq!(parse_birth_date("21 Juli 1907"), Some((1907, 7, 21)));
        assert_eq!(parse_birth_date("21 Februar 1877"), Some((1877, 2, 21)));
        assert_eq!(parse_birth_date("21 Januar 1891"), Some((1891, 1, 21)));
        assert_eq!(parse_birth_date("28 Juli 1879"), Some((1879, 7, 28)));
        assert_eq!(parse_birth_date("10 Dezember 1892"), Some((1892, 12, 10)));
        assert_eq!(parse_birth_date("1 Feb 1888"), Some((1888, 2, 1)));
        assert_eq!(parse_birth_date("25 Februar 1897"), Some((1897, 2, 25)));
        assert_eq!(parse_birth_date("19. Januar 1848"), Some((1848, 1, 19)));
    }
}
