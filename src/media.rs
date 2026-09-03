//! Medien (Fotos): Speichern nach Gramps-Prinzip, Texturen, Avatare.
//!
//! Verdrahtung:
//! - `import_media_file` kopiert ein ausgewähltes Foto in den Medien-
//!   Basisordner `<library>/media` und liefert den relativen Pfad
//!   `media/<inhalts-hash>.<ext>` (Prüfsummen-Prinzip wie bei Gramps).
//!   Aufgerufen beim Setzen von Profilfotos und Galeriebildern.
//! - `photo_texture` löst relative Pfade gegen den Medien-Basisordner auf
//!   (`MiniGramps::library` wird als `media_base` durchgereicht) und cached
//!   Texturen in `MiniGramps::photo_cache`, Schlüssel = `Person::id`. Die
//!   Galerie erzeugt Pseudo-IDs `gallery-<id>-<index>` (siehe `ui`), um
//!   mehrere Bilder derselben Person zu cachen.
//! - `avatar_ui` wird in der rechten Seitenleiste (Profil, Beziehungsreihen,
//!   Galerie) genutzt; `ui::tree` zeichnet Karten mit `photo_texture` und
//!   `initials` direkt.

use std::{collections::HashMap, fs, path::Path};

use eframe::egui::{self, Align2, Color32, FontId, Sense, TextureHandle, Vec2};

use crate::model::{Person, PhotoCrop};

/// UV-Fenster für den Cover-Beschnitt: Das Bild füllt den Rahmen vollständig
/// OHNE Verzerrung (Seitenverhältnis wird beschnitten); `crop` verschiebt
/// und zoomt den Ausschnitt (zoom ≥ 1).
pub fn cover_uv(aspect: f32, crop: Option<&PhotoCrop>) -> egui::Rect {
    let (mut w, mut h) = if aspect > 1.0 {
        (1.0 / aspect, 1.0)
    } else {
        (1.0, aspect)
    };
    let zoom = crop.map(|c| c.zoom.max(1.0)).unwrap_or(1.0);
    w /= zoom;
    h /= zoom;
    let (cx, cy) = crop.map(|c| (c.x, c.y)).unwrap_or((0.0, 0.0));
    let max_x = (1.0 - w) / 2.0;
    let max_y = (1.0 - h) / 2.0;
    let cx = (cx * max_x).clamp(-max_x, max_x);
    let cy = (cy * max_y).clamp(-max_y, max_y);
    egui::Rect::from_min_max(
        egui::pos2(0.5 - w / 2.0 + cx, 0.5 - h / 2.0 + cy),
        egui::pos2(0.5 + w / 2.0 + cx, 0.5 + h / 2.0 + cy),
    )
}

/// Foto in den Medien-Basisordner kopieren und relativen Pfad liefern.
/// Der Dateiname ist ein Inhalts-Hash, doppelte Bilder werden so vermieden.
pub fn import_media_file(library: &Path, source: &Path) -> Option<String> {
    use std::hash::{Hash, Hasher};
    let bytes = fs::read(source).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    let digest = hasher.finish();
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .unwrap_or_else(|| "jpg".into());
    let relative = format!("media/{digest:016x}.{extension}");
    let target = library.join(&relative);
    if !target.exists() {
        fs::create_dir_all(target.parent()?).ok()?;
        fs::write(&target, &bytes).ok()?;
    }
    Some(relative)
}

/// Textur für das Profilbild einer Person (lazy geladen und gecached).
/// Relative Pfade gelten gegenüber `media_base` (= `MiniGramps::library`).
pub fn photo_texture<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
) -> Option<&'a TextureHandle> {
    if !cache.contains_key(&person.id) {
        let photo = person.photo.as_deref()?;
        let raw = Path::new(photo);
        let path = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            media_base.join(raw)
        };
        let image = image::open(path).ok()?.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let pixels = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        cache.insert(
            person.id.clone(),
            ctx.load_texture(format!("photo-{}", person.id), pixels, Default::default()),
        );
    }
    cache.get(&person.id)
}

/// Initialen für den Platzhalter-Avatar (maximal 2 Buchstaben).
pub fn initials(person: &Person) -> String {
    person
        .display_name()
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

/// Avatar: Foto, sonst Kreis mit Initialen. Klickbar (Rückgabe-Response).
pub fn avatar_ui(
    ui: &mut egui::Ui,
    person: &Person,
    cache: &mut HashMap<String, TextureHandle>,
    media_base: &Path,
    size: f32,
) -> egui::Response {
    if let Some(texture) = photo_texture(ui.ctx(), person, cache, media_base) {
        let tv = texture.size_vec2();
        let aspect = tv.x / tv.y.max(1.0);
        // Cover-Beschnitt mit runden Ecken — kein Verzerren des Bildes.
        // Sense click_and_drag, damit der Ausschnitt per Drag verschoben
        // werden kann (egui-Images sind sonst nur hover-empfindlich).
        ui.add(
            egui::Image::from_texture(texture)
                .fit_to_exact_size(Vec2::splat(size))
                .uv(cover_uv(aspect, person.photo_crop.as_ref()))
                .corner_radius(size / 2.0)
                .sense(Sense::click_and_drag()),
        )
    } else {
        let (response, painter) = ui.allocate_painter(Vec2::splat(size), Sense::click_and_drag());
        painter.circle_filled(
            response.rect.center(),
            size / 2.0,
            Color32::from_rgb(55, 91, 101),
        );
        painter.text(
            response.rect.center(),
            Align2::CENTER_CENTER,
            initials(person),
            FontId::proportional(size * 0.32),
            Color32::WHITE,
        );
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_photo_into_media_folder() {
        let temp = std::env::temp_dir().join("minigramps-media-test");
        let _ = fs::remove_dir_all(&temp);
        let source = temp.join("quelle.png");
        fs::create_dir_all(&temp).unwrap();
        fs::write(&source, b"pngdata").unwrap();
        let relative = import_media_file(&temp, &source).unwrap();
        assert!(relative.starts_with("media/"));
        assert!(temp.join(&relative).exists());
        // Erneutes Importieren derselben Daten liefert denselben Pfad.
        assert_eq!(import_media_file(&temp, &source).unwrap(), relative);
        let _ = fs::remove_dir_all(&temp);
    }
}
