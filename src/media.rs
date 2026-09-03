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

use std::{
    collections::HashMap,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use eframe::egui::{self, Align2, Color32, FontId, Sense, TextureHandle, Vec2};
use image::{GenericImageView, RgbaImage, imageops::FilterType};

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
    photo_texture_with_limit(ctx, person, cache, media_base, None, "")
}

fn photo_texture_with_limit<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
    max_side: Option<u32>,
    cache_prefix: &str,
) -> Option<&'a TextureHandle> {
    let cache_key = format!("{cache_prefix}{}", person.id);
    if !cache.contains_key(&cache_key) {
        let photo = person.photo.as_deref()?;
        let raw = Path::new(photo);
        let path = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            media_base.join(raw)
        };
        let image = if let Some(max_side) = max_side {
            image::open(path)
                .ok()?
                .thumbnail(max_side, max_side)
                .to_rgba8()
        } else {
            image::open(path).ok()?.to_rgba8()
        };
        let size = [image.width() as usize, image.height() as usize];
        let pixels = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        let texture_id = cache_key.clone();
        cache.insert(
            texture_id.clone(),
            ctx.load_texture(texture_id, pixels, Default::default()),
        );
    }
    cache.get(&cache_key)
}

fn media_path(media_base: &Path, photo: &str) -> PathBuf {
    let raw = Path::new(photo);
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        media_base.join(raw)
    }
}

fn thumb_key(person: &Person, size: u32, round: bool) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    person.photo.hash(&mut hasher);
    size.hash(&mut hasher);
    round.hash(&mut hasher);
    if let Some(crop) = &person.photo_crop {
        crop.x.to_bits().hash(&mut hasher);
        crop.y.to_bits().hash(&mut hasher);
        crop.zoom.to_bits().hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn thumb_path(media_base: &Path, key: &str) -> PathBuf {
    media_base.join("media").join(".thumbs").join(key)
}

fn ensure_gallery_thumb(media_base: &Path, person: &Person, size: u32) -> Option<PathBuf> {
    let key = format!("gallery-{}.png", thumb_key(person, size, false));
    let target = thumb_path(media_base, &key);
    if target.exists() {
        return Some(target);
    }
    let source = media_path(media_base, person.photo.as_deref()?);
    let image = image::open(source).ok()?.thumbnail(size, size).to_rgba8();
    fs::create_dir_all(target.parent()?).ok()?;
    image.save(&target).ok()?;
    Some(target)
}

fn ensure_round_avatar(media_base: &Path, person: &Person, size: u32) -> Option<PathBuf> {
    let key = format!("avatar-{}.png", thumb_key(person, size, true));
    let target = thumb_path(media_base, &key);
    if target.exists() {
        return Some(target);
    }
    let source = media_path(media_base, person.photo.as_deref()?);
    let image = image::open(source).ok()?;
    let (w, h) = image.dimensions();
    let aspect = w as f32 / h.max(1) as f32;
    let uv = cover_uv(aspect, person.photo_crop.as_ref());
    let left = (uv.left().clamp(0.0, 1.0) * w as f32).round() as u32;
    let top = (uv.top().clamp(0.0, 1.0) * h as f32).round() as u32;
    let right = (uv.right().clamp(0.0, 1.0) * w as f32).round() as u32;
    let bottom = (uv.bottom().clamp(0.0, 1.0) * h as f32).round() as u32;
    let crop_w = right.saturating_sub(left).max(1);
    let crop_h = bottom.saturating_sub(top).max(1);
    let cropped = image.crop_imm(left, top, crop_w, crop_h);
    let mut avatar: RgbaImage = image::imageops::resize(&cropped, size, size, FilterType::Lanczos3);
    let center = (size as f32 - 1.0) / 2.0;
    let radius = size as f32 / 2.0;
    for (x, y, pixel) in avatar.enumerate_pixels_mut() {
        let dx = x as f32 - center;
        let dy = y as f32 - center;
        if (dx * dx + dy * dy).sqrt() > radius {
            pixel.0[3] = 0;
        }
    }
    fs::create_dir_all(target.parent()?).ok()?;
    avatar.save(&target).ok()?;
    Some(target)
}

fn texture_from_file<'a>(
    ctx: &egui::Context,
    cache: &'a mut HashMap<String, TextureHandle>,
    key: String,
    path: PathBuf,
) -> Option<&'a TextureHandle> {
    if !cache.contains_key(&key) {
        let image = image::open(path).ok()?.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let pixels = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        cache.insert(
            key.clone(),
            ctx.load_texture(key.clone(), pixels, Default::default()),
        );
    }
    cache.get(&key)
}

