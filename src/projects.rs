//! Neue Projekte erhalten immer einen eigenen, exklusiv angelegten Ordner.
//! Vor mutierenden Importen wird der Projektordner nach `backups/` kopiert,
//! damit ein fehlerhafter Import rückgängig gemacht werden kann.
use std::{collections::HashMap, fs, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

use crate::{model::TreeData, store::{DataStore, FileSystemStore}};

/// Dateiname für Projektordner bereinigen (inkl. Schutz vor reservierten
/// Windows-Namen wie CON/NUL durch das "Projekt-"-Präfix der Aufrufer).
fn sanitize_project_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    if cleaned.trim().is_empty() {
        "Projekt".to_string()
    } else {
        cleaned.trim().to_string()
    }
}

/// Exklusiv anzulegenden Projektordner bestimmen (`Basis`, `Basis (2)`, …).
fn exclusive_dir(root: &Path, base: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let mut number = 1u32;
    loop {
        let suffix = if number == 1 {
            String::new()
        } else {
            format!(" ({number})")
        };
        let candidate = root.join(format!("{base}{suffix}"));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => number += 1,
            Err(e) => return Err(e.to_string()),
        }
    }
}

pub fn create(
    root: &Path,
    data: &TreeData,
    source_base: &Path,
    offsets: &[(String, f32)],
    reference: Option<&str>,
) -> Result<PathBuf, String> {
    // Präfix vermeidet unter Windows reservierte Namen wie CON und NUL.
    let directory =
        exclusive_dir(root, &format!("Projekt-{}", sanitize_project_name(&data.project.name)))?;
    let result = (|| {
        let mut imported = data.clone();
        fs::create_dir(directory.join("media")).map_err(|e| e.to_string())?;
        let mut copied = HashMap::<String, String>::new();
        for person in &mut imported.people {
            for path in person
                .photo
                .iter_mut()
                .chain(person.gallery.iter_mut())
                .chain(person.documents.iter_mut().map(|document| &mut document.path))
            {
                if let Some(relative) = copied.get(path) {
                    *path = relative.clone();
                    continue;
                }
                let source = source_base.join(&*path);
                let relative = crate::media::import_media_file(&directory, &source)
                    .ok_or_else(|| format!("Medienkopie fehlgeschlagen: {}", source.display()))?;
                copied.insert(path.clone(), relative.clone());
                *path = relative;
            }
        }
        let path = directory.join("baum.minigramps.json");
        let store = FileSystemStore::for_data_file(&path);
        store.write_data(&imported)?;
        store.write_layout(offsets, reference)?;
        crate::import::save_project_manifest(&path, &imported)?;
        Ok(path)
    })();
    if result.is_err() {
        // Ausschließlich den gerade von uns exklusiv erstellten Ordner entfernen.
        let _ = fs::remove_dir_all(&directory);
    }
    result
}

/// Dateien für das Komplettpaket einsammeln: JSON-Dateien im Projektordner
/// plus `media/`-Baum (ohne versteckte Dateien wie `.thumbs`).
fn collect_export_files(base: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir).map_err(|e| e.to_string())?.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry
            .file_name()
            .to_str()
            .ok_or("Ungültiger Dateiname")?
            .to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            // Nur der eigene Medienordner gehört zum Paket (darin alles).
            if dir == base && name != "media" {
                continue;
            }
            collect_export_files(base, &path, out)?;
        } else if path.is_file() {
            if dir == base {
                let is_data = name.ends_with(".minigramps.json")
                    || name.ends_with(".layout.json")
                    || name.ends_with(".manifest.json");
                if !is_data {
                    continue;
                }
            }
            let relative = path
                .strip_prefix(base)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            out.push(relative);
        }
    }
    Ok(())
}

