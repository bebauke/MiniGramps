//! Laden und Parsen von Familiendaten sowie Projekt-Suche.
//!
//! Verdrahtung:
//! - `load_file` wird von `ui::MiniGramps::load_path` und `import_dialog`
//!   aufgerufen und liefert ein `model::TreeData`.
//! - `discover_projects` speist die Liste im Öffnen-Dialog (`dialogs::show_open`).
//! - `default_library` liefert den Standard-Speicherort, benutzt von
//!   `ui::MiniGramps::new` und hier in `discover_projects`.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
use roxmltree::Document;

use crate::model::{EventKind, Family, Gender, Person, TreeData, person};

/// Kleine, schnell lesbare Projektbeschreibung neben der eigentlichen
/// Datendatei. Spaetere `.mfg`/`.mmg`-Pakete verwenden dieselben Felder in
/// ihrem Manifest am Paketanfang.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProjectManifest {
    pub format: String,
    pub format_version: u32,
    pub name: String,
    pub data_file: String,
}

/// Standard-Speicherort: App-Datenordner des Betriebssystems
/// (`...\AppData\Local\minigramps\MiniGramps\data`).
pub fn default_library() -> PathBuf {
    ProjectDirs::from("org", "minigramps", "MiniGramps")
        .map(|d| d.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("minigramps-data"))
}

/// Sitzungsdatei im fixen Datenordner: merkt sich das zuletzt geöffnete
/// Projekt, damit die App es beim Start wieder öffnet.
fn session_path() -> PathBuf {
    default_library().join("letzte-sitzung.json")
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SessionState {
    data_path: String,
}

/// Zuletzt geöffnetes Projekt (None = keine Sitzung oder Datei weg).
pub fn load_last_project() -> Option<PathBuf> {
    let text = fs::read_to_string(session_path()).ok()?;
    let state: SessionState = serde_json::from_str(&text).ok()?;
    let path = PathBuf::from(state.data_path);
    path.is_file().then_some(path)
}

/// Pfad des zuletzt geöffneten Projekts merken (Fehler still ignorieren —
/// die Sitzung ist ein Komfort-Feature, kein Datenbestand).
pub fn save_last_project(path: &Path) {
    let state = SessionState {
        data_path: path.display().to_string(),
    };
    if let Ok(text) = serde_json::to_string(&state) {
        let _ = fs::write(session_path(), text);
    }
}

/// Projekte im MiniGramps-Datenordner und im Gramps-Dokumentenordner
/// finden (rekursiv, Tiefe 3).
pub fn discover_projects(library: &Path) -> Vec<PathBuf> {
    let mut folders = vec![library.to_path_buf(), default_library()];
    if let Some(documents) =
        directories::UserDirs::new().and_then(|dirs| dirs.document_dir().map(Path::to_path_buf))
    {
        folders.push(documents.join("Gramps"));
    }
    let mut projects = Vec::new();
    for folder in folders {
        collect_project_files(&folder, &mut projects, 3);
    }
    projects.sort();
    projects
}

pub fn collect_project_files(folder: &Path, projects: &mut Vec<PathBuf>, depth: u8) {
    if let Ok(entries) = fs::read_dir(folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && depth > 0 {
                collect_project_files(&path, projects, depth - 1);
            }
            let is_metadata = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.ends_with(".layout.json") || name.ends_with(".manifest.json")
                });
            let supported = !is_metadata
                && path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                    matches!(
                        e.to_ascii_lowercase().as_str(),
                        "json" | "ged" | "gedcom" | "gramps" | "xml"
                    )
                });
            if supported && !projects.contains(&path) {
                projects.push(path);
            }
        }
    }
}

pub fn manifest_path(data_path: &Path) -> PathBuf {
    let stem = data_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("projekt");
    data_path.with_file_name(format!("{stem}.manifest.json"))
}

