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
    /// Notiz zum Ereignis (Kontextmenü je Eintrag, Anzeige darunter).
    /// `None` = keine (Eintrag ausgeblendet), `Some` = vorhanden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Quellen zum Ereignis (Kontextmenü je Eintrag, Anzeige darunter).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceEntry>,
    /// Sicherheit des Ereignisses (Default ungesetzt).
    #[serde(default, skip_serializing_if = "Certainty::is_unset")]
    pub certainty: Certainty,
}

impl Event {
    /// Leeres Ereignis der angegebenen Art.
    pub fn new(kind: EventKind) -> Self {
        Self {
            kind,
            date: String::new(),
            place: String::new(),
            description: String::new(),
            notes: None,
            sources: Vec::new(),
            certainty: Certainty::Unset,
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
    /// Alternative Namen (Alias-/Geburts-/Ehenamen …): jeder Eintrag trägt
    /// dieselben Bestandteile wie der Hauptname (Gramps-Prinzip).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alt_names: Vec<AlternativeName>,
    /// Ereignisse (Geburt, Tod, Heirat, Beruf …).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
    /// Notizen je Info-Feld (Schlüssel wie "title", "birth", "death" …):
    /// Kontextmenü je Eintrag, Anzeige darunter.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub field_notes: HashMap<String, String>,
    /// Quellen je Info-Feld (Schlüssel wie oben).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub field_sources: HashMap<String, Vec<SourceEntry>>,
    /// Sicherheit je Info-Feld (Schlüssel wie oben, Default ungesetzt).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub field_certainty: HashMap<String, Certainty>,
}

/// Alternativer Name einer Person (Alias-/Geburts-/Ehename …): dieselben
/// Bestandteile wie der Hauptname, plus Art und Herkunft je Eintrag.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AlternativeName {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub given_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub family_name: String,
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
}

impl AlternativeName {
    /// Leer = kein Bestandteil gefüllt.
    pub fn is_empty(&self) -> bool {
        self.given_name.trim().is_empty()
            && self.family_name.trim().is_empty()
            && self.title.trim().is_empty()
            && self.nick_name.trim().is_empty()
            && self.call_name.trim().is_empty()
            && self.name_prefix.trim().is_empty()
            && self.surname_prefix.trim().is_empty()
            && self.suffix.trim().is_empty()
            && self.name_type.trim().is_empty()
            && self.name_origin.trim().is_empty()
    }

    /// Alle Namensfelder trimmen (innenliegende Leerzeichen bleiben).
    pub fn strip(&mut self) {
        self.given_name = self.given_name.trim().to_string();
        self.family_name = self.family_name.trim().to_string();
        self.title = self.title.trim().to_string();
        self.nick_name = self.nick_name.trim().to_string();
        self.call_name = self.call_name.trim().to_string();
        self.name_prefix = self.name_prefix.trim().to_string();
        self.surname_prefix = self.surname_prefix.trim().to_string();
        self.suffix = self.suffix.trim().to_string();
        self.name_type = self.name_type.trim().to_string();
        self.name_origin = self.name_origin.trim().to_string();
    }

    /// Anzeigename ("Vorname Nachname", sonst erster gefüllter Bestandteil).
    pub fn display(&self) -> String {
        let combined = format!("{} {}", self.given_name, self.family_name);
        let combined = combined.trim();
        if !combined.is_empty() {
            return combined.to_string();
        }
        for part in [
            &self.nick_name,
            &self.call_name,
            &self.title,
            &self.name_type,
        ] {
            if !part.trim().is_empty() {
                return part.trim().to_string();
            }
        }
        String::new()
    }
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
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SourceEntry {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Fundstelle/Seite (Gramps-`page` der Citation).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// Optionale Mediendatei als Beleg (relativer Pfad wie Galerie, aber
    /// separat verwaltet: nicht in der Galerie, erreichbar über das
    /// Quellen-Kontextmenü). Leer = keine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
}

impl SourceEntry {
    /// Leer = weder Titel noch Detail noch Medium.
    pub fn is_empty(&self) -> bool {
        self.title.trim().is_empty() && self.detail.trim().is_empty() && self.media.is_none()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Gender {
    Male,
    Female,
    #[default]
    Unknown,
}

/// Sicherheit/Beleggrad einer Info (Kontextmenü je Eintrag): von mündlicher
/// Info bis zum beglaubigten Dokument, Default ungesetzt. Sortierung =
/// Warnrang (Warnstufe hebt alles bis zur Stufe hervor).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Certainty {
    /// Noch nicht bewertet (Default).
    #[default]
    Unset,
    /// Mündliche Info (Hörensagen).
    Oral,
    /// Dokument (schriftlich, unbeglaubigt).
    Document,
    /// Beglaubigtes Dokument (höchster Beleg).
    Certified,
}

impl Certainty {
    /// Anzeigename für Menüs und Tooltips.
    pub fn label(&self) -> &'static str {
        match self {
            Certainty::Unset => "Ungesetzt",
            Certainty::Oral => "Mündliche Info",
            Certainty::Document => "Dokument",
            Certainty::Certified => "Beglaubigtes Dokument",
        }
    }