/// Vorschau-Textur für kleine Listen/Thumbnails.
pub fn photo_preview_texture<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
) -> Option<&'a TextureHandle> {
    let path = ensure_gallery_thumb(media_base, person, 256)?;
    texture_from_file(
        ctx,
        cache,
        format!("preview:{}:{}", person.id, thumb_key(person, 256, false)),
        path,
    )
}

pub fn round_avatar_texture<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
) -> Option<&'a TextureHandle> {
    let path = ensure_round_avatar(media_base, person, 512)?;
    texture_from_file(
        ctx,
        cache,
        format!("avatar:{}:{}", person.id, thumb_key(person, 512, true)),
        path,
    )
}

/// Rechteckige Galerie-Vorschau aus dem Vollbild. Klick öffnet die Lightbox.
pub fn gallery_thumbnail_ui(
    ui: &mut egui::Ui,
    person: &Person,
    cache: &mut HashMap<String, TextureHandle>,
    media_base: &Path,
    size: Vec2,
) -> egui::Response {
    if let Some(texture) = photo_preview_texture(ui.ctx(), person, cache, media_base) {
        let tv = texture.size_vec2();
        let aspect = tv.x / tv.y.max(1.0);
        ui.add(
            egui::Image::from_texture(texture)
                .fit_to_exact_size(size)
                .uv(cover_uv(aspect, None))
                .corner_radius(5.0)
                .sense(Sense::click()),
        )
    } else {
        ui.allocate_response(size, Sense::click())
    }
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
    if let Some(texture) = round_avatar_texture(ui.ctx(), person, cache, media_base) {
        let (response, painter) = ui.allocate_painter(Vec2::splat(size), Sense::click_and_drag());
        painter.circle_filled(
            response.rect.center(),
            size / 2.0,
            Color32::from_black_alpha(24),
        );
        painter.image(
            texture.id(),
            response.rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.circle_stroke(
            response.rect.center(),
            size / 2.0,
            egui::Stroke::new(1.0, Color32::from_white_alpha(40)),
        );
        response
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

/// Avatar-Vorschau: kleiner geladene Textur, sonst wie `avatar_ui`.
pub fn avatar_ui_preview(
    ui: &mut egui::Ui,
    person: &Person,
    cache: &mut HashMap<String, TextureHandle>,
    media_base: &Path,
    size: f32,
) -> egui::Response {
    if let Some(texture) = round_avatar_texture(ui.ctx(), person, cache, media_base) {
        let (response, painter) = ui.allocate_painter(Vec2::splat(size), Sense::click_and_drag());
        painter.circle_filled(
            response.rect.center(),
            size / 2.0,
            Color32::from_black_alpha(24),
        );
        painter.image(
            texture.id(),
            response.rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.circle_stroke(
            response.rect.center(),
            size / 2.0,
            egui::Stroke::new(1.0, Color32::from_white_alpha(40)),
        );
        response
    } else {
        avatar_ui(ui, person, cache, media_base, size)
    }
}

/// Alle Texturen einer Person aus dem Cache entfernen.
pub fn clear_person_photo_cache(cache: &mut HashMap<String, TextureHandle>, person_id: &str) {
    cache.remove(person_id);
    cache.remove(&format!("preview:{person_id}"));
    cache.remove(&format!("avatar:{person_id}"));
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