pub fn load_project_manifest(data_path: &Path) -> Option<ProjectManifest> {
    let text = fs::read_to_string(manifest_path(data_path)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_project_manifest(data_path: &Path, data: &TreeData) -> Result<PathBuf, String> {
    let manifest = ProjectManifest {
        format: "minigramps".into(),
        format_version: data.project.format_version,
        name: data.project.name.clone(),
        data_file: data_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("familienbaum.minigramps.json")
            .to_string(),
    };
    let path = manifest_path(data_path);
    let text = serde_json::to_string_pretty(&manifest).map_err(|error| error.to_string())?;
    fs::write(&path, text).map_err(|error| error.to_string())?;
    Ok(path)
}

/// Kurzer Anzeigename fuer den Oeffnen-Dialog. MiniGramps-JSON enthaelt den
/// vom Benutzer vergebenen Projektnamen; Fremdformate verwenden ihren
/// Dateinamen, ohne den vollstaendigen Speicherpfad offenzulegen.
pub fn project_display_name(path: &Path) -> String {
    if let Some(manifest) = load_project_manifest(path)
        && !manifest.name.trim().is_empty()
    {
        return manifest.name;
    }
    // Alte MiniGramps-Dateien besitzen noch kein Manifest. Dieser Fallback
    // verschwindet, sobald Projekte nur noch als `.mfg`/`.mmg` vorliegen.
    let is_json = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
    if is_json
        && let Ok(text) = fs::read_to_string(path)
        && let Ok(data) = serde_json::from_str::<TreeData>(&text)
        && !data.project.name.trim().is_empty()
        && data.project.name != "Unbenanntes Projekt"
    {
        return data.project.name;
    }
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Unbenanntes Projekt")
        .strip_suffix(".minigramps")
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("Unbenanntes Projekt")
        })
        .to_string()
}

/// Datei lesen: Gramps-Sicherungen sind GZIP-gepackt, Kodierungen können
/// UTF-8/UTF-16/CP1252 sein. Nach der Dekodierung je nach Endung parsen;
/// anschließend Legacy-Kombinamen normalisieren (`normalize_names`).
pub fn load_file(path: &Path) -> Result<TreeData, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let bytes = gunzip_if_needed(bytes)?;
    let content = decode_bytes(&bytes);
    let mut data = match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "json" => serde_json::from_str(&content).map_err(|e| e.to_string())?,
        "ged" | "gedcom" => parse_gedcom(&content)?,
        "gramps" | "xml" => parse_gramps_xml(&content)?,
        _ => return Err("Nicht unterstütztes Dateiformat".into()),
    };
    normalize_names(&mut data);
    Ok(data)
}

/// Alte Projektdateien konsolidieren: kombinierte Namen ("Vorname
/// Nachname") in die getrennten Felder `given_name`/`family_name`
/// überführen (letztes Wort = Nachname) und das Legacy-Feld leeren.
pub fn normalize_names(data: &mut TreeData) {
    for person in &mut data.people {
        if person.given_name.is_empty() && person.family_name.is_empty() && !person.name.is_empty()
        {
            let tokens: Vec<&str> = person.name.split_whitespace().collect();
            if let Some((last, first)) = tokens.split_last() {
                person.family_name = last.to_string();
                person.given_name = first.join(" ");
            } else if let Some(single) = tokens.first() {
                person.given_name = single.to_string();
            }
        }
        person.name.clear();
    }
}

fn gunzip_if_needed(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if !bytes.starts_with(&[0x1F, 0x8B]) {
        return Ok(bytes);
    }
    use std::io::Read as _;
    let mut output = Vec::new();
    flate2::read::GzDecoder::new(&bytes[..])
        .read_to_end(&mut output)
        .map_err(|error| format!("GZIP-Dekompression fehlgeschlagen: {error}"))?;
    Ok(output)
}