    /// Für `skip_serializing_if`: Unbewertetes nicht schreiben.
    pub fn is_unset(&self) -> bool {
        matches!(self, Certainty::Unset)
    }
}

/// Warnstufe greift? Hebt alles bis zur Stufe hervor (Aus = nichts) — als
/// Handlungsbedarf-Signal für Baum und Seitenleiste. Unbewertetes zählt mit,
/// sobald die Stufe es einschließt.
pub fn certainty_warns(certainty: Certainty, warn_until: Option<Certainty>) -> bool {
    match warn_until {
        None => false,
        Some(max) => certainty <= max,
    }
}

impl Person {
    /// Namen beim Speichern strippen (Leerzeichen weg): alle Namensfelder
    /// trimmen (innenliegende bleiben erhalten).
    pub fn strip_names(&mut self) {
        self.given_name = self.given_name.trim().to_string();
        self.family_name = self.family_name.trim().to_string();
        self.name = self.name.trim().to_string();
        self.title = self.title.trim().to_string();
        self.nick_name = self.nick_name.trim().to_string();
        self.call_name = self.call_name.trim().to_string();
        self.name_prefix = self.name_prefix.trim().to_string();
        self.surname_prefix = self.surname_prefix.trim().to_string();
        self.suffix = self.suffix.trim().to_string();
        self.name_type = self.name_type.trim().to_string();
        self.name_origin = self.name_origin.trim().to_string();
        for alt in &mut self.alt_names {
            alt.strip();
        }
        self.alt_names.retain(|alt| !alt.is_empty());
    }

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
    /// Notiz zur Beziehung (Kontextmenü je Eintrag, Anzeige darunter).
    /// `None` = keine (Eintrag ausgeblendet), `Some` = vorhanden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Quellen zur Beziehung (Kontextmenü je Eintrag, Anzeige darunter).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceEntry>,
    /// Sicherheit der Beziehung (Default ungesetzt).
    #[serde(default, skip_serializing_if = "Certainty::is_unset")]
    pub certainty: Certainty,
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
                    notes: None,
                    sources: Vec::new(),
                    certainty: Certainty::Unset,
                },
                Family {
                    id: "f2".into(),
                    parent_a: Some("p3".into()),
                    parent_b: Some("p4".into()),
                    children: vec!["p5".into(), "p6".into()],
                    notes: None,
                    sources: Vec::new(),
                    certainty: Certainty::Unset,
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

    /// Familien-ID, die beide Personen verknüpft (Partner, Eltern/Kind oder
    /// Geschwister über gemeinsame Familie) — Anker für Beziehungsnotizen,
    /// -quellen und -sicherheit. Erste passende Familie (stabile Ordnung).
    pub fn relation_family_id(&self, first: &str, second: &str) -> Option<String> {
        self.families
            .iter()
            .filter(|family| {
                let has = |id: &str| {
                    family.parent_a.as_deref() == Some(id)
                        || family.parent_b.as_deref() == Some(id)
                        || family.children.iter().any(|child| child == id)
                };
                has(first) && has(second)
            })
            .map(|family| family.id.clone())
            .next()
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
                notes: None,
                sources: Vec::new(),
                certainty: Certainty::Unset,
            });
        }
    }
    /// Einfaches Kinder-Zuordnen (Vorfahren-/Nachfahrenbaum-Verbindungen und
    /// Geschwister-Verknüpfung aus dem Beziehungspicker). Bereits verknüpfte
    /// Paare (Eltern/Kind, egal in welcher Familie) werden nicht erneut
    /// angelegt — dieselbe Beziehung gibt es genau einmal.
    pub fn link_child(&mut self, parent_id: &str, child_id: &str) {
        if parent_id == child_id {
            return;
        }
        if self.families.iter().any(|family| {
            (family.parent_a.as_deref() == Some(parent_id)
                || family.parent_b.as_deref() == Some(parent_id))
                && family.children.iter().any(|child| child == child_id)
        }) {
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
                notes: None,
                sources: Vec::new(),
                certainty: Certainty::Unset,
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
        // Bereits verknüpft (Eltern/Kind, egal in welcher Familie) nicht
        // erneut anlegen — dieselbe Beziehung gibt es genau einmal. Gleiche
        // Guard wie `link_child`; ohne ihn entstünden beim Schnellerfassen
        // doppelte Eltern/Kind-Links (Ein-Elternteil-Familie + Paar-Familie).
        let already_linked = |parent: Option<&str>| -> bool {
            match parent {
                Some(parent) => self.families.iter().any(|family| {
                    (family.parent_a.as_deref() == Some(parent)
                        || family.parent_b.as_deref() == Some(parent))
                        && family.children.iter().any(|child| child == child_id)
                }),
                None => false,
            }
        };
        if already_linked(parent_a) || already_linked(parent_b) {
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
                notes: None,
                sources: Vec::new(),
                certainty: Certainty::Unset,
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
        for alt in drop.alt_names {
            if !alt.is_empty() && !keep.alt_names.contains(&alt) {
                keep.alt_names.push(alt);
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
        // Umverdrahtung kann doppelte Familien erzeugen (z. B. beide Partner
        // desselben Kindes) — sofort vereinen statt liegen lassen.
        self.dedupe_relationships();
    }

    /// Selektives Zusammenführen aus dem Review: Standard-Merge plus
    /// seitenweise Wahl für Name/Geburts-/Sterbedaten (inkl. Ereignis-Abgleich).
    pub fn apply_merge_choice(
        &mut self,
        keep_id: &str,
        drop_id: &str,
        take_new_birth: bool,
        take_new_death: bool,
        take_new_name: bool,
    ) {
        let drop = self.find(drop_id).cloned();
        self.merge_persons(keep_id, drop_id);
        let Some(drop) = drop else {
            return;
        };
        if take_new_name {
            if let Some(keep) = self.people.iter_mut().find(|person| person.id == keep_id) {
                keep.given_name = drop.given_name.clone();
                keep.family_name = drop.family_name.clone();
            }
        }
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
                    notes: None,
                    sources: Vec::new(),
                    certainty: Certainty::Unset,
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
                // Ignore-Menge des Paars (alle Nachnamengruppen inkl. Varianten).
                union_buf.clear();
                for surname in family_variants(existing)
                    .into_iter()
                    .chain(family_variants(incoming))
                {
                    if let Some(tokens) = common.get(&normalize_token(surname)) {
                        union_buf.extend(tokens.iter().cloned());
                    }
                }
                let ignore: &HashSet<String> =
                    if union_buf.is_empty() { &no_ignore } else { &union_buf };
                let name_score = best_given_score(existing, incoming, ignore);
                if name_score < threshold {
                    continue;
                }
                if !birth_compatible(&existing.birth, &incoming.birth) {
                    continue;
                }
                let exact_given = exact_given_variant(existing, incoming);
                // 1-von-2-exakt (oder voller Treffer) ersetzt die
                // Verwandtschafts-Hürde; sonst muss die Verwandtschaft passen.
                // Häufige Familiennamen (z. B. viele Marias) zählen hier nicht.
                let token_hit = token_hit_variant(existing, incoming, ignore);
                let kin_score = kin_similarity(self, existing, incoming, threshold, false);
                if !exact_given && !token_hit && kin_score < threshold {
                    continue;
                }
                let family_score = best_family_score(existing, incoming);
                // Nachname unter 50 %: kein automatischer Match (nur wenn
                // überhaupt ein Name bekannt ist — Unwissen ≠ Widerspruch).
                let surname_known = surname_known_variant(existing, incoming);
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
                for surname in family_variants(first)
                    .into_iter()
                    .chain(family_variants(second))
                {
                    if let Some(tokens) = common.get(&normalize_token(surname)) {
                        union_buf.extend(tokens.iter().cloned());
                    }
                }
                let ignore: &HashSet<String> =
                    if union_buf.is_empty() { &no_ignore } else { &union_buf };
                let name_score = best_given_score(first, second, ignore);
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
                let exact_given = exact_given_variant(first, second);
                // Häufige Familiennamen (z. B. viele Marias) zählen hier nicht.
                let token_hit = token_hit_variant(first, second, ignore);
                let kin_score = kin_similarity(self, first, second, threshold, true);
                if !exact_given && !token_hit && kin_score < threshold {
                    continue;
                }
                let family_score = best_family_score(first, second);
                // Nachname unter 50 %: kein automatischer Match (nur wenn
                // überhaupt ein Name bekannt ist — Unwissen ≠ Widerspruch).
                let surname_known = surname_known_variant(first, second);
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

    /// Paar-Scores berechnen (Namen/Vorname mit Häufigkeits-Dämpfung,
    /// Nachname, Geburt, Verwandtschaft) — Kern für manuellen Treffer und
    /// Paar-Match im Merge-Dialog.
    fn score_pair(
        &self,
        keep: &Person,
        drop: &Person,
        kin_threshold: f32,
        use_ids: bool,
        common_min: usize,
    ) -> MergeCandidate {
        let common = self.common_given_tokens(common_min);
        let mut ignore = HashSet::new();
        for surname in family_variants(keep)
            .into_iter()
            .chain(family_variants(drop))
        {
            if let Some(tokens) = common.get(&normalize_token(surname)) {
                ignore.extend(tokens.iter().cloned());
            }
        }
        MergeCandidate {
            keep_id: keep.id.clone(),
            drop_id: drop.id.clone(),
            name_score: best_given_score(keep, drop, &ignore),
            family_score: best_family_score(keep, drop),
            birth_match: !keep.birth.trim().is_empty() && !drop.birth.trim().is_empty(),
            kin_score: kin_similarity(self, keep, drop, kin_threshold, use_ids),
        }
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
        Some(self.score_pair(keep, drop, 0.0, false, common_min))
    }

    /// Match-Kandidat für zwei Bestandspersonen (Merge-Dialog): echte Scores
    /// ohne Schwellen (explizite Auswahl), mit Verwandten-IDs wie
    /// Projekt-Duplikate. `None` bei gleicher/unbekannter ID oder
    /// Eltern/Kind-Paar.
    pub fn pair_match_candidate(
        &self,
        keep_id: &str,
        drop_id: &str,
        threshold: f32,
        common_min: usize,
    ) -> Option<MergeCandidate> {
        if keep_id == drop_id {
            return None;
        }
        let keep = self.find(keep_id)?;
        let drop = self.find(drop_id)?;
        if self.parents_of(keep_id).iter().any(|parent| parent.id == drop_id)
            || self.parents_of(drop_id).iter().any(|parent| parent.id == keep_id)
        {
            return None;
        }
        Some(self.score_pair(keep, drop, threshold, true, common_min))
    }

    /// Exakter Treffer für den Import-Overlay? Name + Nachname +
    /// Verwandtschaft je ≥ 99,9 % — nur solche Paare werden ohne Review
    /// automatisch eingebettet (Lücken füllen, nie überschreiben).
    pub fn is_exact_candidate(candidate: &MergeCandidate) -> bool {
        candidate.name_score >= 0.999
            && candidate.family_score >= 0.999
            && candidate.kin_score >= 0.999
    }

    /// 1:1-Mapping Import → Bestand (greedy nach Score): jede Bestands- und
    /// jede Import-Person höchstens einmal — widersprüchliche Zuordnungen
    /// (A→X und B→X) sind damit technisch ausgeschlossen.
    pub fn build_import_mapping(
        &self,
        fresh_ids: &HashSet<String>,
        threshold: f32,
        common_min: usize,
    ) -> Vec<ImportMapping> {
        let mut out = Vec::new();
        let mut used_keep: HashSet<String> = HashSet::new();
        let mut used_drop: HashSet<String> = HashSet::new();
        for candidate in self.find_merge_candidates(fresh_ids, threshold, common_min) {
            if used_keep.contains(&candidate.keep_id) || used_drop.contains(&candidate.drop_id) {
                continue;
            }
            used_keep.insert(candidate.keep_id.clone());
            used_drop.insert(candidate.drop_id.clone());
            let exact = Self::is_exact_candidate(&candidate);
            out.push(ImportMapping {
                keep_id: candidate.keep_id,
                drop_id: candidate.drop_id,
                exact,
                name_score: candidate.name_score,
                family_score: candidate.family_score,
                kin_score: candidate.kin_score,
            });
        }
        out
    }

    /// Graphdistanz im ANHANG ab einer Import-Person (BFS über Familien-
    /// Mitgliedschaften, nur frische IDs): ordnet den Review vom Fixpunkt
    /// nach außen (Eltern/Partner/Kinder zuerst).
    pub fn import_distances(
        &self,
        from_drop: &str,
        fresh_ids: &HashSet<String>,
    ) -> HashMap<String, usize> {
        use std::collections::VecDeque;
        let mut dist: HashMap<String, usize> = HashMap::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        dist.insert(from_drop.to_string(), 0);
        queue.push_back(from_drop.to_string());
        while let Some(id) = queue.pop_front() {
            let depth = dist[&id];
            let mut neighbors: Vec<String> = Vec::new();
            for family in &self.families {
                let members: Vec<&str> = family
                    .parent_a
                    .as_deref()
                    .into_iter()
                    .chain(family.parent_b.as_deref())
                    .chain(family.children.iter().map(String::as_str))
                    .collect();
                if members.contains(&id.as_str()) {
                    neighbors.extend(members.into_iter().map(str::to_string));
                }
            }
            for neighbor in neighbors {
                if fresh_ids.contains(&neighbor) && !dist.contains_key(&neighbor) {
                    dist.insert(neighbor.clone(), depth + 1);
                    queue.push_back(neighbor);
                }
            }
        }
        dist
    }

    /// Import-Familie auf Bestands-Familie abbilden: Mitglieder über das
    /// Mapping (drop→keep) übersetzen (nicht gemappte Frische bleiben neue
    /// Personen); Eltern-Menge (reihenfolgefrei) vergleichen. Reine
    /// Frisch-Familien zählen nicht als Bestand (mind. ein Alt-Mitglied).
    pub fn pair_import_family(
        &self,
        drop_family_id: &str,
        drop_to_keep: &HashMap<String, String>,
        fresh_ids: &HashSet<String>,
    ) -> Option<String> {
        let family = self
            .families
            .iter()
            .find(|family| family.id == drop_family_id)?;
        let map_member = |id: Option<&String>| {
            id.map(|id| {
                drop_to_keep
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| id.clone())
            })
        };
        let mut want: Vec<String> = [
            map_member(family.parent_a.as_ref()),
            map_member(family.parent_b.as_ref()),
        ]
        .into_iter()
        .flatten()
        .collect();
        want.sort();
        self.families
            .iter()
            .filter(|other| other.id != drop_family_id)
            .filter(|other| {
                [&other.parent_a, &other.parent_b]
                    .into_iter()
                    .flatten()
                    .any(|member| !fresh_ids.contains(member))
            })
            .find(|other| {
                let mut have: Vec<String> = [&other.parent_a, &other.parent_b]
                    .into_iter()
                    .flatten()
                    .cloned()
                    .collect();
                have.sort();
                have == want
            })
            .map(|other| other.id.clone())
    }

    /// Skalar-Diffs zweier Personen (nur Unterschiede, in stabiler
    /// Feldreihenfolge):-label/Bestand/Neu je Zeile.
    pub fn person_scalar_diffs(keep: &Person, drop: &Person) -> Vec<ScalarDiff> {
        let mut out = Vec::new();
        let mut push = |key: &'static str, label: &'static str, keep_text: &str, new_text: &str| {
            if keep_text.trim() != new_text.trim() {
                out.push(ScalarDiff {
                    key,
                    label,
                    keep_text: keep_text.to_string(),
                    new_text: new_text.to_string(),
                });
            }
        };
        push("given_name", "Vorname", &keep.given_name, &drop.given_name);
        push(
            "family_name",
            "Nachname",
            &keep.family_name,
            &drop.family_name,
        );
        push("title", "Titel", &keep.title, &drop.title);
        push("nick_name", "Spitzname", &keep.nick_name, &drop.nick_name);
        push("call_name", "Rufname", &keep.call_name, &drop.call_name);
        push(
            "name_prefix",
            "Vornamenspräfix",
            &keep.name_prefix,
            &drop.name_prefix,
        );
        push(
            "surname_prefix",
            "Nachnamenspräfix",
            &keep.surname_prefix,
            &drop.surname_prefix,
        );
        push("suffix", "Suffix", &keep.suffix, &drop.suffix);
        push("name_type", "Namensart", &keep.name_type, &drop.name_type);
        push(
            "name_origin",
            "Herkunft",
            &keep.name_origin,
            &drop.name_origin,
        );
        push("birth", "Geburt", &keep.birth, &drop.birth);
        push(
            "birth_place",
            "Geburtsort",
            &keep.birth_place,
            &drop.birth_place,
        );
        push("death", "Tod", &keep.death, &drop.death);
        push(
            "death_place",
            "Sterbeort",
            &keep.death_place,
            &drop.death_place,
        );
        push("notes", "Notiz", &keep.notes, &drop.notes);
        push("source", "Quelle", &keep.source, &drop.source);
        if keep.gender != drop.gender {
            out.push(ScalarDiff {
                key: "gender",
                label: "Geschlecht",
                keep_text: gender_label(keep.gender).to_string(),
                new_text: gender_label(drop.gender).to_string(),
            });
        }
        out
    }

    /// Skalar-Diff übernehmen (ein Feld vom Import in den Bestand).
    pub fn apply_scalar_diff(&mut self, keep_id: &str, key: &str, new_text: &str) {
        let Some(keep) = self.people.iter_mut().find(|person| person.id == keep_id) else {
            return;
        };
        match key {
            "given_name" => keep.given_name = new_text.to_string(),
            "family_name" => keep.family_name = new_text.to_string(),
            "title" => keep.title = new_text.to_string(),
            "nick_name" => keep.nick_name = new_text.to_string(),
            "call_name" => keep.call_name = new_text.to_string(),
            "name_prefix" => keep.name_prefix = new_text.to_string(),
            "surname_prefix" => keep.surname_prefix = new_text.to_string(),
            "suffix" => keep.suffix = new_text.to_string(),
            "name_type" => keep.name_type = new_text.to_string(),
            "name_origin" => keep.name_origin = new_text.to_string(),
            "birth" => keep.birth = new_text.to_string(),
            "birth_place" => keep.birth_place = new_text.to_string(),
            "death" => keep.death = new_text.to_string(),
            "death_place" => keep.death_place = new_text.to_string(),
            "notes" => keep.notes = new_text.to_string(),
            "source" => keep.source = new_text.to_string(),
            "gender" => {
                keep.gender = match new_text {
                    "Männlich" => Gender::Male,
                    "Weiblich" => Gender::Female,
                    _ => Gender::Unknown,
                }
            }
            _ => {}
        }
    }

    /// Beziehungs-Diffs eines Paars: Import-Familien des Drops gegen
    /// gemappte Bestands-Familien (Partnerart, Heirat/Scheidung, Notiz,
    /// Quellen, fehlende Kinder, Kind-Arten). Ohne Treffer = neue Familie
    /// (bleibt angehängt, nur Anzeige).
    pub fn relation_diffs(
        &self,
        keep_id: &str,
        drop_id: &str,
        drop_to_keep: &HashMap<String, String>,
        fresh_ids: &HashSet<String>,
    ) -> Vec<RelDiff> {
        let mut out = Vec::new();
        let drop_families: Vec<String> = self
            .families
            .iter()
            .filter(|family| {
                family.parent_a.as_deref() == Some(drop_id)
                    || family.parent_b.as_deref() == Some(drop_id)
                    || family.children.iter().any(|child| child == drop_id)
            })
            .map(|family| family.id.clone())
            .collect();
        let keep = self.find(keep_id);
        let drop = self.find(drop_id);
        for drop_family_id in drop_families {
            let family = self.families.iter().find(|f| f.id == drop_family_id);
            let Some(drop_family) = family else { continue };
            let Some(keep_family_id) =
                self.pair_import_family(&drop_family_id, drop_to_keep, fresh_ids)
            else {
                out.push(RelDiff {
                    family_keep_id: None,
                    family_drop_id: drop_family_id.clone(),
                    kind: RelDiffKind::NewFamily,
                });
                continue;
            };
            let keep_family = self.families.iter().find(|f| f.id == keep_family_id);
            // Partnerart.
            let keep_relation = self
                .partner_relations
                .get(&keep_family_id)
                .copied()
                .unwrap_or(PartnerRelation::Unknown);
            let new_relation = self
                .partner_relations
                .get(&drop_family_id)
                .copied()
                .unwrap_or(PartnerRelation::Unknown);
            if keep_relation != new_relation {
                out.push(RelDiff {
                    family_keep_id: Some(keep_family_id.clone()),
                    family_drop_id: drop_family_id.clone(),
                    kind: RelDiffKind::PartnerRelation {
                        keep: keep_relation,
                        new: new_relation,
                    },
                });
            }
            // Heirat/Scheidung: Import-Events gegen Bestand vergleichen.
            if let (Some(keep), Some(drop)) = (keep, drop) {
                for event in &drop.events {
                    if !matches!(event.kind, EventKind::Marriage | EventKind::Divorce) {
                        continue;
                    }
                    let same = keep.events.iter().find(|own| {
                        own.kind == event.kind
                            && own.date == event.date
                            && own.place == event.place
                    });
                    let clash = keep.events.iter().find(|own| {
                        own.kind == event.kind
                            && (own.date != event.date || own.place != event.place)
                    });
                    if same.is_none() {
                        out.push(RelDiff {
                            family_keep_id: Some(keep_family_id.clone()),
                            family_drop_id: drop_family_id.clone(),
                            kind: RelDiffKind::RelationEvent {
                                event: event.clone(),
                                keep_event: clash.cloned(),
                            },
                        });
                    }
                }
            }
            // Familiennotiz.
            match (&keep_family.and_then(|f| f.notes.clone()), &drop_family.notes) {
                (_, None) => {}
                (None, Some(new)) if !new.trim().is_empty() => out.push(RelDiff {
                    family_keep_id: Some(keep_family_id.clone()),
                    family_drop_id: drop_family_id.clone(),
                    kind: RelDiffKind::FamilyNote {
                        keep: None,
                        new: new.clone(),
                    },
                }),
                (Some(keep_note), Some(new))
                    if !new.trim().is_empty()
                        && keep_note.trim() != new.trim()
                        && !keep_note.contains(new.trim()) =>
                {
                    out.push(RelDiff {
                        family_keep_id: Some(keep_family_id.clone()),
                        family_drop_id: drop_family_id.clone(),
                        kind: RelDiffKind::FamilyNote {
                            keep: Some(keep_note.clone()),
                            new: new.clone(),
                        },
                    })
                }
                _ => {}
            }
            // Familienquellen (fehlende Titel).
            if let Some(keep_family) = keep_family {
                let missing: Vec<SourceEntry> = drop_family
                    .sources
                    .iter()
                    .filter(|source| {
                        !keep_family
                            .sources
                            .iter()
                            .any(|own| own.title == source.title)
                    })
                    .cloned()
                    .collect();
                if !missing.is_empty() {
                    out.push(RelDiff {
                        family_keep_id: Some(keep_family_id.clone()),
                        family_drop_id: drop_family_id.clone(),
                        kind: RelDiffKind::FamilySources { missing },
                    });
                }
                // Kinder: Import-Kinder (gemappt) gegen Bestand.
                for child in &drop_family.children {
                    let mapped = drop_to_keep
                        .get(child)
                        .cloned()
                        .unwrap_or_else(|| child.clone());
                    if !keep_family.children.iter().any(|own| own == &mapped) {
                        let name = self
                            .find(&mapped)
                            .map(|person| person.display_name())
                            .unwrap_or_else(|| mapped.clone());
                        out.push(RelDiff {
                            family_keep_id: Some(keep_family_id.clone()),
                            family_drop_id: drop_family_id.clone(),
                            kind: RelDiffKind::ChildMissing {
                                child_id: mapped.clone(),
                                child_name: name,
                            },
                        });
                    } else {
                        // Kind beidseitig: Art vergleichen.
                        let keep_rel = self.relation_of_child(&keep_family_id, &mapped);
                        let new_rel = self.relation_of_child(&drop_family_id, child);
                        if keep_rel != new_rel {
                            let name = self
                                .find(&mapped)
                                .map(|person| person.display_name())
                                .unwrap_or_else(|| mapped.clone());
                            out.push(RelDiff {
                                family_keep_id: Some(keep_family_id.clone()),
                                family_drop_id: drop_family_id.clone(),
                                kind: RelDiffKind::ChildRelation {
                                    child_id: mapped.clone(),
                                    child_name: name,
                                    keep: keep_rel,
                                    new: new_rel,
                                },
                            });
                        }
                    }
                }
            }
        }
        out
    }

    /// Beziehungsart einer Familie setzen (Wizard-Option „Ersetzen").
    pub fn apply_family_partner_relation(
        &mut self,
        keep_family_id: &str,
        relation: PartnerRelation,
    ) {
        self.partner_relations
            .insert(keep_family_id.to_string(), relation);
    }

    /// Beziehungsereignis spiegeln: auf die Person UND den anderen Elternteil
    /// der Familie übertragen (Heirat/Scheidung gehört beiden).
    fn mirror_relation_event(
        &mut self,
        keep_family_id: &str,
        person_id: &str,
        event: &Event,
        mode: RelationEventMode,
    ) {
        let other = self
            .families
            .iter()
            .find(|family| family.id == keep_family_id)
            .and_then(|family| {
                [&family.parent_a, &family.parent_b]
                    .into_iter()
                    .flatten()
                    .find(|parent| parent.as_str() != person_id)
                    .cloned()
            });
        for pid in [person_id.to_string()].into_iter().chain(other) {
            let Some(person) = self.people.iter_mut().find(|p| p.id == pid) else {
                continue;
            };
            match mode {
                RelationEventMode::Replace => {
                    if let Some(own) = person
                        .events
                        .iter_mut()
                        .find(|own| own.kind == event.kind)
                    {
                        *own = event.clone();
                    } else {
                        person.events.push(event.clone());
                    }
                }
                RelationEventMode::Add => {
                    if !person.events.iter().any(|own| {
                        own.kind == event.kind
                            && own.date == event.date
                            && own.place == event.place
                    }) {
                        person.events.push(event.clone());
                    }
                }
                RelationEventMode::Merge => {
                    if let Some(own) = person
                        .events
                        .iter_mut()
                        .find(|own| own.kind == event.kind)
                    {
                        for source in &event.sources {
                            if !own.sources.contains(source) {
                                own.sources.push(source.clone());
                            }
                        }
                        if let Some(notes) = &event.notes {
                            if !notes.trim().is_empty() {
                                match &mut own.notes {
                                    Some(have) if !have.contains(notes.trim()) => {
                                        have.push_str("\n");
                                        have.push_str(notes.trim());
                                    }
                                    None => own.notes = Some(notes.clone()),
                                    _ => {}
                                }
                            }
                        }
                    } else {
                        person.events.push(event.clone());
                    }
                }
            }
        }
    }

    /// Wizard-Optionen auf ein Beziehungsereignis anwenden: Ersetzen =
    /// Datum/Ort/Notiz/Quellen vom Import; Ergänzen = zweites Ereignis
    /// daneben; Zusammenführen = Datum/Ort behalten, Notizen/Quellen vereinen.
    pub fn apply_relation_event(
        &mut self,
        keep_family_id: &str,
        person_id: &str,
        event: &Event,
        mode: RelationEventMode,
    ) {
        self.mirror_relation_event(keep_family_id, person_id, event, mode);
    }

    /// Familiennotiz: Ersetzen = überschreiben, Ergänzen = anhängen.
    pub fn apply_family_note(
        &mut self,
        keep_family_id: &str,
        new: &str,
        replace: bool,
    ) {
        let Some(family) = self
            .families
            .iter_mut()
            .find(|family| family.id == keep_family_id)
        else {
            return;
        };
        if replace || family.notes.as_deref().is_none_or(|note| note.trim().is_empty()) {
            family.notes = Some(new.to_string());
        } else if let Some(have) = family.notes.as_mut() {
            if !have.contains(new.trim()) {
                have.push('\n');
                have.push_str(new.trim());
            }
        }
    }

    /// Fehlende Familienquellen übernehmen (Titel-vergleich, keine Dubletten).
    pub fn apply_family_sources(
        &mut self,
        keep_family_id: &str,
        missing: &[SourceEntry],
    ) {
        let Some(family) = self
            .families
            .iter_mut()
            .find(|family| family.id == keep_family_id)
        else {
            return;
        };
        for source in missing {
            if !family.sources.iter().any(|own| own.title == source.title) {
                family.sources.push(source.clone());
            }
        }
    }

    /// Fehlendes Kind in die Bestands-Familie aufnehmen (Wizard-Option
    /// „Hinzufügen"), inkl. Kind-Art aus der Import-Familie.
    pub fn apply_add_child(
        &mut self,
        keep_family_id: &str,
        drop_family_id: &str,
        child_id: &str,
    ) {
        let relation = self
            .child_relations
            .get(&format!("{drop_family_id}/{child_id}"))
            .or_else(|| {
                self.families
                    .iter()
                    .find(|family| family.id == drop_family_id)
                    .and_then(|family| {
                        family.children.iter().find_map(|original| {
                            self.child_relations
                                .get(&format!("{drop_family_id}/{original}"))
                        })
                    })
            })
            .copied()
            .unwrap_or(ChildRelation::Birth);
        if let Some(family) = self
            .families
            .iter_mut()
            .find(|family| family.id == keep_family_id)
        {
            if !family.children.iter().any(|own| own == child_id) {
                family.children.push(child_id.to_string());
            }
        }
        self.child_relations
            .insert(format!("{keep_family_id}/{child_id}"), relation);
    }

    /// Kind-Art übernehmen (Wizard-Option „Ersetzen").
    pub fn apply_child_relation(
        &mut self,
        keep_family_id: &str,
        child_id: &str,
        relation: ChildRelation,
    ) {
        self.child_relations
            .insert(format!("{keep_family_id}/{child_id}"), relation);
    }

    /// Exaktes Paar automatisch einbetten (nur Lücken füllen): Standard-Merge
    /// plus Protokoll (gefüllte Felder, neue Ereignisse, übernommene
    /// Beziehungsart). Überschrieben wird nichts.
    pub fn supplement_exact_pair(&mut self, keep_id: &str, drop_id: &str) -> Vec<String> {
        let before = self.find(keep_id).cloned();
        let drop = self.find(drop_id).cloned();
        self.apply_merge_choice(keep_id, drop_id, false, false, false);
        let mut notes = Vec::new();
        let (Some(before), Some(drop), Some(after)) =
            (before, drop, self.find(keep_id).cloned())
        else {
            return notes;
        };
        let name = after.display_name();
        for (label, was, now) in [
            ("Geburt", before.birth.as_str(), after.birth.as_str()),
            ("Geburtsort", before.birth_place.as_str(), after.birth_place.as_str()),
            ("Tod", before.death.as_str(), after.death.as_str()),
            ("Sterbeort", before.death_place.as_str(), after.death_place.as_str()),
        ] {
            if was.trim().is_empty() && !now.trim().is_empty() {
                notes.push(format!("{name}: {label} ergänzt ({now})"));
            }
        }
        if after.events.len() > before.events.len() {
            notes.push(format!(
                "{name}: {} Ereignis(se) ergänzt",
                after.events.len() - before.events.len()
            ));
        }
        if after.sources.len() > before.sources.len() {
            notes.push(format!(
                "{name}: {} Quelle(n) ergänzt",
                after.sources.len() - before.sources.len()
            ));
        }
        if after.gallery.len() > before.gallery.len() {
            notes.push(format!("{name}: {} Bild(er) ergänzt", after.gallery.len() - before.gallery.len()));
        }
        if after.alt_names.len() > before.alt_names.len() {
            notes.push(format!(
                "{name}: {} Alternativname(n) ergänzt",
                after.alt_names.len() - before.alt_names.len()
            ));
        }
        let _ = drop;
        notes
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

    /// Doppelte Beziehungen vereinen (Debug-Aktion): dieselbe Beziehung
    /// (Partner/Kind) gibt es genau einmal. Entfernt doppelte Kinder je
    /// Familie, Selbst-Paare, doppelte Familien (Kinder + Notizen + Quellen
    /// vereint, Beziehungsarten auf die behaltene Familie umgebogen) und
    /// doppelte Partnerreihenfolgen. Ein Kind, das in MEHREREN Familien
    /// steht (auch bei überlappenden Eltern — z. B. Ein-Elternteil-Familie +
    /// Paar-Familie aus älteren Erfassungen), bleibt in der Familie mit
    /// beiden Eltern, sonst in der ersten. Gibt die Anzahl entfernter
    /// Einträge zurück. Deterministisch (erste Familie je Paar gewinnt).
    pub fn dedupe_relationships(&mut self) -> usize {
        let mut removed = 0usize;
        // 1) Ein Kind gehört zu EINER Familie. Steht dasselbe Kind in
        // mehreren Familien, entferne es aus allen bis auf die behaltene
        // Familie (Elternanzahl bevorzugt, sonst früheste). Behebt doppelte
        // Eltern/Kind-Links mit überlappenden Eltern — bereinigt, was die
        // Paar-Dedupe weiter unten nicht sieht.
        let memberships: Vec<(String, usize)> = self
            .families
            .iter()
            .enumerate()
            .flat_map(|(index, family)| {
                family
                    .children
                    .iter()
                    .map(move |child| (child.clone(), index))
            })
            .collect();
        let mut by_child: HashMap<String, Vec<usize>> = HashMap::new();
        for (child, index) in memberships {
            by_child.entry(child).or_default().push(index);
        }
        let mut drop_membership: Vec<(usize, String)> = Vec::new();
        for (child, indices) in by_child {
            if indices.len() < 2 {
                continue;
            }
            let keep = *indices
                .iter()
                .min_by_key(|&&index| {
                    let family = &self.families[index];
                    let parents = usize::from(family.parent_a.is_some())
                        + usize::from(family.parent_b.is_some());
                    (2 - parents, index)
                })
                .unwrap();
            for index in indices {
                if index != keep {
                    drop_membership.push((index, child.clone()));
                }
            }
        }
        for (index, child) in drop_membership {
            if let Some(family) = self.families.get_mut(index) {
                if let Some(pos) = family.children.iter().position(|c| c == &child) {
                    family.children.remove(pos);
                    removed += 1;
                }
            }
        }
        // 2) Je Familie: Kinder dedupieren, Selbst-Paar auflösen.
        for family in &mut self.families {
            let before = family.children.len();
            family.children.sort();
            family.children.dedup();
            removed += before - family.children.len();
            if family.parent_a.is_some() && family.parent_a == family.parent_b {
                family.parent_b = None;
                removed += 1;
            }
        }
        // 3) Doppelte Familien (gleiches Elternpaar, reihenfolgeunabhängig):
        // erste behalten, Rest einmischen + umbiegen.
        let mut keep_index: HashMap<(Option<String>, Option<String>), usize> = HashMap::new();
        let mut drop_to_keep: HashMap<String, String> = HashMap::new();
        let mut index = 0usize;
        while index < self.families.len() {
            let key = {
                let family = &self.families[index];
                let (mut first, mut second) =
                    (family.parent_a.clone(), family.parent_b.clone());
                if first > second {
                    std::mem::swap(&mut first, &mut second);
                }
                (first, second)
            };
            if let Some(&kept) = keep_index.get(&key) {
                let dropped = self.families.remove(index);
                removed += 1;
                let kept_id = self.families[kept].id.clone();
                if let Some(dropped_relation) = self.partner_relations.get(&dropped.id).copied() {
                    let relation = self
                        .partner_relations
                        .entry(kept_id.clone())
                        .or_insert(PartnerRelation::Unknown);
                    if *relation == PartnerRelation::Unknown {
                        *relation = dropped_relation;
                    }
                }
                drop_to_keep.insert(dropped.id.clone(), kept_id);
                let kept_family = &mut self.families[kept];
                for child in dropped.children {
                    if kept_family.children.contains(&child) {
                        removed += 1;
                    } else {
                        kept_family.children.push(child);
                    }
                }
                if kept_family.notes.is_none() {
                    kept_family.notes = dropped.notes;
                }
                for source in dropped.sources {
                    if !kept_family.sources.contains(&source) {
                        kept_family.sources.push(source);
                    }
                }
                if dropped.certainty > kept_family.certainty {
                    kept_family.certainty = dropped.certainty;
                }
            } else {
                keep_index.insert(key, index);
                index += 1;
            }
        }
        // 3) Beziehungsarten auf behaltene Familien umbiegen (Konflikt: kept gewinnt).
        if !drop_to_keep.is_empty() {
            let mut moved: HashMap<String, ChildRelation> = HashMap::new();
            for (key, relation) in std::mem::take(&mut self.child_relations) {
                let mut parts = key.splitn(2, '/');
                let family = parts.next().unwrap_or("");
                let child = parts.next().unwrap_or("").to_string();
                let family = drop_to_keep
                    .get(family)
                    .map(String::as_str)
                    .unwrap_or(family);
                moved
                    .entry(format!("{family}/{child}"))
                    .or_insert(relation);
            }
            self.child_relations = moved;
            let mut moved_partners: HashMap<String, PartnerRelation> = HashMap::new();
            for (key, relation) in std::mem::take(&mut self.partner_relations) {
                let family = drop_to_keep.get(&key).unwrap_or(&key).clone();
                moved_partners.entry(family).or_insert(relation);
            }
            self.partner_relations = moved_partners;
        }
        // 4) Doppelte Partnerreihenfolgen (Ordnung bleibt).
        for order in self.partner_order.values_mut() {
            let before = order.len();
            let mut seen = HashSet::new();
            order.retain(|partner| seen.insert(partner.clone()));
            removed += before - order.len();
        }
        // 5) Beziehungsarten aufräumen: nur Einträge behalten, deren Familie
        // das Kind noch führt (entfernte Mitgliedschaften + Umbiege-Lücken).
        let valid_memberships: HashSet<String> = self
            .families
            .iter()
            .flat_map(|family| {
                family
                    .children
                    .iter()
                    .map(|child| format!("{}/{child}", family.id))
            })
            .collect();
        let relations_before = self.child_relations.len();
        self.child_relations
            .retain(|key, _| valid_memberships.contains(key));
        removed += relations_before - self.child_relations.len();
        let families_before = self.families.len();
        self.cleanup_empty_families();
        removed += families_before - self.families.len();
        removed
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

/// 1:1-Zuordnung Import → Bestand im geführten Abgleich (Fixpunkt-Wizard):
/// `exact` = ohne Review automatisch einbettbar (100 % Name + Beziehungen).
#[derive(Clone, Debug)]
pub struct ImportMapping {
    pub keep_id: String,
    pub drop_id: String,
    pub exact: bool,
    pub name_score: f32,
    pub family_score: f32,
    pub kin_score: f32,
}

/// Skalar-Diff eines Personenfelds (nur Unterschiede): Schlüssel fürs
/// Übernehmen, Label + beider Texte für die Anzeige.
#[derive(Clone, Debug)]
pub struct ScalarDiff {
    pub key: &'static str,
    pub label: &'static str,
    pub keep_text: String,
    pub new_text: String,
}

/// Modus für Beziehungsereignisse im Wizard: Ersetzen = Datum/Ort/Notiz/
/// Quellen vom Import; Ergänzen = zweites Ereignis daneben; Zusammenführen =
/// Datum/Ort behalten, Notizen/Quellen vereinen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationEventMode {
    Replace,
    Add,
    Merge,
}

/// Beziehungs-Unterschied eines Paars (Import-Familie vs. gemappte
/// Bestands-Familie): Art, Ereignis, Notiz, Quellen, fehlende Kinder,
/// Kind-Arten — oder neue Familie (nur Anzeige, bleibt angehängt).
#[derive(Clone, Debug)]
pub struct RelDiff {
    pub family_keep_id: Option<String>,
    pub family_drop_id: String,
    pub kind: RelDiffKind,
}

/// Art eines Beziehungs-Unterschieds (Daten für Anzeige + Anwendung).
#[derive(Clone, Debug)]
pub enum RelDiffKind {
    PartnerRelation {
        keep: PartnerRelation,
        new: PartnerRelation,
    },
    RelationEvent {
        event: Event,
        keep_event: Option<Event>,
    },
    FamilyNote {
        keep: Option<String>,
        new: String,
    },
    FamilySources {
        missing: Vec<SourceEntry>,
    },
    ChildMissing {
        child_id: String,
        child_name: String,
    },
    ChildRelation {
        child_id: String,
        child_name: String,
        keep: ChildRelation,
        new: ChildRelation,
    },
    NewFamily,
}

/// Geschlechts-Label für Diffs (wie die Profilanzeige).
pub fn gender_label(gender: Gender) -> &'static str {
    match gender {
        Gender::Male => "Männlich",
        Gender::Female => "Weiblich",
        Gender::Unknown => "Unbekannt",
    }
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

/// Vornamens-Varianten einer Person (Hauptname + Alternativnamen, ohne
/// Leere/Dubletten): fürs Matching zählt die beste Paarung (z. B. Ehename
/// trifft Geburtsnamen).
fn given_variants(person: &Person) -> Vec<&str> {
    let mut out = vec![person.given_name.as_str()];
    for alt in &person.alt_names {
        let value = alt.given_name.as_str();
        if !value.trim().is_empty() && !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

/// Familiennamens-Varianten einer Person (Hauptname + Alternativnamen).
fn family_variants(person: &Person) -> Vec<&str> {
    let mut out = vec![person.family_name.as_str()];
    for alt in &person.alt_names {
        let value = alt.family_name.as_str();
        if !value.trim().is_empty() && !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

/// Beste Vornamens-Deckung über alle Namensvarianten beider Personen.
fn best_given_score(first: &Person, second: &Person, ignore: &HashSet<String>) -> f32 {
    let mut best = 0.0f32;
    for left in given_variants(first) {
        for right in given_variants(second) {
            best = best.max(name_similarity_common(left, right, ignore));
        }
    }
    best
}

/// Beste Familiennamens-Deckung über alle Namensvarianten beider Personen.
fn best_family_score(first: &Person, second: &Person) -> f32 {
    let mut best = 0.0f32;
    for left in family_variants(first) {
        for right in family_variants(second) {
            best = best.max(multi_token_score(left, right));
        }
    }
    best
}

/// Exakt gleicher Vorname in irgendeiner Variante (normiert verglichen).
fn exact_given_variant(first: &Person, second: &Person) -> bool {
    given_variants(first).iter().any(|left| {
        given_variants(second)
            .iter()
            .any(|right| normalize_token(left) == normalize_token(right))
    })
}

/// Geteiltes exaktes Vornamens-Token in irgendeiner Variante.
fn token_hit_variant(first: &Person, second: &Person, ignore: &HashSet<String>) -> bool {
    given_variants(first).iter().any(|left| {
        given_variants(second)
            .iter()
            .any(|right| shares_exact_token_common(left, right, ignore))
    })
}

/// Irgendeine Seite kennt einen Nachnamen (Haupt- oder Alternativname).
fn surname_known_variant(first: &Person, second: &Person) -> bool {
    family_variants(first)
        .into_iter()
        .chain(family_variants(second))
        .any(|name| !name_tokens(name).is_empty())
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
        given_name: given_name.trim().into(),
        family_name: family_name.trim().into(),
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
        alt_names: Vec::new(),
        events: Vec::new(),
        field_notes: HashMap::new(),
        field_sources: HashMap::new(),
        field_certainty: HashMap::new(),
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
    fn link_child_to_skips_already_linked_child() {
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
        // c ist bereits Kind von a (Ein-Elternteil-Familie, z. B. aus einer
        // früheren Aufwärts-Erfassung).
        data.link_child("a", "c");
        // Paar-Verknüpfung (a, b) mit demselben Kind: KEINE zweite
        // Eltern/Kind-Beziehung — sonst liefert children_of("a") c
        // zweimal.
        data.link_child_to(Some("a"), Some("b"), "c", ChildRelation::Birth);
        assert_eq!(data.families.len(), 1);
        assert_eq!(data.parents_of("c").len(), 1);
        assert_eq!(data.children_of("a").len(), 1);
        assert_eq!(data.children_of("b").len(), 0);
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
        let imported = TreeData {
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
    fn new_entry_fields_default_empty() {
        let person = person("x", "A", "B", "", Gender::Unknown);
        assert!(person.field_notes.is_empty());
        assert!(person.field_sources.is_empty());
        assert!(person.field_certainty.is_empty());
        let event = Event::new(EventKind::Birth);
        assert!(event.notes.is_none());
        assert!(event.sources.is_empty());
        assert_eq!(event.certainty, Certainty::Unset);
    }

    #[test]
    fn certainty_labels_and_order() {
        assert_eq!(Certainty::Unset.label(), "Ungesetzt");
        assert_eq!(Certainty::Oral.label(), "Mündliche Info");
        assert_eq!(Certainty::Document.label(), "Dokument");
        assert_eq!(Certainty::Certified.label(), "Beglaubigtes Dokument");
        assert!(Certainty::Unset < Certainty::Oral);
        assert!(Certainty::Oral < Certainty::Document);
        assert!(Certainty::Document < Certainty::Certified);
        assert!(Certainty::Unset.is_unset());
        assert!(!Certainty::Certified.is_unset());
        assert_eq!(Certainty::default(), Certainty::Unset);
    }

    #[test]
    fn legacy_json_without_new_keys_parses() {
        // Alte Dateien ohne neue Schlüssel laden mit Defaults.
        let person: Person = serde_json::from_str(
            r#"{"id":"p1","birth":"","death":"","gender":"Unknown"}"#,
        )
        .expect("Person ohne neue Schlüssel");
        assert!(person.field_notes.is_empty());
        let event: Event = serde_json::from_str(r#"{"kind":"Birth"}"#).expect("Event minimal");
        assert_eq!(event.certainty, Certainty::Unset);
        assert!(event.sources.is_empty());
    }

    #[test]
    fn strip_names_trims_outer_spaces() {
        let mut person = person("x", "  Hans  ", "  Bauke  ", "", Gender::Male);
        person.title = " Dr. ".into();
        person.nick_name = "  Hänschen ".into();
        person.strip_names();
        assert_eq!(person.given_name, "Hans");
        assert_eq!(person.family_name, "Bauke");
        assert_eq!(person.title, "Dr.");
        assert_eq!(person.nick_name, "Hänschen");
        // Innenliegende Leerzeichen bleiben.
        person.given_name = "Hans  Peter".into();
        person.strip_names();
        assert_eq!(person.given_name, "Hans  Peter");
    }

    #[test]
    fn relation_family_id_links_both_roles() {
        let data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("p1", "A", "X", "", Gender::Male),
                person("p2", "B", "X", "", Gender::Female),
                person("c1", "C", "X", "", Gender::Male),
                person("s", "S", "Y", "", Gender::Female),
            ],
            families: vec![Family {
                id: "f1".into(),
                parent_a: Some("p1".into()),
                parent_b: Some("p2".into()),
                children: vec!["c1".into()],
                notes: None,
                sources: Vec::new(),
                certainty: Certainty::Unset,
            }],
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        assert_eq!(data.relation_family_id("p1", "p2"), Some("f1".into()));
        assert_eq!(data.relation_family_id("p1", "c1"), Some("f1".into()));
        assert_eq!(data.relation_family_id("c1", "p2"), Some("f1".into()));
        assert_eq!(data.relation_family_id("p1", "s"), None);
        assert_eq!(data.relation_family_id("p1", "weg"), None);
    }

    #[test]
    fn dedupe_relationships_merges_duplicate_families() {
        let mut data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("p1", "A", "X", "", Gender::Male),
                person("p2", "B", "X", "", Gender::Female),
                person("c1", "C", "X", "", Gender::Male),
            ],
            families: vec![
                Family {
                    id: "f1".into(),
                    parent_a: Some("p1".into()),
                    parent_b: Some("p2".into()),
                    children: vec!["c1".into(), "c1".into()],
                    notes: None,
                    sources: Vec::new(),
                    certainty: Certainty::Unset,
                },
                Family {
                    id: "f2".into(),
                    parent_a: Some("p2".into()),
                    parent_b: Some("p1".into()),
                    children: vec!["c1".into()],
                    notes: None,
                    sources: Vec::new(),
                    certainty: Certainty::Unset,
                },
            ],
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        data.child_relations
            .insert("f2/c1".into(), ChildRelation::Birth);
        let removed = data.dedupe_relationships();
        // Doppeltes Kind (1) + Familie (1) + Kind-Überhang (1) = 3.
        assert_eq!(removed, 3);
        assert_eq!(data.families.len(), 1);
        assert_eq!(data.families[0].id, "f1");
        assert_eq!(data.families[0].children, vec!["c1".to_string()]);
        // Beziehungsart auf die behaltene Familie umgebogen.
        assert_eq!(
            data.child_relations.get("f1/c1"),
            Some(&ChildRelation::Birth)
        );
        assert!(data.child_relations.get("f2/c1").is_none());
        // Zweiter Lauf: nichts mehr zu tun.
        assert_eq!(data.dedupe_relationships(), 0);
    }

    #[test]
    fn dedupe_removes_child_from_extra_family_with_shared_parent() {
        let mut data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("a", "Alex", "", "", Gender::Unknown),
                person("b", "Bea", "", "", Gender::Unknown),
                person("c", "Chris", "", "", Gender::Unknown),
            ],
            families: vec![
                Family {
                    id: "f1".into(),
                    parent_a: Some("a".into()),
                    parent_b: None,
                    children: vec!["c".into()],
                    notes: None,
                    sources: Vec::new(),
                    certainty: Certainty::Unset,
                },
                Family {
                    id: "f2".into(),
                    parent_a: Some("a".into()),
                    parent_b: Some("b".into()),
                    children: vec!["c".into()],
                    notes: None,
                    sources: Vec::new(),
                    certainty: Certainty::Unset,
                },
            ],
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        data.child_relations.insert("f2/c".into(), ChildRelation::Birth);
        let removed = data.dedupe_relationships();
        // c steckt in f1 (Ein-Elternteil) UND f2 (Paar): f2 gewinnt, aus f1
        // wird es entfernt. Die Paar-Dedupe allein hätte das übersehen.
        assert_eq!(removed, 1);
        assert_eq!(data.children_of("a").len(), 1);
        assert_eq!(data.children_of("b").len(), 1);
        assert!(data
            .families
            .iter()
            .any(|family| family.id == "f2" && family.children == vec!["c".to_string()]));
        assert!(data.families.iter().all(|family| !family.children.contains(&"c".to_string())
            || family.id == "f2"));
        // Beziehungsart blieb auf der Paar-Familie.
        assert_eq!(
            data.child_relations.get("f2/c"),
            Some(&ChildRelation::Birth)
        );
        // Zweiter Lauf: nichts mehr zu tun.
        assert_eq!(data.dedupe_relationships(), 0);
    }

    #[test]
    fn merge_persons_dedupes_shared_families() {
        // keep und drop sind beide Vater von C bei derselben Mutter: nach dem
        // Merge genau eine Familie (keep, mom) mit C genau einmal.
        let mut data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("keep", "A", "X", "", Gender::Male),
                person("drop", "A", "X", "", Gender::Male),
                person("mom", "M", "X", "", Gender::Female),
                person("c", "C", "X", "", Gender::Male),
            ],
            families: vec![
                Family {
                    id: "f1".into(),
                    parent_a: Some("keep".into()),
                    parent_b: Some("mom".into()),
                    children: vec!["c".into()],
                    notes: None,
                    sources: Vec::new(),
                    certainty: Certainty::Unset,
                },
                Family {
                    id: "f2".into(),
                    parent_a: Some("drop".into()),
                    parent_b: Some("mom".into()),
                    children: vec!["c".into()],
                    notes: None,
                    sources: Vec::new(),
                    certainty: Certainty::Unset,
                },
            ],
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        data.merge_persons("keep", "drop");
        assert!(data.find("drop").is_none());
        assert_eq!(data.families.len(), 1);
        assert_eq!(data.families[0].children, vec!["c".to_string()]);
        // Zweitlauf mit frischem Duplikat-Paar (Partner doppelt verheiratet).
        data.families.push(Family {
            id: "f3".into(),
            parent_a: Some("keep".into()),
            parent_b: Some("mom".into()),
            children: Vec::new(),
            notes: None,
            sources: Vec::new(),
            certainty: Certainty::Unset,
        });
        assert_eq!(data.dedupe_relationships(), 1);
        assert_eq!(data.families.len(), 1);
    }

    #[test]
    fn merge_persons_carries_relation_infos_from_duplicate_family() {
        let marriage = Event {
            kind: EventKind::Marriage,
            date: "10 MAY 1811".into(),
            place: "Zielenzig".into(),
            description: String::new(),
            notes: None,
            sources: vec![SourceEntry {
                title: "Traubuch".into(),
                detail: String::new(),
                media: None,
            }],
            certainty: Certainty::Document,
        };
        let keep = person("p1", "Karl", "Aschenborn", "", Gender::Male);
        let spouse = person("p2", "Wilhelmine", "Kaßner", "", Gender::Female);
        let mut drop_keep = person("m1", "Karl", "Aschenborn", "", Gender::Male);
        let mut drop_spouse = person("m2", "Wilhelmine", "Kaßner", "", Gender::Female);
        drop_keep.events.push(marriage.clone());
        drop_spouse.events.push(marriage.clone());
        let mut data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![keep, spouse, drop_keep, drop_spouse],
            families: vec![
                Family {
                    id: "f1".into(),
                    parent_a: Some("p1".into()),
                    parent_b: Some("p2".into()),
                    children: Vec::new(),
                    notes: None,
                    sources: Vec::new(),
                    certainty: Certainty::Unset,
                },
                Family {
                    id: "f2".into(),
                    parent_a: Some("m1".into()),
                    parent_b: Some("m2".into()),
                    children: Vec::new(),
                    notes: Some("aus GEDCOM".into()),
                    sources: vec![SourceEntry {
                        title: "Familienquelle".into(),
                        detail: String::new(),
                        media: None,
                    }],
                    certainty: Certainty::Document,
                },
            ],
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        data.partner_relations
            .insert("f2".into(), PartnerRelation::Married);
        data.merge_persons("p1", "m1");
        data.merge_persons("p2", "m2");
        assert_eq!(data.families.len(), 1);
        let family = &data.families[0];
        assert_eq!(family.notes.as_deref(), Some("aus GEDCOM"));
        assert_eq!(family.sources[0].title, "Familienquelle");
        assert_eq!(family.certainty, Certainty::Document);
        assert_eq!(
            data.partner_relations.get(&family.id),
            Some(&PartnerRelation::Married)
        );
        for id in ["p1", "p2"] {
            let person = data.find(id).unwrap();
            let event = person
                .events
                .iter()
                .find(|event| event.kind == EventKind::Marriage)
                .unwrap();
            assert_eq!(event.date, "10 MAY 1811");
            assert_eq!(event.place, "Zielenzig");
            assert_eq!(event.sources[0].title, "Traubuch");
        }
    }

    #[test]
    fn merge_persons_unions_alternative_names() {
        let keep_person = person("p1", "Wilhelmine", "Kaßner", "", Gender::Female);
        let mut drop_person = person("m1", "Wilhelmine", "Aschenborn", "", Gender::Female);
        let mut alt = AlternativeName::default();
        alt.given_name = "Wilhelmine".into();
        alt.family_name = "Kaßner".into();
        alt.name_type = "Geburtsname".into();
        drop_person.alt_names.push(alt.clone());
        drop_person.alt_names.push(AlternativeName::default());
        let mut data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![keep_person, drop_person],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        data.merge_persons("p1", "m1");
        let keep = data.find("p1").unwrap();
        // Echter Alternativname übernommen, leerer verworfen.
        assert_eq!(keep.alt_names, vec![alt]);
    }

    #[test]
    fn merge_candidates_match_name_variants() {
        // Bestand kennt nur den Ehenamen, der Import nur den Geburtsnamen —
        // über die Alternativnamen finden sie sich trotzdem.
        let mut existing = person("p1", "Wilhelmine", "Aschenborn", "1785", Gender::Female);
        let mut alt = AlternativeName::default();
        alt.given_name = "Wilhelmine".into();
        alt.family_name = "Kaßner".into();
        existing.alt_names.push(alt);
        let incoming = person("m1", "Wilhelmine", "Kaßner", "1785", Gender::Female);
        let data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![existing, incoming],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        let fresh: HashSet<String> = ["m1".to_string()].into_iter().collect();
        let candidates = data.find_merge_candidates(&fresh, 0.5, 4);
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].family_score >= 0.99);
    }

    #[test]
    fn strip_names_cleans_alternative_names() {
        let mut p = person("p1", " A ", " B ", "", Gender::Unknown);
        let mut alt = AlternativeName::default();
        alt.given_name = " C ".into();
        alt.family_name = " D ".into();
        p.alt_names.push(alt);
        p.alt_names.push(AlternativeName::default());
        p.strip_names();
        assert_eq!(p.alt_names.len(), 1);
        assert_eq!(p.alt_names[0].given_name, "C");
        assert_eq!(p.alt_names[0].family_name, "D");
    }

    /// Wizard-Basis: Vater+Sohn im Bestand, identisches Paar im Anhang.
    fn wizard_tree() -> (TreeData, HashSet<String>) {
        let mut data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("p1", "Karl", "Aschenborn", "1800", Gender::Male),
                person("p2", "Fritz", "Aschenborn", "1830", Gender::Male),
            ],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        data.link_child("p1", "p2");
        let mut imported = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("x1", "Karl", "Aschenborn", "1800", Gender::Male),
                person("x2", "Fritz", "Aschenborn", "1830", Gender::Male),
            ],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        imported.link_child("x1", "x2");
        let fresh: HashSet<String> = data.append_import(imported).into_iter().collect();
        assert_eq!(fresh.len(), 2);
        (data, fresh)
    }

    #[test]
    fn wizard_mapping_is_one_to_one_and_exact() {
        let (data, fresh) = wizard_tree();
        let mapping = data.build_import_mapping(&fresh, 0.5, 4);
        assert_eq!(mapping.len(), 2);
        assert!(mapping.iter().all(|entry| entry.exact));
        // 1:1: keine Keep- oder Drop-ID doppelt.
        let mut keeps = HashSet::new();
        let mut drops = HashSet::new();
        for entry in &mapping {
            assert!(keeps.insert(entry.keep_id.clone()));
            assert!(drops.insert(entry.drop_id.clone()));
        }
    }

    #[test]
    fn wizard_distances_order_from_anchor() {
        let (data, fresh) = wizard_tree();
        let drop_father = data
            .people
            .iter()
            .find(|person| fresh.contains(&person.id) && person.given_name == "Karl")
            .unwrap()
            .id
            .clone();
        let distances = data.import_distances(&drop_father, &fresh);
        assert_eq!(distances.get(&drop_father), Some(&0));
        // Sohn genau eine Kante weiter.
        assert_eq!(distances.len(), 2);
        assert!(distances.values().any(|depth| *depth == 1));
    }

    #[test]
    fn wizard_pair_import_family_matches_parents() {
        let (data, fresh) = wizard_tree();
        let mapping = data.build_import_mapping(&fresh, 0.5, 4);
        let drop_to_keep: HashMap<String, String> = mapping
            .iter()
            .map(|entry| (entry.drop_id.clone(), entry.keep_id.clone()))
            .collect();
        // Import-Familie des Sohns ↔ Bestands-Familie des Sohns.
        let drop_family = data
            .families
            .iter()
            .find(|family| {
                family.children.iter().any(|child| {
                    fresh.contains(child)
                        && data.find(child).is_some_and(|person| {
                            person.given_name == "Fritz"
                        })
                })
            })
            .unwrap()
            .id
            .clone();
        let paired = data.pair_import_family(&drop_family, &drop_to_keep, &fresh);
        assert!(paired.is_some());
        let keep_family = data.families.iter().find(|family| Some(&family.id) == paired.as_ref()).unwrap();
        assert!(keep_family.children.iter().any(|child| child == "p2"));
    }

    #[test]
    fn wizard_scalar_diffs_and_apply() {
        let keep = person("p1", "Karl", "Aschenborn", "1800", Gender::Male);
        let mut drop = person("m1", "Karl", "Aschenborn", "1800", Gender::Male);
        drop.death = "1870".into();
        drop.title = "Dr.".into();
        let diffs = TreeData::person_scalar_diffs(&keep, &drop);
        assert!(diffs.iter().any(|diff| diff.key == "death"));
        assert!(diffs.iter().any(|diff| diff.key == "title"));
        assert!(!diffs.iter().any(|diff| diff.key == "birth"));
        let mut data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![keep, drop],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        data.apply_scalar_diff("p1", "death", "1870");
        assert_eq!(data.find("p1").unwrap().death, "1870");
    }

    #[test]
    fn wizard_relation_diffs_find_marriage_and_missing_child() {
        let marriage = Event {
            kind: EventKind::Marriage,
            date: "1811".into(),
            place: "Zielenzig".into(),
            description: String::new(),
            notes: None,
            sources: Vec::new(),
            certainty: Certainty::Unset,
        };
        let mut data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![
                person("p1", "Karl", "A", "", Gender::Male),
                person("p2", "Mina", "B", "", Gender::Female),
                person("m1", "Karl", "A", "", Gender::Male),
                person("m2", "Mina", "B", "", Gender::Female),
                person("m3", "Kind", "A", "", Gender::Male),
            ],
            families: vec![
                Family {
                    id: "f1".into(),
                    parent_a: Some("p1".into()),
                    parent_b: Some("p2".into()),
                    children: Vec::new(),
                    notes: None,
                    sources: Vec::new(),
                    certainty: Certainty::Unset,
                },
                Family {
                    id: "f2".into(),
                    parent_a: Some("m1".into()),
                    parent_b: Some("m2".into()),
                    children: vec!["m3".into()],
                    notes: None,
                    sources: Vec::new(),
                    certainty: Certainty::Unset,
                },
            ],
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        data.partner_relations
            .insert("f2".into(), PartnerRelation::Married);
        data.people
            .iter_mut()
            .find(|person| person.id == "m1")
            .unwrap()
            .events
            .push(marriage);
        let mut drop_to_keep = HashMap::new();
        drop_to_keep.insert("m1".to_string(), "p1".to_string());
        drop_to_keep.insert("m2".to_string(), "p2".to_string());
        let fresh: HashSet<String> =
            ["m1", "m2", "m3"].iter().map(|id| id.to_string()).collect();
        let diffs = data.relation_diffs("p1", "m1", &drop_to_keep, &fresh);
        assert!(diffs.iter().any(|diff| matches!(
            diff.kind,
            RelDiffKind::PartnerRelation { .. }
        )));
        assert!(diffs.iter().any(|diff| matches!(
            diff.kind,
            RelDiffKind::RelationEvent { .. }
        )));
        assert!(diffs.iter().any(|diff| matches!(
            diff.kind,
            RelDiffKind::ChildMissing { .. }
        )));
        // Hinzufügen übernimmt das Kind in die Bestands-Familie.
        data.apply_add_child("f1", "f2", "m3");
        assert!(data
            .families
            .iter()
            .find(|family| family.id == "f1")
            .unwrap()
            .children
            .contains(&"m3".to_string()));
        // Beziehungsart übernehmen.
        data.apply_family_partner_relation("f1", PartnerRelation::Married);
        assert_eq!(
            data.partner_relations.get("f1"),
            Some(&PartnerRelation::Married)
        );
    }

    #[test]
    fn wizard_supplement_fills_gaps_only() {
        let keep = person("p1", "Karl", "Aschenborn", "1800", Gender::Male);
        let mut drop = person("m1", "Karl", "Aschenborn", "1800", Gender::Male);
        drop.death = "1870".into();
        drop.title = "Dr.".into();
        let mut data = TreeData {
            project: ProjectMetadata::default(),
            people: vec![keep, drop],
            families: Vec::new(),
            child_relations: HashMap::new(),
            partner_relations: HashMap::new(),
            partner_order: HashMap::new(),
        };
        let notes = data.supplement_exact_pair("p1", "m1");
        assert!(data.find("m1").is_none());
        let keep = data.find("p1").unwrap();
        assert_eq!(keep.death, "1870");
        assert_eq!(keep.title, "Dr.");
        assert!(!notes.is_empty());
        // Bestand bleibt, wo er Inhalt hatte (Geburt unverändert).
        assert_eq!(keep.birth, "1800");
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
        data.apply_merge_choice("p1", "m9", false, true, false);
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
            notes: None,
            sources: Vec::new(),
            certainty: Certainty::Unset,
        });
        let mut d = person("d", "Dana", "", "", Gender::Unknown);
        d.events.push(Event {
            kind: EventKind::Custom("Partnerschaft".into()),
            date: "2001".into(),
            place: String::new(),
            description: String::new(),
            notes: None,
            sources: Vec::new(),
            certainty: Certainty::Unset,
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