/// Komplettpaket (.mfg) schreiben: Daten + Layout + Manifest + Medien als ZIP.
pub fn export_mfg(data_file: &Path, dest: &Path) -> Result<(), String> {
    let project_dir = data_file.parent().ok_or("Projektordner fehlt")?;
    let mut files = Vec::new();
    collect_export_files(project_dir, project_dir, &mut files)?;
    if !files.iter().any(|name| name.ends_with(".minigramps.json")) {
        return Err("Keine Projektdaten zum Exportieren".into());
    }
    let file = fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for name in files {
        let bytes = fs::read(project_dir.join(&name)).map_err(|e| e.to_string())?;
        zip.start_file(&name, options).map_err(|e| e.to_string())?;
        use std::io::Write as _;
        zip.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// Datendatei in einem entpackten Projektordner finden (bevorzugt
/// `baum.minigramps.json`).
fn find_data_json(directory: &Path) -> Option<PathBuf> {
    let mut fallback: Option<PathBuf> = None;
    let mut entries: Vec<_> = fs::read_dir(directory).ok()?.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name()?.to_str()?.to_string();
        if name == "baum.minigramps.json" {
            return Some(path);
        }
        if fallback.is_none() && name.ends_with(".minigramps.json") {
            fallback = Some(path);
        }
    }
    fallback
}

/// Komplettpaket (.mfg) in einen frischen Projektordner entpacken (ZIP-Slip-
/// geschützt); liefert die Datendatei zurück.
pub fn import_mfg(projects_root: &Path, mfg_path: &Path) -> Result<PathBuf, String> {
    let stem = mfg_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("projekt");
    let directory = exclusive_dir(
        projects_root,
        &format!("Projekt-{}", sanitize_project_name(stem)),
    )?;
    let result: Result<PathBuf, String> = (|| {
        let file = fs::File::open(mfg_path).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
            let Some(path) = entry.enclosed_name() else {
                continue;
            };
            let target = directory.join(&path);
            if entry.is_dir() {
                fs::create_dir_all(&target).map_err(|e| e.to_string())?;
                continue;
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out = fs::File::create(&target).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
        find_data_json(&directory).ok_or("Kein Projekt in .mfg gefunden".into())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&directory);
    }
    result
}

/// Medienverweise frisch angehängter Personen ins Zielprojekt kopieren
/// (Quellbasis = Ordner der Importdatei).
pub fn rebase_media_files(
    data: &mut TreeData,
    fresh_ids: &[String],
    source_base: &Path,
    target_dir: &Path,
) -> Result<(), String> {
    use std::collections::HashSet;
    let fresh: HashSet<&str> = fresh_ids.iter().map(String::as_str).collect();
    for person in &mut data.people {
        if !fresh.contains(person.id.as_str()) {
            continue;
        }
        let mut references: Vec<&mut String> = Vec::new();
        if let Some(photo) = person.photo.as_mut() {
            references.push(photo);
        }
        references.extend(person.gallery.iter_mut());
        references.extend(person.documents.iter_mut().map(|document| &mut document.path));
        for reference in references {
            let source = source_base.join(&*reference);
            if !source.is_file() {
                continue;
            }
            let Some(new_relative) = crate::media::import_media_file(target_dir, &source) else {
                return Err(format!("Medienkopie fehlgeschlagen: {}", source.display()));
            };
            *reference = new_relative;
        }
    }
    Ok(())
}

/// Verzeichnis rekursiv kopieren (Ziel wird angelegt, Inhalt überschrieben).
fn copy_dir(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|e| e.to_string())?;
    let entries = fs::read_dir(source).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let from = entry.path();
        let to = target.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Projektordner vor einem mutierenden Import sichern (`backups/<Name>-<Zeit>`,
/// höchstens die 5 neuesten je Projekt).
pub fn backup_project(library: &Path, project_dir: &Path) -> Result<PathBuf, String> {
    let name = project_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Projekt");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let root = library.join("backups");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let mut number = 0u32;
    let destination = loop {
        let suffix = if number == 0 {
            String::new()
        } else {
            format!("-{number}")
        };
        let candidate = root.join(format!("{name}-{stamp}{suffix}"));
        if fs::create_dir(&candidate).is_ok() {
            break candidate;
        }
        number += 1;
    };
    if let Err(error) = copy_dir(project_dir, &destination) {
        let _ = fs::remove_dir_all(&destination);
        return Err(error);
    }
    // Nur die 5 neuesten Sicherungen je Projekt behalten.
    let mut kept: Vec<PathBuf> = list_backups(library, name);
    kept.sort();
    kept.reverse();
    for outdated in kept.into_iter().skip(5) {
        let _ = fs::remove_dir_all(outdated);
    }
    Ok(destination)
}

/// Sicherungen eines Projekts (aufsteigend sortiert, ggf. leer).
pub fn list_backups(library: &Path, project_name: &str) -> Vec<PathBuf> {
    let root = library.join("backups");
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut backups: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name == project_name || name.starts_with(&format!("{project_name}-"))
                    })
        })
        .collect();
    backups.sort();
    backups
}

