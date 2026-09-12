//! Neue Projekte erhalten immer einen eigenen, exklusiv angelegten Ordner.
use std::{collections::HashMap, fs, path::{Path, PathBuf}};

use crate::{model::TreeData, store::{DataStore, FileSystemStore}};

pub fn create(
    root: &Path,
    data: &TreeData,
    source_base: &Path,
    offsets: &[(String, f32)],
    reference: Option<&str>,
) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let name: String = data.project.name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == ' ' { c } else { '_' })
        .take(80).collect();
    let name = if name.trim().is_empty() { "Projekt" } else { name.trim() };
    // Präfix vermeidet unter Windows reservierte Namen wie CON und NUL.
    let mut number = 1;
    let directory = loop {
        let suffix = if number == 1 { String::new() } else { format!(" ({number})") };
        let candidate = root.join(format!("Projekt-{name}{suffix}"));
        match fs::create_dir(&candidate) {
            Ok(()) => break candidate,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => number += 1,
            Err(e) => return Err(e.to_string()),
        }
    };
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

#[cfg(test)]
mod tests {
    use super::*;

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
