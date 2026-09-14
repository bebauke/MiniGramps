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

use std::collections::{HashMap, HashSet};

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

impl Event {
    /// Leeres Ereignis der angegebenen Art.
    pub fn new(kind: EventKind) -> Self {
        Self {
            kind,
            date: String::new(),
            place: String::new(),
            description: String::new(),
        }
    }
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
    /// Dokument-Dateien (Urkunden, Scans, PDFs …), gleiche Regel wie `photo`.
    /// Gramps-Gegenstück: Media-Objekte einer Person (ohne Galerie-Bilder).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documents: Vec<DocumentEntry>,
    /// Quellen-Einträge (Titel + Fundstelle/Seite). Gramps-Gegenstück:
    /// Citation (Source-Titel + Page) pro Person; das alte Freitextfeld
    /// `source` bleibt für bestehende Projekte erhalten.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceEntry>,
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

/// Dokument-Eintrag einer Person (Dateipfad im Medienordner plus
/// Anzeigename, da gespeicherte Dateien Hash-Namen tragen).
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentEntry {
    pub path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

/// Quelleneintrag einer Person (Gramps: Citation aus Source-Titel + Page).
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceEntry {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Fundstelle/Seite (Gramps-`page` der Citation).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
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
    ///
    /// Ist ein **Rufname** hinterlegt, werden nur der erste vom Rufnamen
    /// abweichende Vorname und der Rufname gezeigt (z. B. „Hans Jürgen" für
    /// „Hans Jürgen" mit Rufname „Jürgen"); gibt es keinen abweichenden
    /// Vornamen, nur der Rufname.
    pub fn given_short(&self) -> String {
        let tokens: Vec<&str> = self.given_name.split_whitespace().collect();
        let call = self.call_name.trim();
        if !call.is_empty() {
            return match tokens.iter().find(|token| !token.eq_ignore_ascii_case(call)) {
                Some(first) => format!("{first} {call}"),
                None => call.to_string(),
            };
        }
        tokens.into_iter().take(2).collect::<Vec<_>>().join(" ")
    }

    /// Kurzname für Baumkarten: höchstens die ersten beiden Vornamen plus
    /// Nachname, damit lange Namensfolgen die Karten nicht aufblähen.
    #[allow(dead_code)]
    pub fn display_name_short(&self) -> String {
        let combined = format!("{} {}", self.given_short(), self.family_name);
        let combined = combined.trim();
        if !combined.is_empty() {
            combined.to_string()
        } else {
            self.name.clone()
        }
    }

    /// Geburtsdatum für die Baumkarten: normiert („DD Mnt YYYY"), leere
    /// Angabe wird zu „Unbekannt".
    pub fn birth_short(&self) -> String {
        let normalized = normalize_date(&self.birth);
        if normalized.is_empty() {
            "Unbekannt".to_string()
        } else {
            normalized
        }
    }

    /// Sterbedatum für die Baumkarten: normiert, leer wenn unbekannt (die
    /// Zeile entfällt dann).
    pub fn death_short(&self) -> String {
        normalize_date(&self.death)
    }

    /// Lebensdaten für die Baumkarten: Geburts- und Sterbedatum normiert
    /// („DD Mnt YYYY – DD Mnt YYYY"); ohne Sterbedatum nur die Geburt.
    #[allow(dead_code)]
    pub fn lifespan_short(&self) -> String {
        let birth = self.birth_short();
        let death = normalize_date(&self.death);
        if death.is_empty() {
            birth
        } else {
            format!("{birth} – {death}")
        }
    }

    /// Stellt sicher, dass **Geburt** und **Tod** als Standard-Ereignisse
    /// vorhanden sind (Gramps-Prinzip: Ereignisse mit Typ). Geburt steht
    /// zuerst, dann Tod; die übrigen Ereignisse folgen in ihrer Reihenfolge.
    /// Fehlende Standard-Ereignisse werden aus den Kurzfeldern erzeugt.
    pub fn ensure_standard_events(&mut self) {
        if !self.events.iter().any(|event| event.kind == EventKind::Birth) {
            let mut event = Event::new(EventKind::Birth);
            event.date = self.birth.clone();
            event.place = self.birth_place.clone();
            self.events.insert(0, event);
        }
        if !self.events.iter().any(|event| event.kind == EventKind::Death) {
            let mut event = Event::new(EventKind::Death);
            event.date = self.death.clone();
            event.place = self.death_place.clone();
            let position = self
                .events
                .iter()
                .position(|event| event.kind == EventKind::Birth)
                .map(|index| index + 1)
                .unwrap_or(self.events.len());
            self.events.insert(position, event);
        }
        // Reihenfolge normalisieren: Geburt, Tod, dann alle übrigen.
        let mut birth = None;
        let mut death = None;
        let mut rest = Vec::new();
        for event in self.events.drain(..) {
            match event.kind {
                EventKind::Birth => birth = Some(event),
                EventKind::Death => death = Some(event),
                _ => rest.push(event),
            }
        }
        self.events.extend(birth);
        self.events.extend(death);
        self.events.extend(rest);
    }

    /// Schreibt Datum/Ort der Standard-Ereignisse in die Kurzfelder
    /// (`birth`/`death`) zurück, damit Baum, Sortierung und Import konsistent
    /// bleiben.
    pub fn sync_standard_fields(&mut self) {
        match self
            .events
            .iter()
            .find(|event| event.kind == EventKind::Birth)
            .cloned()
        {
            Some(event) => {
                self.birth = event.date;
                self.birth_place = event.place;
            }
            None => {
                self.birth.clear();
                self.birth_place.clear();
            }
        }
        match self
            .events
            .iter()
            .find(|event| event.kind == EventKind::Death)
            .cloned()
        {
            Some(event) => {
                self.death = event.date;
                self.death_place = event.place;
            }
            None => {
                self.death.clear();
                self.death_place.clear();
            }
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

    /// Vornamen-Statistik aus dem Bestand: Geschlecht mit klarer Mehrheit
    /// (>50 %) für den Vornamen (erster Token der Anfrage, normiert) — z. B.
    /// Johann mit 90 % Männlich-Anteil ergibt `Male`. Es zählen alle
    /// Vornamen-Token der Bestandspersonen, auch Zweitnamen (Rufname an
    /// zweiter Stelle wie „Johann Hartmut" stimmt für die Anfrage „Hartmut"
    /// mit ab). `None` ohne Treffer, bei Gleichstand oder wenn nur
    /// „unbekannt" belegt ist — unbelegte Namen können nichts vorschlagen.
    /// Dient als initialer Geschlechts-Default neuer Personen (editierbar,
    /// kein Zurückschreiben).
    pub fn gender_for_given_name(&self, given: &str) -> Option<Gender> {
        let key = normalize_token(given.split_whitespace().next().unwrap_or(""));
        if key.is_empty() {
            return None;
        }
        let mut male = 0u32;
        let mut female = 0u32;
        for person in &self.people {
            let matches = person
                .given_name
                .split_whitespace()
                .any(|token| normalize_token(token) == key);
            if !matches {
                continue;
            }
            match person.gender {
                Gender::Male => male += 1,
                Gender::Female => female += 1,
                Gender::Unknown => {}
            }
        }
        if male > female {
            Some(Gender::Male)
        } else if female > male {
            Some(Gender::Female)
        } else {
            None
        }
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

    /// Importierte Daten anhängen: Alle Personen-/Familien-IDs werden frisch
    /// vergeben (`m…`/`mf…`), damit nichts Bestehendes kollidiert. Gibt die
    /// neuen Personen-IDs zurück (Abgleich gegen den Bestand).
    pub fn append_import(&mut self, mut imported: TreeData) -> Vec<String> {
        let mut taken: HashSet<String> = self
            .people
            .iter()
            .map(|person| person.id.clone())
            .chain(self.families.iter().map(|family| family.id.clone()))
            .collect();
        let fresh = |prefix: &str, taken: &mut HashSet<String>| -> String {
            let mut counter = 1usize;
            loop {
                let id = format!("{prefix}{counter}");
                if taken.insert(id.clone()) {
                    return id;
                }
                counter += 1;
            }
        };
        let mut person_map: HashMap<String, String> = HashMap::new();
        for person in &imported.people {
            let new_id = fresh("m", &mut taken);
            person_map.insert(person.id.clone(), new_id);
        }
        let mut family_map: HashMap<String, String> = HashMap::new();
        for family in &imported.families {
            let new_id = fresh("mf", &mut taken);
            family_map.insert(family.id.clone(), new_id);
        }
        let remap_person = |id: &mut String, map: &HashMap<String, String>| {
            if let Some(new_id) = map.get(id) {
                *id = new_id.clone();
            }
        };
        for person in &mut imported.people {
            remap_person(&mut person.id, &person_map);
        }
        for family in &mut imported.families {
            remap_person(&mut family.id, &family_map);
            for parent in [&mut family.parent_a, &mut family.parent_b]
                .into_iter()
                .flatten()
            {
                remap_person(parent, &person_map);
            }
            for child in &mut family.children {
                remap_person(child, &person_map);
            }
        }
        let mut order_map: HashMap<String, Vec<String>> = HashMap::new();
        for (owner, order) in &imported.partner_order {
            if let Some(new_owner) = person_map.get(owner) {
                order_map.insert(
                    new_owner.clone(),
                    order
                        .iter()
                        .map(|id| person_map.get(id).cloned().unwrap_or_else(|| id.clone()))
                        .collect(),
                );
            }
        }
        imported.partner_order = order_map;
        let mut child_map = HashMap::new();
        for (key, relation) in &imported.child_relations {
            let mut parts = key.splitn(2, '/');
            let family = parts.next().unwrap_or("");
            let child = parts.next().unwrap_or("");
            let new_key = format!(
                "{}/{}",
                family_map.get(family).cloned().unwrap_or_else(|| family.into()),
                person_map.get(child).cloned().unwrap_or_else(|| child.into())
            );
            child_map.insert(new_key, *relation);
        }
        imported.child_relations = child_map;
        let mut partner_map = HashMap::new();
        for (family, relation) in &imported.partner_relations {
            partner_map.insert(
                family_map.get(family).cloned().unwrap_or_else(|| family.clone()),
                *relation,
            );
        }
        imported.partner_relations = partner_map;
        let new_ids: Vec<String> = imported
            .people
            .iter()
            .map(|person| person.id.clone())
            .collect();
        self.people.extend(imported.people);
        self.families.extend(imported.families);
        self.child_relations.extend(imported.child_relations);
        self.partner_relations.extend(imported.partner_relations);
        self.partner_order.extend(imported.partner_order);
        new_ids
    }

    /// Duplikat in ein bestehendes Profil einführen: leere Skalarfelder
    /// auffüllen, Ereignisse/Galerie/Dokumente/Quellen vereinen, Familien
    /// umhängen, Duplikat löschen.
    pub fn merge_persons(&mut self, keep_id: &str, drop_id: &str) {
        if keep_id == drop_id {
            return;
        }
        let Some(drop) = self.find(drop_id).cloned() else {
            return;
        };
        let Some(keep) = self
            .people
            .iter_mut()
            .find(|person| person.id == keep_id)
        else {
            return;
        };
        for (target, source) in [
            (&mut keep.given_name, &drop.given_name),
            (&mut keep.family_name, &drop.family_name),
            (&mut keep.birth, &drop.birth),
            (&mut keep.birth_place, &drop.birth_place),
            (&mut keep.death, &drop.death),
            (&mut keep.death_place, &drop.death_place),
            (&mut keep.notes, &drop.notes),
            (&mut keep.source, &drop.source),
            (&mut keep.title, &drop.title),
            (&mut keep.nick_name, &drop.nick_name),
            (&mut keep.call_name, &drop.call_name),
            (&mut keep.name_prefix, &drop.name_prefix),
            (&mut keep.surname_prefix, &drop.surname_prefix),
            (&mut keep.suffix, &drop.suffix),
            (&mut keep.name_type, &drop.name_type),
            (&mut keep.name_origin, &drop.name_origin),
        ] {
            if target.is_empty() && !source.is_empty() {
                *target = source.clone();
            }
        }
        if keep.gender == Gender::Unknown {
            keep.gender = drop.gender;
        }
        if keep.photo.is_none() {
            keep.photo = drop.photo.clone();
            keep.photo_crop = drop.photo_crop;
        }
        for event in drop.events {
            if !keep.events.contains(&event) {
                keep.events.push(event);
            }
        }
        for path in drop.gallery {
            if !keep.gallery.iter().any(|entry| entry == &path) {
                keep.gallery.push(path);
            }
        }
        for document in drop.documents {
            if !keep.documents.iter().any(|entry| entry.path == document.path) {
                keep.documents.push(document);
            }
        }
        for source in drop.sources {
            if !keep.sources.contains(&source) {
                keep.sources.push(source);
            }
        }
        for family in &mut self.families {
            for parent in [&mut family.parent_a, &mut family.parent_b]
                .into_iter()
                .flatten()
            {
                if parent == drop_id {
                    *parent = keep_id.to_string();
                }
            }
            if family.parent_a == family.parent_b {
                family.parent_b = None;
            }
            for child in &mut family.children {
                if child == drop_id {
                    *child = keep_id.to_string();
                }
            }
            family.children.sort();
            family.children.dedup();
        }
        for order in self.partner_order.values_mut() {
            for id in order.iter_mut() {
                if id == drop_id {
                    *id = keep_id.to_string();
                }
            }
            order.sort();
            order.dedup();
        }
        if let Some(order) = self.partner_order.remove(drop_id) {
            self.partner_order
                .entry(keep_id.to_string())
                .or_default()
                .extend(order);
        }
        let mut child_map = HashMap::new();
        for (key, relation) in std::mem::take(&mut self.child_relations) {
            let mut parts = key.splitn(2, '/');
            let family = parts.next().unwrap_or("");
            let child = parts.next().unwrap_or("");
            let child = if child == drop_id { keep_id } else { child };
            child_map.insert(format!("{family}/{child}"), relation);
        }
        self.child_relations = child_map;
        self.people.retain(|person| person.id != drop_id);
        self.cleanup_empty_families();
    }

    /// Selektives Zusammenführen aus dem Review: Standard-Merge plus
    /// seitenweise Wahl für Geburts-/Sterbedaten (inkl. Ereignis-Abgleich).
    pub fn apply_merge_choice(
        &mut self,
        keep_id: &str,
        drop_id: &str,
        take_new_birth: bool,
        take_new_death: bool,
    ) {
        let drop = self.find(drop_id).cloned();
        self.merge_persons(keep_id, drop_id);
        let Some(drop) = drop else {
            return;
        };
        for (take_new, kind, date, place) in [
            (
                take_new_birth,
                EventKind::Birth,
                drop.birth.clone(),
                drop.birth_place.clone(),
            ),
            (
                take_new_death,
                EventKind::Death,
                drop.death.clone(),
                drop.death_place.clone(),
            ),
        ] {
            if !take_new || date.trim().is_empty() {
                continue;
            }
            let Some(keep) = self.people.iter_mut().find(|person| person.id == keep_id) else {
                continue;
            };
            if kind == EventKind::Birth {
                keep.birth = date.clone();
                keep.birth_place = place.clone();
            } else {
                keep.death = date.clone();
                keep.death_place = place.clone();
            }
            match keep.events.iter_mut().find(|event| event.kind == kind) {
                Some(event) => {
                    event.date = date;
                    event.place = place;
                }
                None => keep.events.push(Event {
                    kind,
                    date,
                    place,
                    description: String::new(),
                }),
            }
        }
    }

    /// Häufige Vornamen-Token je normiertem Nachnamen: Token mit mindestens
    /// `min_count` Trägern in derselben Nachnamengruppe (je Person einmal
    /// gezählt). Einmalig pro Matcher-Lauf berechnen, pro Paar die Mengen
    /// beider Nachnamen vereinigen.
    fn common_given_tokens(&self, min_count: usize) -> HashMap<String, HashSet<String>> {
        let mut counts: HashMap<String, HashMap<String, usize>> = HashMap::new();
        for person in &self.people {
            let surname = normalize_token(&person.family_name);
            if surname.is_empty() {
                continue;
            }
            let entry = counts.entry(surname).or_default();
            let mut seen = HashSet::new();
            for token in name_tokens(&person.given_name) {
                if seen.insert(token.clone()) {
                    *entry.entry(token).or_insert(0) += 1;
                }
            }
        }
        counts
            .into_iter()
            .map(|(surname, tokens)| {
                (
                    surname,
                    tokens
                        .into_iter()
                        .filter(|(_, count)| *count >= min_count.max(2))
                        .map(|(token, _)| token)
                        .collect(),
                )
            })
            .collect()
    }

    /// Duplikat-Kandidat aus dem Abgleich (Bestand ↔ frisch Angehängtes).
    /// Sortierung absteigend nach Gesamtähnlichkeit (Name + Verwandtschaft).
    /// `common_min`: häufige Vornamen ab so vielen Gleichnamigen je
    /// Nachnamengruppe ohne Exakt-Boost (einstellbar, Default 4).
    pub fn find_merge_candidates(
        &self,
        fresh_ids: &HashSet<String>,
        threshold: f32,
        common_min: usize,
    ) -> Vec<MergeCandidate> {
        let fresh: Vec<&Person> = self
            .people
            .iter()
            .filter(|person| fresh_ids.contains(&person.id))
            .collect();
        let mut candidates = Vec::new();
        // Häufige Vornamen je Nachnamengruppe (einmalig): leere Referenz für
        // Paare ohne Befund, wiederverwendeter Puffer für die Vereinigung.
        let common = self.common_given_tokens(common_min);
        let no_ignore: HashSet<String> = HashSet::new();
        let mut union_buf: HashSet<String> = HashSet::new();
        for incoming in fresh {
            for existing in &self.people {
                if fresh_ids.contains(&existing.id) || existing.id == incoming.id {
                    continue;
                }
                // Ignore-Menge des Paars (beide Nachnamengruppen).
                union_buf.clear();
                for surname in [&existing.family_name, &incoming.family_name] {
                    if let Some(tokens) = common.get(&normalize_token(surname)) {
                        union_buf.extend(tokens.iter().cloned());
                    }
                }
                let ignore: &HashSet<String> =
                    if union_buf.is_empty() { &no_ignore } else { &union_buf };
                let name_score = name_similarity_common(
                    &existing.given_name,
                    &incoming.given_name,
                    ignore,
                );
                if name_score < threshold {
                    continue;
                }
                if !birth_compatible(&existing.birth, &incoming.birth) {
                    continue;
                }
                let exact_given = normalize_token(&existing.given_name)
                    == normalize_token(&incoming.given_name);
                // 1-von-2-exakt (oder voller Treffer) ersetzt die
                // Verwandtschafts-Hürde; sonst muss die Verwandtschaft passen.
                // Häufige Familiennamen (z. B. viele Marias) zählen hier nicht.
                let token_hit = shares_exact_token_common(
                    &existing.given_name,
                    &incoming.given_name,
                    ignore,
                );
                let kin_score = kin_similarity(self, existing, incoming, threshold, false);
                if !exact_given && !token_hit && kin_score < threshold {
                    continue;
                }
                let family_score = multi_token_score(
                    &existing.family_name,
                    &incoming.family_name,
                );
                // Nachname unter 50 %: kein automatischer Match (nur wenn
                // überhaupt ein Name bekannt ist — Unwissen ≠ Widerspruch).
                let surname_known = !name_tokens(&existing.family_name).is_empty()
                    || !name_tokens(&incoming.family_name).is_empty();
                if surname_known && family_score < 0.5 {
                    continue;
                }
                candidates.push(MergeCandidate {
                    keep_id: existing.id.clone(),
                    drop_id: incoming.id.clone(),
                    name_score,
                    family_score,
                    birth_match: !existing.birth.trim().is_empty()
                        && !incoming.birth.trim().is_empty(),
                    kin_score,
                });
            }
        }
        candidates.sort_by(|a, b| {
            (b.name_score + b.kin_score + b.family_score)
                .total_cmp(&(a.name_score + a.kin_score + a.family_score))
                .then_with(|| a.keep_id.cmp(&b.keep_id))
                .then_with(|| a.drop_id.cmp(&b.drop_id))
        });
        candidates
    }

    /// Duplikate im eigenen Bestand suchen (Projekt-Button): gleiche
    /// Vergleichsfunktion wie beim Import, aber MIT Verwandten-IDs (starkes
    /// Signal im gleichen Baum). Eltern und eigene Kinder sind nie Duplikate
    /// voneinander. Sortiert absteigend, begrenzt auf 200. `common_min` wie
    /// beim Import-Abgleich (einstellbar, Default 4).
    pub fn find_project_duplicates(&self, threshold: f32, common_min: usize) -> Vec<MergeCandidate> {
        // Eltern-IDs je Person einmalig (Paar-Schleife ist quadratisch).
        let mut parent_ids: HashMap<&str, HashSet<&str>> = HashMap::new();
        for person in &self.people {
            parent_ids.insert(
                person.id.as_str(),
                self.parents_of(&person.id)
                    .iter()
                    .map(|parent| parent.id.as_str())
                    .collect(),
            );
        }
        let mut candidates = Vec::new();
        // Häufige Vornamen je Nachnamengruppe (einmalig, vgl. Import-Matcher).
        let common = self.common_given_tokens(common_min);
        let no_ignore: HashSet<String> = HashSet::new();
        let mut union_buf: HashSet<String> = HashSet::new();
        for (index, first) in self.people.iter().enumerate() {
            for second in &self.people[index + 1..] {
                union_buf.clear();
                for surname in [&first.family_name, &second.family_name] {
                    if let Some(tokens) = common.get(&normalize_token(surname)) {
                        union_buf.extend(tokens.iter().cloned());
                    }
                }
                let ignore: &HashSet<String> =
                    if union_buf.is_empty() { &no_ignore } else { &union_buf };
                let name_score =
                    name_similarity_common(&first.given_name, &second.given_name, ignore);
                if name_score < threshold {
                    continue;
                }
                if !birth_compatible(&first.birth, &second.birth) {
                    continue;
                }
                // Eigenes Kind (beide Richtungen) kann kein Duplikat sein.
                if parent_ids[first.id.as_str()].contains(second.id.as_str())
                    || parent_ids[second.id.as_str()].contains(first.id.as_str())
                {
                    continue;
                }
                let exact_given =
                    normalize_token(&first.given_name) == normalize_token(&second.given_name);
                // Häufige Familiennamen (z. B. viele Marias) zählen hier nicht.
                let token_hit =
                    shares_exact_token_common(&first.given_name, &second.given_name, ignore);
                let kin_score = kin_similarity(self, first, second, threshold, true);
                if !exact_given && !token_hit && kin_score < threshold {
                    continue;
                }
                let family_score = multi_token_score(&first.family_name, &second.family_name);
                // Nachname unter 50 %: kein automatischer Match (nur wenn
                // überhaupt ein Name bekannt ist — Unwissen ≠ Widerspruch).
                let surname_known = !name_tokens(&first.family_name).is_empty()
                    || !name_tokens(&second.family_name).is_empty();
                if surname_known && family_score < 0.5 {
                    continue;
                }
                candidates.push(MergeCandidate {
                    keep_id: first.id.clone(),
                    drop_id: second.id.clone(),
                    name_score,
                    family_score,
                    birth_match: !first.birth.trim().is_empty()
                        && !second.birth.trim().is_empty(),
                    kin_score,
                });
            }
        }
        candidates.sort_by(|a, b| {
            (b.name_score + b.kin_score + b.family_score)
                .total_cmp(&(a.name_score + a.kin_score + a.family_score))
                .then_with(|| a.keep_id.cmp(&b.keep_id))
                .then_with(|| a.drop_id.cmp(&b.drop_id))
        });
        candidates.truncate(200);
        candidates
    }

    /// Manueller Treffer (Review-Dialog): Bestand (`keep`) gegen frisch
    /// Angehängtes (`drop`) — mit echten Ähnlichkeitswerten, aber OHNE
    /// Schwellen (explizite Auswahl sticht die Automatik; die Vornamen-Scores
    /// dämpfen häufige Familiennamen trotzdem für ehrliche Anzeige).
    /// `None` bei unbekannten IDs, vertauschten Seiten oder identischer Person.
    pub fn manual_match_candidate(
        &self,
        keep_id: &str,
        drop_id: &str,
        fresh_ids: &HashSet<String>,
        common_min: usize,
    ) -> Option<MergeCandidate> {
        if keep_id == drop_id || fresh_ids.contains(keep_id) || !fresh_ids.contains(drop_id) {
            return None;
        }
        let keep = self.find(keep_id)?;
        let drop = self.find(drop_id)?;
        // Eigenes Kind (beide Richtungen) kann kein Duplikat sein.
        if self.parents_of(keep_id).iter().any(|parent| parent.id == drop_id)
            || self.parents_of(drop_id).iter().any(|parent| parent.id == keep_id)
        {
            return None;
        }
        let common = self.common_given_tokens(common_min);
        let mut ignore = HashSet::new();
        for surname in [&keep.family_name, &drop.family_name] {
            if let Some(tokens) = common.get(&normalize_token(surname)) {
                ignore.extend(tokens.iter().cloned());
            }
        }
        Some(MergeCandidate {
            keep_id: keep.id.clone(),
            drop_id: drop.id.clone(),
            name_score: name_similarity_common(&keep.given_name, &drop.given_name, &ignore),
            family_score: multi_token_score(&keep.family_name, &drop.family_name),
            birth_match: !keep.birth.trim().is_empty() && !drop.birth.trim().is_empty(),
            kin_score: kin_similarity(self, keep, drop, 0.0, false),
        })
    }

    /// Nächste freie Personen-ID (Lücke-sicher, anders als reines Anzählen).
    fn next_person_id(&self) -> String {
        let mut number = self.people.len() + 1;
        while self.find(&format!("p{number}")).is_some() {
            number += 1;
        }
        format!("p{number}")
    }

    /// Schnell erfasste Person auflösen: gebundene Bestands-ID wiederverwenden
    /// (falls vorhanden), sonst neu anlegen. `None` bei leerer Zeile — und
    /// bei verwaister Bindung (gesetzte Person inzwischen weg): dann lieber
    /// überspringen als unbemerkt ein Duplikat anzulegen.
    fn resolve_quick_person(&mut self, quick: &QuickPerson) -> Option<String> {
        if let Some(id) = quick.bind.as_deref() {
            if self.find(id).is_some() {
                return Some(id.to_string());
            }
            return None;
        }
        if quick.is_empty() {
            return None;
        }
        let id = self.next_person_id();
        let mut person = person(&id, &quick.given, &quick.family, "", quick.gender);
        person.birth = quick.birth.clone();
        person.death = quick.death.clone();
        self.people.push(person);
        Some(id)
    }

    /// Gestapelte Schnell-Blöcke wie beim Import verknüpfen: abwärts je Block
    /// Partner × Referenz plus gemeinsame Kinder (ohne Partner dessen Kinder
    /// allein), aufwärts je Block die Eltern — mit Kind, ersatzweise direkt
    /// an der Referenzperson (Aufwärts-Erfassung ohne Kind-Zeile). Gibt neu
    /// angelegte IDs zurück (verknüpfte Bestands-IDs nicht enthalten).
    /// Abwärts-Blöcke ohne vorhandene Referenzperson werden übersprungen,
    /// ebenso Aufwärts-Blöcke ohne Kind und ohne gültige Referenz. Neue
    /// Personen entstehen in Kopf→Zeilen-Reihenfolge (Vertrag für die
    /// Queue-Zuordnung in `persist_quick_form`).
    pub fn commit_quick_blocks(
        &mut self,
        ref_id: Option<&str>,
        blocks: Vec<(QuickDir, Option<QuickPerson>, Vec<QuickPerson>)>,
    ) -> Vec<String> {
        let mut created = Vec::new();
        for (dir, partner, others) in blocks {
            match dir {
                QuickDir::Down => {
                    let Some(ref_id) = ref_id.filter(|id| self.find(id).is_some()) else {
                        continue;
                    };
                    let before = self.people.len();
                    let partner_id = partner
                        .as_ref()
                        .and_then(|person| self.resolve_quick_person(person));
                    if let Some(partner_id) = &partner_id {
                        self.link_partner(ref_id, partner_id);
                    }
                    let children: Vec<QuickPerson> = others
                        .iter()
                        .filter(|person| {
                            !person.is_empty()
                                || person
                                    .bind
                                    .as_deref()
                                    .is_some_and(|id| self.find(id).is_some())
                        })
                        .cloned()
                        .collect();
                    for other in &children {
                        if let Some(child_id) = self.resolve_quick_person(other) {
                            match &partner_id {
                                Some(partner_id) => self.link_child_to(
                                    Some(ref_id),
                                    Some(partner_id.as_str()),
                                    &child_id,
                                    ChildRelation::Birth,
                                ),
                                None => self.link_child(ref_id, &child_id),
                            }
                        }
                    }
                    created.extend(
                        self.people[before..].iter().map(|person| person.id.clone()),
                    );
                }
                QuickDir::Up => {
                    // Block = ([Kind,] [Eltern...]). Ohne Kind dient die
                    // Referenzperson als Kind (Eltern direkt erfassen).
                    let before = self.people.len();
                    let child_id = partner
                        .as_ref()
                        .and_then(|person| self.resolve_quick_person(person))
                        .or_else(|| {
                            ref_id
                                .filter(|id| self.find(id).is_some())
                                .map(str::to_string)
                        });
                    let Some(child_id) = child_id else {
                        continue;
                    };
                    let parents: Vec<QuickPerson> = others
                        .iter()
                        .filter(|person| {
                            !person.is_empty()
                                || person
                                    .bind
                                    .as_deref()
                                    .is_some_and(|id| self.find(id).is_some())
                        })
                        .cloned()
                        .collect();
                    let mut parent_ids: Vec<String> = Vec::new();
                    for other in &parents {
                        if let Some(parent_id) = self.resolve_quick_person(other) {
                            parent_ids.push(parent_id);
                        }
                    }
                    match parent_ids.as_slice() {
                        [first, second, rest @ ..] => {
                            self.link_child_to(
                                Some(first.as_str()),
                                Some(second.as_str()),
                                &child_id,
                                ChildRelation::Birth,
                            );
                            for extra in rest {
                                self.link_child(extra, &child_id);
                            }
                        }
                        [single] => self.link_child(single, &child_id),
                        [] => {}
                    }
                    if before < self.people.len() {
                        created.extend(
                            self.people[before..].iter().map(|person| person.id.clone()),
                        );
                    }
                }
            }
        }
        created
    }

    /// Familien ohne Eltern UND ohne Kinder auflösen; verwaiste
    /// Beziehungsart-Einträge aufräumen.
    fn cleanup_empty_families(&mut self) {        self.families
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

/// Richtung der Schnellerfassung: abwärts (Partner + Kinder zur
/// Referenzperson) oder aufwärts (Eltern direkt zur Referenzperson, ohne
/// Kind-Zeile; ersatzweise mit explizitem Kind, dann ohne Referenz).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickDir {
    Down,
    Up,
}

/// Eine schnell erfasste Person (Dialog-Zeile): Vorname, Nachname, Geburt,
/// Tod, Geschlecht — plus optional gebundene Bestands-ID (Verknüpfen statt
/// neu anlegen, Merge-Suche im Fenster).
#[derive(Clone, Debug, Default)]
pub struct QuickPerson {
    pub given: String,
    pub family: String,
    pub birth: String,
    pub death: String,
    pub gender: Gender,
    pub bind: Option<String>,
}

impl QuickPerson {
    /// Leer = weder Vor- noch Nachname (wird beim Übernehmen ignoriert).
    pub fn is_empty(&self) -> bool {
        self.given.trim().is_empty() && self.family.trim().is_empty()
    }
}

/// Geburtsjahr aus einem Datumsstring ziehen (letzte 4-stellige Zahl,
/// z. B. „1901" aus „12.03.1901"); leer, wenn kein Jahr enthalten ist.
pub fn birth_year(birth: &str) -> String {
    let mut year = "";
    let mut run_start: Option<usize> = None;
    let bytes = birth.as_bytes();
    // ASCII-Lauf über Ziffernblöcke (Jahre sind immer ASCII-Ziffern).
    for (index, byte) in bytes.iter().enumerate() {
        if byte.is_ascii_digit() {
            if run_start.is_none() {
                run_start = Some(index);
            }
        } else if let Some(start) = run_start.take() {
            if index - start == 4 {
                year = &birth[start..index];
            }
        }
    }
    if let Some(start) = run_start {
        if bytes.len() - start == 4 {
            year = &birth[start..];
        }
    }
    year.to_string()
}

/// Kandidat für selektives Zusammenführen (Review-Dialog): bestehende
/// Person behalten, angehängtes Duplikat einführen und löschen.
#[derive(Clone, Debug)]
pub struct MergeCandidate {
    pub keep_id: String,
    pub drop_id: String,
    /// Vornamens-Deckung 0–1 (Token mit Tippfehler-Toleranz, 1-von-2-exakt).
    pub name_score: f32,
    /// Familiennamens-Deckung 0–1 (Doppelnamen, Schreibfehler) — fließt in
    /// Sortierung und Anzeige ein; in der Automatik blockiert < 50 % bei
    /// bekanntem Namen (manuell weiter möglich, z. B. Ehenamen-Wechsel).
    pub family_score: f32,
    /// Beide Geburtsdaten vorhanden und gleich.
    pub birth_match: bool,
    /// Verwandtschafts-Deckung 0–1 (Anteil ähnlicher Verwandter).
    pub kin_score: f32,
}

/// Vornamen normieren (klein, Umlaute gefaltet) für den Abgleich.
fn normalize_token(text: &str) -> String {
    text.to_lowercase()
        .replace(['ä'], "a")
        .replace(['ö'], "o")
        .replace(['ü'], "u")
        .replace("ß", "ss")
}

/// Normierte Namenstoken (klein, Umlaute gefaltet, nur alphanumerisch).
fn name_tokens(text: &str) -> Vec<String> {
    normalize_token(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

/// Mindestens ein exakt gleiches, nicht ignoriertes Token auf beiden Seiten
/// (z. B. 1 von 2 Vornamen oder ein Teil eines Doppelnachnamens). Mit leerer
/// Ignore-Menge das klassische Verhalten (häufige Familiennamen ausgenommen).
fn shares_exact_token_common(first: &str, second: &str, ignore: &HashSet<String>) -> bool {
    let right = name_tokens(second);
    name_tokens(first).iter().any(|token| {
        !ignore.contains(token) && right.iter().any(|other| token == other)
    })
}

/// Zeichen-Ähnlichkeit 0–1 zweier normierter Token (Levenshtein normiert —
/// Schreibfehler wie Hermine/Hermiene zählen mit).
fn token_similarity(first: &str, second: &str) -> f32 {
    if first == second {
        return 1.0;
    }
    let left: Vec<char> = first.chars().collect();
    let right: Vec<char> = second.chars().collect();
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let mut prev: Vec<usize> = (0..=right.len()).collect();
    for (i, &a) in left.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, &b) in right.iter().enumerate() {
            current.push(
                (prev[j] + usize::from(a != b))
                    .min(prev[j + 1] + 1)
                    .min(current[j] + 1),
            );
        }
        prev = current;
    }
    1.0 - prev[right.len()] as f32 / left.len().max(right.len()) as f32
}

/// Namens-Deckung 0–1 zweier Namensangaben: je Token die beste
/// Zeichen-Deckung der Gegenseite, symmetrisch gemittelt. Ein exakt gleiches
/// Token (z. B. 1 von 2 Vornamen) hebt mindestens auf 0,85 — Doppelnamen und
/// Schreibweisen zählen dadurch mit. Gilt für Vor- wie Nachnamen.
fn multi_token_score(first: &str, second: &str) -> f32 {
    let left = name_tokens(first);
    let right = name_tokens(second);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let mut exact = false;
    let mut sum = 0.0f32;
    for token in &left {
        let mut best = 0.0f32;
        for other in &right {
            if token == other {
                exact = true;
                best = 1.0;
                break;
            }
            best = best.max(token_similarity(token, other));
        }
        sum += best;
    }
    for token in &right {
        let mut best = 0.0f32;
        for other in &left {
            if token == other {
                exact = true;
                best = 1.0;
                break;
            }
            best = best.max(token_similarity(token, other));
        }
        sum += best;
    }
    let mean = sum / (left.len() + right.len()) as f32;
    if exact {
        mean.max(0.85)
    } else {
        mean
    }
}

/// Wie `multi_token_score`, aber exakte Token aus der Ignore-Menge heben
/// nicht auf 0,85 (Wert 1,0 für identische Token bleibt — nur der Boden
/// entfällt, damit „Anna Maria" vs. „Maria Magdalena" über geteiltes
/// „Maria" allein nicht bestehen).
fn multi_token_score_common(first: &str, second: &str, ignore: &HashSet<String>) -> f32 {
    let left = name_tokens(first);
    let right = name_tokens(second);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let mut exact = false;
    let mut sum = 0.0f32;
    for token in &left {
        let mut best = 0.0f32;
        for other in &right {
            if token == other {
                if !ignore.contains(token) {
                    exact = true;
                }
                best = 1.0;
                break;
            }
            best = best.max(token_similarity(token, other));
        }
        sum += best;
    }
    for token in &right {
        let mut best = 0.0f32;
        for other in &left {
            if token == other {
                if !ignore.contains(token) {
                    exact = true;
                }
                best = 1.0;
                break;
            }
            best = best.max(token_similarity(token, other));
        }
        sum += best;
    }
    let mean = sum / (left.len() + right.len()) as f32;
    if exact {
        mean.max(0.85)
    } else {
        mean
    }
}

/// Vornamens-Deckung 0–1 (Toleranz für Tippfehler und Mehrfachnamen).
fn name_similarity(first: &str, second: &str) -> f32 {
    multi_token_score(first, second)
}

/// Vornamens-Deckung mit Ignore-Menge: darin enthaltene Token (häufige Namen
/// der beteiligten Familien, siehe `common_given_tokens`) geben weder den
/// Exakt-Boost noch einen 1-von-2-Treffer — die reine Zeichen-Deckung zählt
/// weiter, damit identische Namen (Mittel 1,0) bestehen bleiben.
fn name_similarity_common(first: &str, second: &str, ignore: &HashSet<String>) -> f32 {
    multi_token_score_common(first, second, ignore)
}

/// Geburtsdaten verträglich: Nur wenn BEIDE eins haben, müssen sie gleich
/// sein (vergleichbar geparst).
fn birth_compatible(first: &str, second: &str) -> bool {
    if first.trim().is_empty() || second.trim().is_empty() {
        return true;
    }
    match (parse_birth_date(first), parse_birth_date(second)) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// Verwandtschafts-Deckung 0–1: Anteil der Verwandten (Eltern, Kinder,
/// Partner) von `first`, zu denen `second` einen ähnlichen Verwandten hat
/// (Vornamens-Deckung ≥ Schwelle, Geburtsdaten verträglich). Ohne Verwandte
/// auf beiden Seiten 0,0 (dann muss der Vorname exakt gleichen). Mit
/// `use_ids` (gleicher Datenbestand, z. B. Projekt-Duplikate) zählen
/// identische Verwandten-IDs stark mit (70 %) neben der Namensdeckung (30 %);
/// beim Import spielen IDs keine Rolle (getrennte Bestände).
fn kin_similarity(
    data: &TreeData,
    first: &Person,
    second: &Person,
    threshold: f32,
    use_ids: bool,
) -> f32 {
    let relatives = |person: &Person| {
        let mut ids: Vec<&str> = data
            .parents_of(&person.id)
            .into_iter()
            .chain(data.children_of(&person.id))
            .chain(data.partners_of(&person.id))
            .map(|relative| relative.id.as_str())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    };
    let left = relatives(first);
    let right = relatives(second);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let similar = left
        .iter()
        .filter(|id| {
            let Some(relative) = data.find(id) else {
                return false;
            };
            right.iter().any(|other| {
                data.find(other).is_some_and(|candidate| {
                    name_similarity(&relative.given_name, &candidate.given_name) >= threshold
                        && birth_compatible(&relative.birth, &candidate.birth)
                })
            })
        })
        .count() as f32;
    let by_name = 2.0 * similar / (left.len() + right.len()) as f32;
    if !use_ids {
        return by_name;
    }
    let common = left.iter().filter(|id| right.contains(id)).count() as f32;
    let by_id = 2.0 * common / (left.len() + right.len()) as f32;
    0.7 * by_id + 0.3 * by_name
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
        documents: Vec::new(),
        sources: Vec::new(),
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
/// Monatsnamen/-kürzel (deutsch + englisch) für den Datumsdolmetscher.
const MONTH_ALIASES: [&[&str]; 12] = [
    &["jan", "januar", "january"],
    &["feb", "februar", "february"],
    &["mar", "mär", "märz", "march"],
    &["apr", "april"],
    &["mai", "may"],
    &["jun", "juni", "june"],
    &["jul", "juli", "july"],
    &["aug", "august"],
    &["sep", "september"],
    &["okt", "oct", "oktober", "october"],
    &["nov", "november"],
    &["dez", "dec", "dezember", "december"],
];

/// Kurze deutsche Monatsnamen für die normierte Anzeige.
const MONTHS_DE: [&str; 12] = [
    "Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez",
];

/// Datum für die Baumanzeige normieren: vollständige Daten als „DD Mnt YYYY",
/// Monat+Jahr als „Mnt YYYY". Andere Angaben (nur Jahr, „um …", leer) bleiben
/// unverändert, damit keine Information verloren geht.
pub fn normalize_date(date: &str) -> String {
    let original = date.trim();
    if original.is_empty() {
        return String::new();
    }
    let lower = original.to_lowercase();
    let tokens: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.is_empty() {
        return original.to_string();
    }

    let mut year = None;
    let mut month = None;
    let mut day = None;
    for token in &tokens {
        let mut found_month = false;
        for (idx, aliases) in MONTH_ALIASES.iter().enumerate() {
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
            if (100..=3000).contains(&num) {
                year = Some(num);
            } else if (1..=31).contains(&num) {
                if day.is_none() {
                    day = Some(num);
                } else if month.is_none() && num <= 12 {
                    month = Some(num);
                }
            }
        }
    }
    // Rein numerische Tripel (TT.MM.JJJJ bzw. JJJJ-MM-TT) korrekt zuordnen.
    if tokens.len() == 3 {
        if let (Ok(n1), Ok(n2), Ok(n3)) = (
            tokens[0].parse::<i32>(),
            tokens[1].parse::<i32>(),
            tokens[2].parse::<i32>(),
        ) {
            if (100..=3000).contains(&n3) {
                year = Some(n3);
                month = Some(n2);
                day = Some(n1);
            } else if (100..=3000).contains(&n1) {
                year = Some(n1);
                month = Some(n2);
                day = Some(n3);
            }
        }
    }

    match (year, month, day) {
        (Some(y), Some(m), Some(d)) if (1..=12).contains(&m) && (1..=31).contains(&d) => {
            format!("{d:02} {} {y}", MONTHS_DE[(m - 1) as usize])
        }
        (Some(y), Some(m), _) if (1..=12).contains(&m) => {
            format!("{} {y}", MONTHS_DE[(m - 1) as usize])
        }
        _ => original.to_string(),
    }
}

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

    let month_names = MONTH_ALIASES;

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
    fn normalizes_tree_dates() {
        assert_eq!(normalize_date("29 Feb 1876"), "29 Feb 1876");
        assert_eq!(normalize_date("29.02.1876"), "29 Feb 1876");
        assert_eq!(normalize_date("1876-02-29"), "29 Feb 1876");
        assert_eq!(normalize_date("5. März 1900"), "05 Mär 1900");
        assert_eq!(normalize_date("Feb 1876"), "Feb 1876");
        assert_eq!(normalize_date("1876"), "1876");
        assert_eq!(normalize_date("um 1850"), "um 1850");
        assert_eq!(normalize_date(""), "");
    }

    #[test]
    fn call_name_short_name() {
        let mut p = person("x", "Jürgen Hans", "Bauke", "", Gender::Male);
        p.call_name = "Jürgen".into();
        assert_eq!(p.given_short(), "Hans Jürgen");
        assert_eq!(p.display_name_short(), "Hans Jürgen Bauke");
        p.call_name = "Hans".into();
        assert_eq!(p.given_short(), "Jürgen Hans");
        let mut q = person("y", "Anna", "Muster", "", Gender::Female);
        q.call_name = "Anna".into();
        assert_eq!(q.given_short(), "Anna");
    }

    #[test]
    fn lifespan_combines_full_birth_and_death_dates() {
        let mut p = person("x", "Max", "Muster", "16 Jun 1998", Gender::Male);
        assert_eq!(p.lifespan_short(), "16 Jun 1998");
        p.death = "2 Sep 2022".into();
        assert_eq!(p.lifespan_short(), "16 Jun 1998 – 02 Sep 2022");
        assert_eq!(p.death_short(), "02 Sep 2022");
        let q = person("y", "Anna", "Muster", "", Gender::Female);
        assert_eq!(q.lifespan_short(), "Unbekannt");
    }

    #[test]
    fn standard_events_are_ensured_and_synced() {
        let mut p = person("x", "Max", "Muster", "1876", Gender::Male);
        p.death = "1940".into();
        p.ensure_standard_events();
        assert_eq!(p.events.len(), 2);
        assert_eq!(p.events[0].kind, EventKind::Birth);
        assert_eq!(p.events[0].date, "1876");
        assert_eq!(p.events[1].kind, EventKind::Death);
        assert_eq!(p.events[1].date, "1940");
        // Kurzfelder werden aus den Standard-Ereignissen zurückgeschrieben.
        p.events[0].date = "01 Jan 1876".into();
        p.sync_standard_fields();
        assert_eq!(p.birth, "01 Jan 1876");
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
    fn merge_match_scores_partial_names() {
        // 2 von 3 Token exakt → hoch (exakt-Boost).
        assert!(name_similarity("Johann Christoph", "Johann Christoph Friedrich") >= 0.85);
        assert!(name_similarity("Hans", "Peter") < 0.5);
        assert_eq!(name_similarity("", "Peter"), 0.0);
        assert!(birth_compatible("1900", ""));
        assert!(birth_compatible("10.04.1957", "10.04.1957"));
        assert!(!birth_compatible("1900", "1901"));
    }

    #[test]
    fn name_scores_tolerate_typos_and_partial_names() {
        // Schreibfehler (Hermine/Hermiene, Distanz 1) zählt mit.
        assert!(token_similarity("hermine", "hermiene") >= 0.8);
        assert_eq!(token_similarity("hans", "hans"), 1.0);
        assert_eq!(token_similarity("", "hans"), 0.0);
        // 1 von 2 Vornamen exakt → mindestens 0,85.
        assert!(name_similarity("Anna Maria", "Anna") >= 0.85);
        assert!(name_similarity("Anna", "Anna Maria") >= 0.85);
        // Doppelnachname teils gleich → mindestens 0,85.
        assert!(multi_token_score("Meier Schulze", "Schulze") >= 0.85);
        assert!(multi_token_score("Meier", "Mayer") > 0.5);
        assert!(shares_exact_token_common("Anna Maria", "Maria", &HashSet::new()));
        assert!(!shares_exact_token_common("Anna", "Peter", &HashSet::new()));
        // Ignoriertes Token (häufiger Familienname) löst keinen Treffer aus.
        let ignore: HashSet<String> = ["maria".to_string()].into_iter().collect();
        assert!(!shares_exact_token_common("Anna Maria", "Maria Magdalena", &ignore));
        assert!(multi_token_score_common("Anna Maria", "Maria Magdalena", &ignore) < 0.8);
        assert!(multi_token_score("Anna Maria", "Maria Magdalena") >= 0.85);
    }

    fn merge_tree() -> TreeData {
        let mut data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("p1", "Johann", "Bauke", "1900", Gender::Male),
                person("c1", "Johann Christoph", "Bauke", "1925", Gender::Male),
            ],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        data.link_child("p1", "c1");
        data
    }

    #[test]
    fn finds_merge_candidate_by_name_birth_and_kin() {
        let mut data = merge_tree();
        let mut imported = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("x1", "Johann", "Bauke", "", Gender::Male),
                person("x2", "Johann Christoph Friedrich", "Bauke", "1925", Gender::Male),
            ],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        imported.link_child("x1", "x2");
        let fresh = data.append_import(imported);
        assert_eq!(fresh.len(), 2);
        let fresh_set: HashSet<String> = fresh.into_iter().collect();
        let candidates = data.find_merge_candidates(&fresh_set, 0.8, 2);
        // p1↔x1 (exakt). c1↔x2 (Geburt + Verwandtschaft); c1↔x1 entfällt:
        // „Johann" ist bei den Baukes doppelt belegt (p1, c1) und trägt allein
        // keinen 1-von-2-Treffer mehr.
        assert!(candidates.iter().any(|c| c.keep_id == "p1"));
        assert_eq!(candidates.iter().filter(|c| c.keep_id == "c1").count(), 1);
        assert!(candidates.iter().any(|c| c.keep_id == "c1" && c.kin_score >= 0.8));
        assert!(candidates.iter().any(|c| c.birth_match));
    }

    #[test]
    fn common_given_names_skip_exact_boost_in_family() {
        // Familie mit vielen Marias: geteiltes „Maria" allein trägt keinen
        // Match — weder m1↔m2 noch m2↔m3; identische volle Vornamen schon.
        let data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("m1", "Anna Maria", "Richter", "", Gender::Female),
                person("m2", "Maria Magdalena", "Richter", "", Gender::Female),
                person("m3", "Maria Elisabeth", "Richter", "", Gender::Female),
                person("h1", "Hans Georg", "Richter", "", Gender::Male),
                person("h2", "Hans Georg", "Richter", "", Gender::Male),
            ],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        let dups = data.find_project_duplicates(0.8, 2);
        let pair = |a: &str, b: &str| {
            dups.iter().any(|c| {
                (c.keep_id == a && c.drop_id == b) || (c.keep_id == b && c.drop_id == a)
            })
        };
        assert!(!pair("m1", "m2"));
        assert!(!pair("m2", "m3"));
        assert!(!pair("m1", "m3"));
        assert!(pair("h1", "h2"));
        // Schwelle 4 bei nur 3 Marias: „Maria" zählt wieder → Paare da.
        let dups4 = data.find_project_duplicates(0.8, 4);
        let pair4 = |a: &str, b: &str| {
            dups4.iter().any(|c| {
                (c.keep_id == a && c.drop_id == b) || (c.keep_id == b && c.drop_id == a)
            })
        };
        assert!(pair4("m1", "m2"));
        assert!(pair4("h1", "h2"));
    }

    #[test]
    fn rejects_mismatched_birth_and_keeps_unrelated() {
        let mut data = merge_tree();
        let mut imported = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("x1", "Johann", "Bauke", "1901", Gender::Male),
                person("x9", "Peter", "Fremd", "1900", Gender::Male),
            ],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        imported.link_child("x1", "x9");
        let fresh = data.append_import(imported);
        let fresh_set: HashSet<String> = fresh.into_iter().collect();
        // x1 scheitert am Geburtsdatum, x9 am Vornamen.
        assert!(data.find_merge_candidates(&fresh_set, 0.8, 2).is_empty());
    }

    #[test]
    fn family_name_below_half_blocks_auto_match() {
        let mut data = merge_tree();
        let mut imported = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                // Gleicher Vorname + Geburt wie p1, aber fremder Nachname.
                person("x1", "Johann", "Wendt", "1900", Gender::Male),
                // Gleicher Vorname + Geburt wie c1, Nachname einseitig leer.
                person("x2", "Johann", "", "1925", Gender::Male),
                // Kontrolle: passender Nachname → Treffer auf p1.
                person("x3", "Johann", "Bauke", "1900", Gender::Male),
            ],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        let fresh = data.append_import(imported);
        let fresh_set: HashSet<String> = fresh.into_iter().collect();
        let candidates = data.find_merge_candidates(&fresh_set, 0.8, 2);
        // Frisch-IDs werden remappt (m…) — Treffer über die Drop-Daten erkennen.
        let drop_of = |c: &MergeCandidate| data.find(&c.drop_id);
        // Fremder Nachname (0 %) → kein Match.
        assert!(candidates.iter().all(|c| {
            drop_of(c).map(|p| p.family_name.as_str()) != Some("Wendt")
        }));
        // Einseitig fehlender Nachname + passender Rest → trotzdem keins.
        assert!(candidates.iter().all(|c| {
            let person = drop_of(c);
            !(person.map(|p| p.given_name.as_str()) == Some("Johann")
                && person.map(|p| p.birth.as_str()) == Some("1925")
                && person.map(|p| p.family_name.as_str()) == Some(""))
        }));
        // Kontrolle: passender Nachname → Treffer auf p1.
        assert!(candidates.iter().any(|c| c.keep_id == "p1"
            && drop_of(c).map(|p| (
                p.given_name.as_str(),
                p.family_name.as_str(),
                p.birth.as_str()
            )) == Some(("Johann", "Bauke", "1900"))));
    }

    #[test]
    fn manual_match_accepts_explicit_pair_and_rejects_sides() {
        let mut data = merge_tree();
        let imported = TreeData {
            project: ProjectMetadata::default(),
            people: vec![person("x1", "Johann", "Bauke", "1901", Gender::Male)],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        let fresh_set: HashSet<String> = data.append_import(imported).into_iter().collect();
        assert_eq!(fresh_set.len(), 1);
        let drop = fresh_set.iter().next().unwrap().clone();
        // Gültig: Bestand (p1) gegen Neu — trotz abweichender Geburt, an der
        // die Automatik scheitern würde.
        let candidate = data.manual_match_candidate("p1", &drop, &fresh_set, 2).unwrap();
        assert_eq!(candidate.keep_id, "p1");
        assert_eq!(candidate.drop_id, drop);
        // Seiten vertauscht, identisch, beidseitig Bestand oder unbekannt.
        assert!(data.manual_match_candidate(&drop, "p1", &fresh_set, 2).is_none());
        assert!(data.manual_match_candidate("p1", "p1", &fresh_set, 2).is_none());
        assert!(data.manual_match_candidate("p1", "c1", &fresh_set, 2).is_none());
        assert!(data.manual_match_candidate("nobody", &drop, &fresh_set, 2).is_none());
        assert!(data.manual_match_candidate("p1", "nobody", &fresh_set, 2).is_none());
    }

    fn quick(
        given: &str,
        family: &str,
        birth: &str,
        death: &str,
        gender: Gender,
    ) -> QuickPerson {
        QuickPerson {
            given: given.into(),
            family: family.into(),
            birth: birth.into(),
            death: death.into(),
            gender,
            bind: None,
        }
    }

    #[test]
    fn quick_commit_links_partner_and_children_down() {
        let mut data = merge_tree();
        data.people.push(person("r", "Ref", "Er", "", Gender::Female));
        let blocks = vec![(
            QuickDir::Down,
            Some(quick("Pam", "Er", "1970", "", Gender::Male)),
            vec![
                quick("Kid", "Er", "2000", "", Gender::Unknown),
                quick("", "", "", "", Gender::Unknown),
            ],
        )];
        let created = data.commit_quick_blocks(Some("r"), blocks);
        assert_eq!(created.len(), 2);
        let partner = &created[0];
        let child = &created[1];
        assert!(data.partners_of("r").iter().any(|p| &p.id == partner));
        let parents: Vec<String> = data
            .parents_of(child)
            .iter()
            .map(|parent| parent.id.clone())
            .collect();
        assert!(parents.contains(&"r".to_string()));
        assert!(parents.contains(partner));
        assert_eq!(data.find(child).unwrap().birth, "2000");
    }

    #[test]
    fn quick_commit_up_links_parents_to_reference_without_child() {
        let mut data = merge_tree();
        data.people.push(person("r", "Ref", "Er", "", Gender::Female));
        // Mit Kind: klassisch (Kind zuerst angelegt, dann die Eltern).
        let blocks = vec![(
            QuickDir::Up,
            Some(quick("Kid", "Er", "2000", "", Gender::Unknown)),
            vec![
                quick("Vater", "Er", "", "", Gender::Male),
                quick("Mutter", "Er", "", "", Gender::Female),
                quick("", "", "", "", Gender::Unknown),
            ],
        )];
        let created = data.commit_quick_blocks(None, blocks);
        assert_eq!(created.len(), 3);
        let child = data
            .find(&created[0])
            .expect("Kind zuerst angelegt");
        assert_eq!(child.given_name, "Kid");
        let parents: Vec<String> = data
            .parents_of(&child.id)
            .iter()
            .map(|parent| parent.id.clone())
            .collect();
        assert_eq!(parents.len(), 2);
        assert_eq!(parents, created[1..].to_vec());
        // Ohne Kind: Eltern direkt an die Referenz (Aufwärts-Erfassung ohne
        // Kind-Zeile — die Referenz ist das Kind).
        let linked = data.commit_quick_blocks(
            Some("r"),
            vec![(
                QuickDir::Up,
                None,
                vec![
                    quick("Vater", "Er", "", "", Gender::Male),
                    quick("Mutter", "Er", "", "", Gender::Female),
                ],
            )],
        );
        assert_eq!(linked.len(), 2);
        let ref_parents: Vec<String> = data
            .parents_of("r")
            .iter()
            .map(|parent| parent.id.clone())
            .collect();
        assert_eq!(ref_parents, linked);
        // Ohne Kind und ohne Referenz: Block übersprungen.
        let skipped = data.commit_quick_blocks(
            None,
            vec![(
                QuickDir::Up,
                Some(quick("", "", "", "", Gender::Unknown)),
                vec![quick("Vater", "Er", "", "", Gender::Male)],
            )],
        );
        assert!(skipped.is_empty());
        // Ohne Referenzperson passiert bei Abwärts nichts.
        assert!(data.commit_quick_blocks(None, vec![]).is_empty());
        assert!(data
            .commit_quick_blocks(
                Some("weg"),
                vec![(QuickDir::Down, Some(quick("X", "Y", "", "", Gender::Male)), vec![])]
            )
            .is_empty());
    }

    #[test]
    fn quick_commit_skips_fully_empty_blocks() {
        let mut data = merge_tree();
        data.people.push(person("r", "Ref", "Er", "", Gender::Female));
        let before = data.people.len();
        // Komplett leere Blöcke (abwärts wie aufwärts) erzeugen keine Person.
        let created = data.commit_quick_blocks(
            Some("r"),
            vec![
                (
                    QuickDir::Down,
                    Some(quick("", "", "", "", Gender::Unknown)),
                    vec![quick("", "", "", "", Gender::Unknown)],
                ),
                (
                    QuickDir::Up,
                    None,
                    vec![quick("", "", "", "", Gender::Unknown)],
                ),
            ],
        );
        assert!(created.is_empty());
        assert_eq!(data.people.len(), before);
        // Auch nur-Datum ohne Namen bleibt ohne Person (namenlos sinnlos).
        let dated = data.commit_quick_blocks(
            Some("r"),
            vec![(
                QuickDir::Down,
                None,
                vec![quick("", "", "1900", "", Gender::Male)],
            )],
        );
        assert!(dated.is_empty());
        assert_eq!(data.people.len(), before);
    }

    #[test]
    fn quick_commit_skips_stale_bind_without_duplicate() {
        let mut data = merge_tree();
        data.people.push(person("r", "Ref", "Er", "", Gender::Female));
        let before = data.people.len();
        // Gesetzte, aber inzwischen verschwundene Person: überspringen statt
        // unbemerkt ein Duplikat anzulegen (das käme sonst auf die Queue).
        let mut ghost = quick("Ghost", "Er", "1900", "", Gender::Male);
        ghost.bind = Some("weg".into());
        let created = data.commit_quick_blocks(
            Some("r"),
            vec![(QuickDir::Down, Some(ghost), vec![])],
        );
        assert!(created.is_empty());
        assert_eq!(data.people.len(), before);
        assert!(data.partners_of("r").is_empty());
    }

    #[test]
    fn gender_for_given_name_uses_clear_majority() {
        let mut data = merge_tree();
        // Johann: 2× M aus der Fixture + 1× M + 1× F dazu = 3:1 → Male.
        data.people.push(person("m1", "Johann", "X", "", Gender::Male));
        data.people.push(person("f1", "Johann", "X", "", Gender::Female));
        assert_eq!(data.gender_for_given_name("Johann"), Some(Gender::Male));
        assert_eq!(data.gender_for_given_name("JOHANN"), Some(Gender::Male));
        // Wilma: 2× F → Female.
        data.people.push(person("w1", "Wilma", "X", "", Gender::Female));
        data.people.push(person("w2", "Wilma Wagner", "X", "", Gender::Female));
        assert_eq!(data.gender_for_given_name("Wilma"), Some(Gender::Female));
        // Alex: 1:1 → Gleichstand → None.
        data.people.push(person("a1", "Alex", "X", "", Gender::Male));
        data.people.push(person("a2", "Alex", "X", "", Gender::Female));
        assert_eq!(data.gender_for_given_name("Alex"), None);
        // Nur Unbekannt belegt → None; Unbekannt leer → None.
        data.people.push(person("u1", "Kaspar", "X", "", Gender::Unknown));
        assert_eq!(data.gender_for_given_name("Kaspar"), None);
        assert_eq!(data.gender_for_given_name("Niemand"), None);
        assert_eq!(data.gender_for_given_name(""), None);
        assert_eq!(data.gender_for_given_name("   "), None);
        // Zweitnamen zählen mit: „Johann Hartmut" (M) stimmt für „Hartmut".
        data.people.push(person("h1", "Johann Hartmut", "X", "", Gender::Male));
        data.people.push(person("h2", "Hartmut", "X", "", Gender::Male));
        assert_eq!(data.gender_for_given_name("Hartmut"), Some(Gender::Male));
        assert_eq!(data.gender_for_given_name("Hartmut Hans"), Some(Gender::Male));
    }

    #[test]
    fn birth_year_extracts_last_four_digit_group() {
        assert_eq!(birth_year("12.03.1901"), "1901");
        assert_eq!(birth_year("1901"), "1901");
        assert_eq!(birth_year("um 1870"), "1870");
        assert_eq!(birth_year("1901-1905"), "1905");
        assert_eq!(birth_year("03.1901"), "1901");
        assert_eq!(birth_year(""), "");
        assert_eq!(birth_year("unbekannt"), "");
        assert_eq!(birth_year("12.03.99"), "");
    }

    #[test]
    fn quick_commit_reuses_bound_person() {
        let mut data = merge_tree();
        data.people.push(person("r", "Ref", "Er", "", Gender::Female));
        let before = data.people.len();
        let mut partner = quick("Pam", "Er", "1970", "", Gender::Male);
        partner.bind = Some("p1".into());
        let blocks = vec![(
            QuickDir::Down,
            Some(partner),
            vec![quick("Kid", "Er", "2000", "", Gender::Unknown)],
        )];
        let created = data.commit_quick_blocks(Some("r"), blocks);
        // Nur das Kind neu, der Partner (p1) wiederverwendet + verknüpft.
        assert_eq!(created.len(), 1);
        assert_eq!(data.people.len(), before + 1);
        assert!(data.partners_of("r").iter().any(|p| p.id == "p1"));
        assert!(data
            .parents_of(&created[0])
            .iter()
            .any(|parent| parent.id == "p1"));
    }

    #[test]
    fn project_duplicates_use_relative_ids() {
        let mut data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("p1", "Johann", "Bauke", "1900", Gender::Male),
                person("p2", "Johann", "Bauke", "1900", Gender::Male),
                person("f", "Vater", "Bauke", "1870", Gender::Male),
                person("s", "Johann", "Bauke", "1925", Gender::Male),
                person("s2", "Johann", "Bauke", "1900", Gender::Male),
                person("x", "Peter", "Fremd", "1900", Gender::Male),
            ],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        // p1 und p2 teilen sich Vater f (gleiche IDs) → Treffer.
        data.link_child("f", "p1");
        data.link_child("f", "p2");
        data.link_child("p1", "s");
        data.link_child("p1", "s2");
        let hits = data.find_project_duplicates(0.8, 2);
        let pair = |a: &str, b: &str| {
            hits.iter().any(|hit| {
                (hit.keep_id == a && hit.drop_id == b) || (hit.keep_id == b && hit.drop_id == a)
            })
        };
        // Echte Dublette drin, eigenes Kind (p1/s2) trotz gleichen Jahrs draußen.
        // Onkel/Neffe (p2/s2, gleicher Name + Jahr) bleibt Prüf-Treffer.
        assert!(pair("p1", "p2"));
        assert!(!pair("p1", "s2"));
        assert!(pair("p2", "s2"));
        assert_eq!(hits[0].family_score, 1.0);
        // Namensvetter mit anderer Geburt (Vater/Sohn-Falle) und Fremde fallen raus.
        assert!(hits.iter().all(|hit| hit.drop_id != "s" && hit.keep_id != "s"));
        assert!(hits.iter().all(|hit| hit.drop_id != "x" && hit.keep_id != "x"));
    }

    #[test]
    fn append_import_remaps_colliding_ids() {
        let mut data = merge_tree();
        let mut imported = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("p1", "Anna", "Bauke", "", Gender::Female),
                person("c9", "Kind", "Bauke", "", Gender::Unknown),
            ],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        imported.link_child("p1", "c9");
        let fresh = data.append_import(imported);
        assert_eq!(fresh.len(), 2);
        assert!(!fresh.contains(&"p1".to_string()));
        let ids: HashSet<&str> = data.people.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids.len(), data.people.len());
        // Familienverweise folgen der Umbenennung (Eltern + Kind frisch).
        let (parent, child) = (&fresh[0], &fresh[1]);
        assert!(data.families.iter().any(|f| f.children.contains(child)
            && (f.parent_a.as_deref() == Some(parent.as_str())
                || f.parent_b.as_deref() == Some(parent.as_str()))));
    }

    #[test]
    fn merge_persons_rewires_families_and_fills_gaps() {
        let mut data = merge_tree();
        let mut extra = person("m9", "Johann", "", "", Gender::Unknown);
        extra.death = "1970".into();
        extra.notes = "Notiz".into();
        data.people.push(extra);
        data.people.push(person("k9", "Kind", "Neu", "", Gender::Unknown));
        data.link_child("m9", "k9");
        data.merge_persons("p1", "m9");
        assert!(data.find("m9").is_none());
        let kept = data.find("p1").unwrap();
        assert_eq!(kept.death, "1970");
        assert_eq!(kept.notes, "Notiz");
        assert_eq!(kept.family_name, "Bauke");
        // Kind hängt jetzt an der behaltenen Person, keine Selbst-Paare.
        let parents: Vec<&str> = data
            .parents_of("k9")
            .iter()
            .map(|parent| parent.id.as_str())
            .collect();
        assert_eq!(parents, vec!["p1"]);
        assert!(data.families.iter().all(|f| f.parent_a != f.parent_b));
    }

    #[test]
    fn merge_choice_respects_birth_and_death_side() {
        let mut data = merge_tree();
        let mut extra = person("m9", "Johann", "", "", Gender::Unknown);
        extra.birth = "1901".into();
        extra.birth_place = "Neuort".into();
        extra.death = "1970".into();
        data.people.push(extra);
        // Neues Todesdatum übernehmen, altes Geburtsdatum behalten
        // (der leere Geburtsort wird als Lücke aus der neuen Person gefüllt).
        data.apply_merge_choice("p1", "m9", false, true);
        let kept = data.find("p1").unwrap();
        assert_eq!(kept.birth, "1900");
        assert_eq!(kept.birth_place, "Neuort");
        assert_eq!(kept.death, "1970");
        assert!(data.find("m9").is_none());
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