/// Kodierung erkennen: BOM (UTF-8/UTF-16), sonst UTF-16-Nullbyte-Muster,
/// sonst UTF-8, sonst Windows-1252 als Fallback.
fn decode_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_utf16(&bytes[2..], true);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_utf16(&bytes[2..], false);
    }
    if bytes.len() >= 4 {
        let sample = &bytes[..bytes.len().min(64)];
        let even_zeros = sample.iter().step_by(2).filter(|&&byte| byte == 0).count();
        let odd_zeros = sample
            .iter()
            .skip(1)
            .step_by(2)
            .filter(|&&byte| byte == 0)
            .count();
        if odd_zeros >= 4 && even_zeros == 0 {
            return decode_utf16(bytes, true);
        }
        if even_zeros >= 4 && odd_zeros == 0 {
            return decode_utf16(bytes, false);
        }
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    bytes.iter().map(|&byte| cp1252_char(byte)).collect()
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

fn cp1252_char(byte: u8) -> char {
    match byte {
        0x80 => '€',
        0x82 => '‚',
        0x83 => 'ƒ',
        0x84 => '„',
        0x85 => '…',
        0x86 => '†',
        0x87 => '‡',
        0x88 => 'ˆ',
        0x89 => '‰',
        0x8A => 'Š',
        0x8B => '‹',
        0x8C => 'Œ',
        0x8E => 'Ž',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '•',
        0x96 => '–',
        0x97 => '—',
        0x98 => '\u{02DC}',
        0x99 => '™',
        0x9A => 'š',
        0x9B => '›',
        0x9C => 'œ',
        0x9E => 'ž',
        0x9F => 'Ÿ',
        _ => byte as char,
    }
}

/// Aktuelles GEDCOM-Ereignis (falls vorhanden) in die Personen-Ereignisse
/// übernehmen und zurücksetzen.
fn flush_event(
    p: &mut Person,
    current_event: &mut Option<EventKind>,
    date: &mut String,
    place: &mut String,
) {
    if let Some(kind) = current_event.take() {
        if !date.is_empty() || !place.is_empty() {
            p.events.push(crate::model::Event {
                kind,
                date: std::mem::take(date),
                place: std::mem::take(place),
                description: String::new(),
            });
        }
        date.clear();
        place.clear();
    }
}

/// GEDCOM-Parser: Zeilenzustandsmaschine. Erkennt Personen (INDI), Familien
/// (FAM), NAME/SEX sowie BIRT/DEAT-Ereignisse mit DATE und PLAC
/// (Ereignis-Kontext `current_event`).
fn parse_gedcom(text: &str) -> Result<TreeData, String> {
    let mut data = TreeData::default();
    let mut current_person: Option<Person> = None;
    let mut current_family: Option<usize> = None;
    let mut current_event: Option<EventKind> = None;
    let mut current_event_date = String::new();
    let mut current_event_place = String::new();
    for line in text.lines() {
        let part: Vec<_> = line.split_whitespace().collect();
        if part.len() < 2 {
            continue;
        }
        if part[0] == "0" {
            if let Some(ref mut person) = current_person {
                flush_event(
                    person,
                    &mut current_event,
                    &mut current_event_date,
                    &mut current_event_place,
                );
            }
            if let Some(person) = current_person.take() {
                data.people.push(person);
            }
            current_family = None;
            if part.get(2) == Some(&"INDI") {
                current_person = Some(person(
                    part[1].trim_matches('@'),
                    "",
                    "",
                    "",
                    Gender::Unknown,
                ));
            } else if part.get(2) == Some(&"FAM") {
                let id = part[1].trim_matches('@').to_string();
                data.families.push(Family {
                    id,
                    parent_a: None,
                    parent_b: None,
                    children: vec![],
                });
                current_family = Some(data.families.len() - 1);
            }
        } else if let Some(p) = current_person.as_mut() {
            match part.get(1).copied() {
                Some("NAME") => {
                    let raw = part[2..].join(" ");
                    if let Some((given, family)) = raw.split_once('/') {
                        p.given_name = given.trim().to_string();
                        p.family_name = family.trim().trim_matches('/').to_string();
                    } else {
                        let tokens: Vec<&str> = raw.split_whitespace().collect();
                        if let Some((last, first)) = tokens.split_last() {
                            p.family_name = last.to_string();
                            p.given_name = first.join(" ");
                        }
                    }
                    current_event = None;
                }
                Some("SEX") => {
                    p.gender = match part.get(2) {
                        Some(&"M") => Gender::Male,
                        Some(&"F") => Gender::Female,
                        _ => Gender::Unknown,
                    };
                }
                Some(
                    tag @ ("BIRT" | "DEAT" | "MARR" | "DIV" | "BAPM" | "CHR" | "BURI" | "CREM"
                    | "OCCU" | "RESI" | "IMMI" | "EMIG" | "CENS" | "GRAD" | "EDUC" | "RETI"),
                ) => {
                    flush_event(
                        p,
                        &mut current_event,
                        &mut current_event_date,
                        &mut current_event_place,
                    );
                    current_event = Some(EventKind::from_gedcom(tag));
                }
                Some("DATE") => {
                    if current_event.is_some() {
                        current_event_date = part[2..].join(" ");
                        if current_event == Some(EventKind::Birth) {
                            p.birth = current_event_date.clone();
                        } else if current_event == Some(EventKind::Death) {
                            p.death = current_event_date.clone();
                        }
                    }
                }
                Some("PLAC") => {
                    if current_event.is_some() {
                        let place = part[2..].join(" ");
                        current_event_place = place.clone();
                        if current_event == Some(EventKind::Birth) {
                            p.birth_place = place.clone();
                        } else if current_event == Some(EventKind::Death) {
                            p.death_place = place.clone();
                        }
                    }
                }
                _ => {
                    if part[0] == "1" {
                        flush_event(
                            p,
                            &mut current_event,
                            &mut current_event_date,
                            &mut current_event_place,
                        );
                    }
                }
            }
        }
        if part.len() >= 3 {
            let value = part[2].trim_matches('@').to_string();
            if let Some(family_index) = current_family {
                let f = &mut data.families[family_index];
                match part[1] {
                    "HUSB" => f.parent_a = Some(value),
                    "WIFE" => f.parent_b = Some(value),
                    "CHIL" => f.children.push(value),
                    _ => {}
                }
            }
        }
    }
    if let Some(mut person) = current_person {
        flush_event(
            &mut person,
            &mut current_event,
            &mut current_event_date,
            &mut current_event_place,
        );
        data.people.push(person);
    }
    if data.people.is_empty() {
        Err("Keine Personen in GEDCOM gefunden".into())
    } else {
        Ok(data)
    }
}

/// Gramps-XML-Parser (auch GZIP-gepackte Sicherungen, siehe `load_file`).
/// Die DTD-Deklaration wird erlaubt (`allow_dtd`). Geburts-/Sterbedaten
/// stehen in `<event>`-Datensätzen und werden über `<eventref hlink>` den
/// Personen zugeordnet.
fn parse_gramps_xml(text: &str) -> Result<TreeData, String> {
    let doc = Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let mut data = TreeData::default();
    let mut events: HashMap<String, (String, String, String, String)> = HashMap::new();
    for node in doc.descendants().filter(|n| n.has_tag_name("event")) {
        let handle = node.attribute("handle").unwrap_or_default().to_string();
        let event_type = node.attribute("type").unwrap_or_default().to_string();
        let date = event_date(node);
        let place = node
            .descendants()
            .find(|n| n.has_tag_name("place"))
            .and_then(|n| n.text())
            .unwrap_or("")
            .to_string();
        let desc = node
            .descendants()
            .find(|n| n.has_tag_name("description"))
            .and_then(|n| n.text())
            .unwrap_or("")
            .to_string();
        events.insert(handle, (event_type, date.unwrap_or_default(), place, desc));
    }
    for node in doc.descendants().filter(|n| n.has_tag_name("person")) {
        let id = node.attribute("handle").unwrap_or_default().to_string();
        let gender = match node.attribute("gender") {
            Some("M") => Gender::Male,
            Some("F") => Gender::Female,
            _ => Gender::Unknown,
        };
        let name = node
            .descendants()
            .find(|n| n.has_tag_name("first"))
            .and_then(|n| n.text())
            .unwrap_or("Unbenannt");
        let surname = node
            .descendants()
            .find(|n| n.has_tag_name("surname"))
            .and_then(|n| n.text())
            .unwrap_or("");
        let mut birth = String::new();
        let mut death = String::new();
        let mut person_events = Vec::new();
        for reference in node.children().filter(|n| n.has_tag_name("eventref")) {
            let Some(link) = reference.attribute("hlink") else {
                continue;
            };
            if let Some((event_type, date, place, desc)) = events.get(link) {
                let kind = EventKind::from_gramps(event_type);
                if kind == EventKind::Birth {
                    birth = date.clone();
                } else if kind == EventKind::Death {
                    death = date.clone();
                }
                if !date.is_empty() || !place.is_empty() || !desc.is_empty() {
                    person_events.push(crate::model::Event {
                        kind,
                        date: date.clone(),
                        place: place.clone(),
                        description: desc.clone(),
                    });
                }
            }
        }
        data.people.push(person(&id, name, surname, &birth, gender));
        if let Some(p) = data.people.last_mut() {
            p.death = death;
            p.events = person_events;
        }
    }
    for node in doc.descendants().filter(|n| n.has_tag_name("family")) {
        let mut f = Family {
            id: node.attribute("handle").unwrap_or_default().into(),
            parent_a: None,
            parent_b: None,
            children: vec![],
        };
        for child in node.children() {
            match child.tag_name().name() {
                "father" => f.parent_a = child.attribute("hlink").map(str::to_string),
                "mother" => f.parent_b = child.attribute("hlink").map(str::to_string),
                "childref" => {
                    if let Some(id) = child.attribute("hlink") {
                        f.children.push(id.into())
                    }
                }
                _ => {}
            }
        }
        data.families.push(f);
    }
    if data.people.is_empty() {
        Err("Keine Personen in Gramps-XML gefunden".into())
    } else {
        Ok(data)
    }
}

/// Datum aus einem Gramps-Ereignisknoten ziehen (dateval/datestr/
/// daterange/datespan; bevorzugt `val`, sonst `start`/`stop`).
fn event_date(node: roxmltree::Node) -> Option<String> {
    for tag in ["dateval", "datestr", "daterange", "datespan"] {
        if let Some(date) = node.descendants().find(|n| n.has_tag_name(tag)) {
            if let Some(value) = date
                .attribute("val")
                .or_else(|| date.attribute("start"))
                .or_else(|| date.attribute("stop"))
            {
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf16_and_cp1252() {
        let utf16_le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("Jürgen".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode_bytes(&utf16_le), "Jürgen");
        assert_eq!(decode_bytes(b"J\xFCrgen"), "Jürgen");
        assert_eq!(decode_bytes("\u{FEFF}Jürgen".as_bytes()), "Jürgen");
    }

    #[test]
    fn decodes_utf16_without_bom() {
        let utf16_le: Vec<u8> = "<?xml".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert!(decode_bytes(&utf16_le).starts_with("<?xml"));
    }

    #[test]
    fn loads_gzipped_gramps_backup() {
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE database PUBLIC \"-//Gramps//DTD Gramps XML 1.7.2//EN\" \"http://gramps-project.org/xml/1.7.2/grampsxml.dtd\">\n<database><person handle=\"h1\" gender=\"M\"><name><first>Jonas</first><surname>Doe</surname></name></person></database>";
        use std::io::Write as _;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(xml.as_bytes()).unwrap();
        let gzipped = encoder.finish().unwrap();
        let decompressed = gunzip_if_needed(gzipped).unwrap();
        let data = parse_gramps_xml(&decode_bytes(&decompressed)).unwrap();
        assert_eq!(data.people[0].display_name(), "Jonas Doe");
    }

    #[test]
    fn imports_gramps_event_dates_and_gender() {
        let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><database>\
            <event handle=\"_E1\" type=\"Birth\"><dateval val=\"1971\"/></event>\
            <event handle=\"_E2\" type=\"Death\"><dateval val=\"2020\"/></event>\
            <person handle=\"_I1\" gender=\"M\"><name><first>Jonas</first><surname>Doe</surname></name>\
            <eventref hlink=\"_E1\" role=\"Primary\"/><eventref hlink=\"_E2\" role=\"Primary\"/></person>\
            </database>";
        let data = parse_gramps_xml(xml).unwrap();
        assert_eq!(data.people[0].gender, Gender::Male);
        assert_eq!(data.people[0].birth, "1971");
        assert_eq!(data.people[0].death, "2020");
        assert_eq!(data.people[0].events.len(), 2);
        assert_eq!(
            data.people[0].events[0].kind,
            crate::model::EventKind::Birth
        );
        assert_eq!(data.people[0].events[0].date, "1971");
    }

    #[test]
    fn imports_gedcom_dates_and_gender() {
        let data = parse_gedcom(
            "0 @I1@ INDI\n1 NAME Jürgen /Muster/\n1 SEX M\n1 BIRT\n2 DATE 12 MAR 1950\n2 PLAC Hagen\n1 DEAT\n2 DATE 2001\n0 @I2@ INDI\n1 NAME Anna /Muster/\n1 SEX F\n",
        )
        .unwrap();
        let juergen = &data.people[0];
        assert_eq!(juergen.gender, Gender::Male);
        assert_eq!(juergen.birth, "12 MAR 1950");
        assert_eq!(juergen.birth_place, "Hagen");
        assert_eq!(juergen.death, "2001");
        assert_eq!(data.people[1].gender, Gender::Female);
    }

    #[test]
    fn imports_gedcom_multiple_events() {
        let data = parse_gedcom(
            "0 @I1@ INDI\n1 NAME Jürgen /Muster/\n1 SEX M\n1 BIRT\n2 DATE 12 MAR 1950\n2 PLAC Hagen\n1 OCCU\n2 DATE 1975\n1 RESI\n2 PLAC Berlin\n0 @I2@ INDI\n1 NAME Anna /Muster/\n1 SEX F\n",
        )
        .unwrap();
        let juergen = &data.people[0];
        let kinds: Vec<String> = juergen
            .events
            .iter()
            .map(|e| e.kind.label().to_string())
            .collect();
        assert!(
            kinds.contains(&"Geburt".to_string()),
            "kein Geburtsevent: {kinds:?}"
        );
        assert!(
            kinds.contains(&"Beruf".to_string()),
            "kein Berufsevent: {kinds:?}"
        );
        assert!(
            kinds.contains(&"Wohnort".to_string()),
            "kein Wohnortevent: {kinds:?}"
        );
        let occ = juergen
            .events
            .iter()
            .find(|e| e.kind.label() == "Beruf")
            .unwrap();
        assert_eq!(occ.date, "1975");
    }

    #[test]
    fn imports_gedcom_family_relationships() {
        let data = parse_gedcom("0 @I1@ INDI\n1 NAME Alex /Muster/\n1 SEX M\n0 @I2@ INDI\n1 NAME Bea /Muster/\n1 SEX F\n0 @I3@ INDI\n1 NAME Chris /Muster/\n0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 CHIL @I3@\n").unwrap();
        assert_eq!(data.people.len(), 3);
        assert_eq!(data.parents_of("I3").len(), 2);
        assert_eq!(data.children_of("I1")[0].display_name(), "Chris Muster");
        assert_eq!(data.partners_of("I1")[0].display_name(), "Bea Muster");
    }

    #[test]
    fn uses_manifest_name_for_project_list() {
        let folder = std::env::temp_dir().join("minigramps-manifest-test");
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let data_path = folder.join("familienbaum.minigramps.json");
        let mut data = TreeData::demo();
        data.project.name = "Familie Bauke".into();
        fs::write(&data_path, serde_json::to_string(&data).unwrap()).unwrap();
        save_project_manifest(&data_path, &data).unwrap();

        assert_eq!(project_display_name(&data_path), "Familie Bauke");
        assert_eq!(
            load_project_manifest(&data_path).unwrap().data_file,
            "familienbaum.minigramps.json"
        );
        let _ = fs::remove_dir_all(folder);
    }
}