/// Projektordner aus einer Sicherung wiederherstellen (ersetzt den Inhalt).
pub fn restore_backup(project_dir: &Path, backup_dir: &Path) -> Result<(), String> {
    if !backup_dir.is_dir() {
        return Err("Sicherung fehlt".into());
    }
    // Zuerst kopieren, dann ersetzen — bei Kopierfehler bleibt alles stehen.
    let staging = project_dir.with_extension("restore-tmp");
    let _ = fs::remove_dir_all(&staging);
    copy_dir(backup_dir, &staging)?;
    fs::remove_dir_all(project_dir).map_err(|e| e.to_string())?;
    match fs::rename(&staging, project_dir) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            Err(error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_and_restore_round_trip() {
        let temp = std::env::temp_dir().join(format!("minigramps-backup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        let project = temp.join("projects").join("Projekt-Test");
        fs::create_dir_all(project.join("media")).unwrap();
        fs::write(project.join("baum.minigramps.json"), b"{\"v\":1}").unwrap();
        fs::write(project.join("media").join("a.png"), b"img").unwrap();
        let backup = backup_project(&temp, &project).unwrap();
        assert!(backup.join("baum.minigramps.json").is_file());
        assert!(backup.join("media").join("a.png").is_file());
        // Projekt „kaputtmachen", dann wiederherstellen.
        fs::write(project.join("baum.minigramps.json"), b"defekt").unwrap();
        fs::remove_file(project.join("media").join("a.png")).unwrap();
        restore_backup(&project, &backup).unwrap();
        assert_eq!(fs::read(project.join("baum.minigramps.json")).unwrap(), b"{\"v\":1}");
        assert!(project.join("media").join("a.png").is_file());
        // Sicherung taucht nicht als Projekt auf (Tiefe beachten).
        let mut found = Vec::new();
        crate::import::collect_project_files(&temp, &mut found, 3);
        assert!(!found.iter().any(|path| path.starts_with(temp.join("backups"))));
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn mfg_export_import_full_cycle() {
        let temp = std::env::temp_dir().join(format!("minigramps-mfg-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        // Quellprojekt mit Foto, Galerie, Versatz und Referenz anlegen.
        let source_media = temp.join("quelle.png");
        fs::create_dir_all(&temp).unwrap();
        fs::write(&source_media, b"bilddaten").unwrap();
        let mut data = TreeData::demo();
        data.project.name = "Vollpaket".into();
        data.people[0].photo = Some("quelle.png".into());
        data.people[0].gallery = vec!["quelle.png".into()];
        let json = create(&temp.join("projects"), &data, &temp, &[("p1".into(), 12.5)], Some("p1"))
            .unwrap();
        // Thumbs anlegen (dürfen NICHT ins Paket) + fremde Datei (bleibt außen vor).
        let project_dir = json.parent().unwrap().to_path_buf();
        fs::create_dir_all(project_dir.join("media").join(".thumbs")).unwrap();
        fs::write(project_dir.join("media").join(".thumbs").join("x.png"), b"t").unwrap();
        fs::write(project_dir.join("notizen.txt"), b"privat").unwrap();
        // Exportieren …
        let package = temp.join("vollpaket.mfg");
        export_mfg(&json, &package).unwrap();
        assert!(package.is_file());
        // … und in frischen Wurzelordner importieren.
        let back = import_mfg(&temp.join("restored"), &package).unwrap();
        let stored = FileSystemStore::for_data_file(&back).read_data().unwrap();
        let original = FileSystemStore::for_data_file(&json).read_data().unwrap();
        assert_eq!(
            serde_json::to_string(&stored).unwrap(),
            serde_json::to_string(&original).unwrap()
        );
        let (offsets, reference) = FileSystemStore::for_data_file(&back).read_layout();
        assert_eq!(offsets.get("p1").copied().unwrap_or(0.0), 12.5);
        assert_eq!(reference.as_deref(), Some("p1"));
        // Medieninhalt identisch, Thumbs/Fremddateien nicht übernommen.
        let image = back.parent().unwrap().join(stored.people[0].photo.as_ref().unwrap());
        assert_eq!(fs::read(image).unwrap(), b"bilddaten");
        assert!(!back.parent().unwrap().join("media").join(".thumbs").exists());
        assert!(!back.parent().unwrap().join("notizen.txt").exists());
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn repeated_import_preserves_existing_project_and_copies_media() {
        let temp = std::env::temp_dir().join(format!("minigramps-projects-{}", std::process::id()));
        fs::create_dir_all(&temp).unwrap();
        let source = temp.join("photo.png");
        fs::write(&source, b"original image bytes").unwrap();
        let mut data = TreeData::demo();
        data.project.name = "Bauke".into();
        data.people[0].photo = Some("photo.png".into());
        let first = create(&temp.join("projects"), &data, &temp, &[], None).unwrap();
        let original = fs::read(&first).unwrap();
        data.people[0].given_name = "Changed".into();
        let second = create(&temp.join("projects"), &data, &temp, &[], None).unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(&first).unwrap(), original);
        let stored = FileSystemStore::for_data_file(&second).read_data().unwrap();
        let image = second.parent().unwrap().join(stored.people[0].photo.as_ref().unwrap());
        assert_eq!(fs::read(image).unwrap(), b"original image bytes");
        assert_eq!(fs::read(source).unwrap(), b"original image bytes");
        fs::remove_dir_all(temp).unwrap();
    }
}
