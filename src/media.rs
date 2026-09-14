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
    sync::atomic::{AtomicUsize, Ordering},
    sync::{Mutex, OnceLock, mpsc},
    thread,
    time::UNIX_EPOCH,
};

use eframe::egui::{self, Align2, Color32, FontId, Sense, TextureHandle, Vec2};
use image::{GenericImageView, RgbaImage, imageops::FilterType};

use crate::model::{Person, PhotoCrop, TreeData};

static PENDING_THUMBS: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
static ACTIVE_THUMBS: AtomicUsize = AtomicUsize::new(0);
const MAX_ACTIVE_THUMBS: usize = 2;

/// UV-Fenster für den Cover-Beschnitt: Das Bild füllt den Rahmen vollständig
/// OHNE Verzerrung (Seitenverhältnis wird beschnitten); `crop` verschiebt
/// und zoomt den Ausschnitt (zoom ≥ 1).
pub fn cover_uv(aspect: f32, crop: Option<&PhotoCrop>) -> egui::Rect {
    cover_uv_to(aspect, 1.0, crop)
}

/// UV-Fenster für ein beliebiges Ziel-Seitenverhältnis. Verhindert Stretching,
/// wenn z.B. Galerie-Thumbnails rechteckig statt quadratisch sind.
pub fn cover_uv_to(aspect: f32, target_aspect: f32, crop: Option<&PhotoCrop>) -> egui::Rect {
    let target_aspect = target_aspect.max(0.01);
    let (mut w, mut h) = if aspect > target_aspect {
        (target_aspect / aspect, 1.0)
    } else {
        (1.0, aspect / target_aspect)
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

/// Profil-/Galeriebild um 90° im Uhrzeigersinn drehen. Die Datei wird dabei
/// überschrieben (kein Undo auf Dateiebene); Aufrufer müssen danach die
/// Foto-Caches leeren und den Ausschnitt zurücksetzen.
pub fn rotate_image_file_90(media_base: &Path, relative: &str) -> bool {
    let path = media_path(media_base, relative);
    let Ok(image) = image::open(&path) else {
        return false;
    };
    image.rotate90().save(&path).is_ok()
}

/// Profil-/Galeriebild um 90° gegen den Uhrzeigersinn drehen (siehe
/// `rotate_image_file_90` — eigener Pfad statt dreimaligem Speichern,
/// damit JPEGs nicht mehrfach neu kodiert werden).
pub fn rotate_image_file_ccw(media_base: &Path, relative: &str) -> bool {
    let path = media_path(media_base, relative);
    let Ok(image) = image::open(&path) else {
        return false;
    };
    image.rotate270().save(&path).is_ok()
}

/// Scanlinien/Druckraster glätten (3×3-Median je Kanal): dünne Linien und
/// Halbtonpunkte verschwinden, Flächen und Kanten bleiben weitgehend
/// erhalten. Einmaliger expliziter Eingriff auf der Basisdatei (Viewer
/// „Bearbeiten") — nichts Automatischem in der Thumb-Pipeline.
pub fn median_3x3(image: &image::RgbaImage) -> image::RgbaImage {
    let (width, height) = image.dimensions();
    let mut out = image.clone();
    if width < 3 || height < 3 {
        return out;
    }
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let mut window = [[0u8; 9]; 4];
            let mut index = 0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let pixel = image.get_pixel(
                        (x as i32 + dx) as u32,
                        (y as i32 + dy) as u32,
                    );
                    for channel in 0..4 {
                        window[channel][index] = pixel[channel];
                    }
                    index += 1;
                }
            }
            let mut mixed = image::Rgba([0, 0, 0, 0]);
            for channel in 0..4 {
                window[channel].sort_unstable();
                mixed[channel] = window[channel][4];
            }
            out.put_pixel(x, y, mixed);
        }
    }
    out
}

/// Scanlinien aus der Basisdatei herausrechnen (3×3-Median, Datei wird
/// überschrieben); Aufrufer frischen danach die Thumbs auf.
pub fn descreen_image_file(media_base: &Path, relative: &str) -> bool {
    let path = media_path(media_base, relative);
    let Ok(image) = image::open(&path) else {
        return false;
    };
    median_3x3(&image.to_rgba8()).save(&path).is_ok()
}

/// Alle zwischengespeicherten Vorschaubilder löschen (Anzahl zurück).
/// Avatare und Galerie werden danach bei Bedarf neu aus den Originalen
/// erzeugt (z. B. nach einem Logikwechsel wie Original statt Thumbnail).
pub fn delete_all_thumbs(media_base: &Path) -> usize {
    let dir = media_base.join("media").join(".thumbs");
    let Ok(entries) = fs::read_dir(&dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.path().is_file() && fs::remove_file(entry.path()).is_ok()
        })
        .count()
}

/// Nur runde Profilbild-Thumbs löschen, damit sie neu aus den Originalen
/// erzeugt werden (Galerie- und Vollbild-Thumbs bleiben erhalten).
pub fn delete_avatar_thumbs(media_base: &Path) -> usize {
    let dir = media_base.join("media").join(".thumbs");
    let Ok(entries) = fs::read_dir(&dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.starts_with("avatar2-") || name.starts_with("avatar-")
            }) && entry.path().is_file()
                && fs::remove_file(entry.path()).is_ok()
        })
        .count()
}

/// Alle zwischengespeicherten Vorschaubilder einer Person löschen (z. B.
/// nach dem Drehen des Originals, dessen Pfad — und damit Thumb-Schlüssel —
/// gleich bleibt).
pub fn delete_person_thumbs(media_base: &Path, person: &Person) {
    for (size, round, prefix) in [
        (150u32, false, "gallery"),
        (CARD_THUMB_SHORT_SIDE, false, "card-480"),
        (100u32, true, "avatar"),
        (100u32, true, "avatar2"),
    ] {
        let key = thumb_key(person, size, round);
        let _ = fs::remove_file(thumb_path(media_base, &format!("{prefix}-{key}.png")));
    }
    let _ = fs::remove_file(thumb_path(media_base, &large_thumb_filename(person)));
}

/// Dateipfade aus der Zwischenablage lesen (Explorer: Strg+C auf Dateien).
/// Nur Windows; anderswo immer leer.
pub fn read_clipboard_files() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        clipboard_win::get_clipboard::<Vec<PathBuf>, _>(clipboard_win::formats::FileList)
            .unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Erste Bilddatei aus der Datei-Zwischenablage (kein Bitmap nötig).
pub fn read_clipboard_image_file() -> Option<PathBuf> {
    read_clipboard_files().into_iter().find(|path| is_image_file(path))
}

/// Metadaten eines Projektbilds für die Infozeile des Bildbetrachters.
#[derive(Clone, Debug, Default)]
pub struct PhotoMeta {
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    /// Aufnahmezeitpunkt aus EXIF (DateTimeOriginal), falls vorhanden.
    pub date: Option<String>,
    /// Kamera aus EXIF (Hersteller + Modell), falls vorhanden.
    pub camera: Option<String>,
}

impl PhotoMeta {
    /// Einzeilige Anzeige: „4000 × 3000 · 2,4 MB · 12.03.2024 14:22 · Canon EOS R6".
    pub fn display_line(&self) -> String {
        let mut parts = vec![format!("{} × {}", self.width, self.height)];
        parts.push(format_bytes(self.bytes));
        if let Some(date) = &self.date {
            parts.push(date.clone());
        }
        if let Some(camera) = &self.camera {
            if !camera.is_empty() {
                parts.push(camera.clone());
            }
        }
        parts.join(" · ")
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < 3 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{:.1} {}", value, UNITS[unit]).replace('.', ",")
    }
}

/// Auflösung, Dateigröße und EXIF-Daten (Datum, Kamera) aus der Originaldatei
/// lesen (nur Datei-Header, kein vollständiges Dekodieren).
pub fn read_photo_meta(media_base: &Path, relative: &str) -> Option<PhotoMeta> {
    let raw = Path::new(relative);
    let full = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        media_base.join(raw)
    };
    let metadata = fs::metadata(&full).ok()?;
    let reader = image::ImageReader::open(&full).ok()?;
    let (width, height) = reader.into_dimensions().ok()?;
    let file = fs::File::open(&full).ok()?;
    let mut buffered = std::io::BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut buffered).ok();
    let field = |tag| {
        exif.as_ref()?
            .get_field(tag, exif::In::PRIMARY)
            .map(|field| field.display_value().to_string())
    };
    let date = field(exif::Tag::DateTimeOriginal).map(|raw| {
        // „2024-03-12 14:22:01" → „12.03.2024 14:22".
        let mut parts = raw.splitn(2, ' ');
        let date_part = parts.next().unwrap_or("").replace('-', ".");
        let time_part = parts.next().unwrap_or_default();
        let mut date_bits = date_part.split('.').collect::<Vec<_>>();
        let reordered = if date_bits.len() == 3 {
            date_bits.reverse();
            date_bits.join(".")
        } else {
            date_part
        };
        let time_short = time_part
            .split(':')
            .take(2)
            .collect::<Vec<_>>()
            .join(":");
        if time_short.is_empty() {
            reordered
        } else {
            format!("{reordered} {time_short}")
        }
    });
    let make = field(exif::Tag::Make).unwrap_or_default();
    let model = field(exif::Tag::Model).unwrap_or_default();
    let camera = format!("{make} {model}").trim().to_string();
    Some(PhotoMeta {
        width,
        height,
        bytes: metadata.len(),
        date,
        camera: (!camera.is_empty()).then_some(camera),
    })
}

/// Bilddatei anhand der Endung erkennen (png, jpg, jpeg, webp).
pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp"))
}

/// Bereits hinterlegte Projektbilder auflisten (relativ, neueste zuerst),
/// damit sie wiederverwendet statt erneut importiert werden können.
pub fn list_media_images(media_base: &Path) -> Vec<String> {
    let dir = media_base.join("media");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut images: Vec<(std::time::SystemTime, String)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            if name.starts_with('.') || !is_image_file(Path::new(&name)) {
                return None;
            }
            let modified = entry
                .metadata()
                .ok()?
                .modified()
                .unwrap_or(std::time::UNIX_EPOCH);
            Some((modified, format!("media/{name}")))
        })
        .collect();
    images.sort_by(|a, b| b.cmp(a));
    images.into_iter().map(|(_, relative)| relative).collect()
}

/// Bild aus der Zwischenablage als PNG lesen (nur Desktop).
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn read_clipboard_png() -> Option<Vec<u8>> {
    let image = arboard::Clipboard::new().ok()?.get_image().ok()?;
    let mut buffer = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut buffer);
    use image::ImageEncoder;
    encoder
        .write_image(
            &image.bytes,
            image.width as u32,
            image.height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .ok()?;
    Some(buffer)
}

/// PNG-Bytes anhand ihres Inhalts-Hashs im Medienordner ablegen (keine
/// Duplikate) und relativen Pfad liefern.
pub fn import_image_bytes(library: &Path, png_bytes: &[u8]) -> Option<String> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    png_bytes.hash(&mut hasher);
    let relative = format!("media/{:016x}.png", hasher.finish());
    let target = library.join(&relative);
    if !target.exists() {
        fs::create_dir_all(target.parent()?).ok()?;
        fs::write(&target, png_bytes).ok()?;
    }
    Some(relative)
}

/// Foto in den Medien-Basisordner kopieren und relativen Pfad liefern.
/// Der Dateiname ist ein Inhalts-Hash, doppelte Bilder werden so vermieden.
pub fn import_media_file(library: &Path, source: &Path) -> Option<String> {
    let (relative, target) = media_target(library, source)?;
    if !target.exists() {
        fs::create_dir_all(target.parent()?).ok()?;
        fs::copy(source, &target).ok()?;
    }
    Some(relative)
}

/// Schneller UI-Import: liefert sofort den Zielpfad, kopiert das Original aber
/// im Hintergrund. Galerie/Profil zeigen bis dahin ihren Platzhalter/Cache.
pub fn import_media_file_async(
    ctx: &egui::Context,
    library: &Path,
    source: &Path,
) -> Option<String> {
    let (relative, target) = media_target(library, source)?;
    if target.exists() {
        return Some(relative);
    }
    let source = source.to_path_buf();
    let key = format!("copy:{}", target.display());
    let target_for_job = target.clone();
    spawn_thumb_job(key, ctx.clone(), move || {
        if let Some(parent) = target_for_job.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::copy(source, target_for_job);
    });
    Some(relative)
}

fn media_target(library: &Path, source: &Path) -> Option<(String, PathBuf)> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let metadata = fs::metadata(source).ok()?;
    source.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .hash(&mut hasher);
    let digest = hasher.finish();
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .unwrap_or_else(|| "jpg".into());
    let relative = format!("media/{digest:016x}.{extension}");
    let target = library.join(&relative);
    Some((relative, target))
}

/// Textur für das Profilbild einer Person (lazy geladen und gecached).
/// Relative Pfade gelten gegenüber `media_base` (= `MiniGramps::library`).
#[allow(dead_code)]
pub fn photo_texture<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
) -> Option<&'a TextureHandle> {
    photo_texture_with_limit(ctx, person, cache, media_base, None, "")
}

#[allow(dead_code)]
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

/// Version der Thumb-Pipeline (Quelle, Filter). Bei Änderung werden alle
/// Vorschaubilder einmalig neu erzeugt; alte Dateien räumt die Bereinigung ab.
const THUMB_VERSION: u32 = 1;

fn thumb_key(person: &Person, size: u32, round: bool) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    THUMB_VERSION.hash(&mut hasher);
    person.photo.hash(&mut hasher);
    size.hash(&mut hasher);
    round.hash(&mut hasher);
    if round {
        if let Some(crop) = &person.photo_crop {
            // Rasterung vermeidet beim Ziehen hunderte fast identische Cache-Dateien.
            ((crop.x * 20.0).round() as i32).hash(&mut hasher);
            ((crop.y * 20.0).round() as i32).hash(&mut hasher);
            ((crop.zoom * 20.0).round() as i32).hash(&mut hasher);
        }
    }
    format!("{:016x}", hasher.finish())
}

fn thumb_path(media_base: &Path, key: &str) -> PathBuf {
    media_base.join("media").join(".thumbs").join(key)
}

fn spawn_thumb_job(key: String, ctx: egui::Context, job: impl FnOnce() + Send + 'static) {
    if ACTIVE_THUMBS.load(Ordering::Relaxed) >= MAX_ACTIVE_THUMBS {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        return;
    }
    let pending = PENDING_THUMBS.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
    if !pending
        .lock()
        .ok()
        .is_some_and(|mut set| set.insert(key.clone()))
    {
        return;
    }
    ACTIVE_THUMBS.fetch_add(1, Ordering::Relaxed);
    thread::spawn(move || {
        job();
        ACTIVE_THUMBS.fetch_sub(1, Ordering::Relaxed);
        if let Some(pending) = PENDING_THUMBS.get() {
            if let Ok(mut set) = pending.lock() {
                set.remove(&key);
            }
        }
        ctx.request_repaint();
    });
}

/// Schonend verkleinern: schrittweise halbieren und erst den letzten Schritt
/// mit Lanczos3 auf die exakte Zielgröße rechnen. Einstufiges Verkleinern
/// großer Faktoren (z. B. 4000px → 100px) wird sonst sichtbar matschig.
fn downscale(image: image::DynamicImage, target_w: u32, target_h: u32) -> RgbaImage {
    let target_w = target_w.max(1);
    let target_h = target_h.max(1);
    let mut current = image;
    while current.width() / 2 >= target_w && current.height() / 2 >= target_h {
        let (w, h) = (current.width() / 2, current.height() / 2);
        current = image::DynamicImage::ImageRgba8(image::imageops::resize(
            &current,
            w,
            h,
            FilterType::Triangle,
        ));
    }
    image::imageops::resize(&current, target_w, target_h, FilterType::Lanczos3)
}

fn resize_short_side(image: image::DynamicImage, short_side: u32) -> RgbaImage {
    let (w, h) = image.dimensions();
    let shortest = w.min(h).max(1);
    if shortest <= short_side {
        return image.to_rgba8();
    }
    let scale = short_side as f32 / shortest as f32;
    let width = (w as f32 * scale).round().max(1.0) as u32;
    let height = (h as f32 * scale).round().max(1.0) as u32;
    downscale(image, width, height)
}

fn draw_avatar_placeholder(
    ui: &mut egui::Ui,
    person: &Person,
    size: f32,
    sense: Sense,
    offset: Vec2,
) -> egui::Response {
    let (response, rect) = allocate_avatar(ui, size, sense, offset);
    let painter = ui.painter();
    painter.circle_filled(
        rect.center(),
        size / 2.0,
        Color32::from_rgb(55, 91, 101),
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        initials(person),
        FontId::proportional(size * 0.32),
        Color32::WHITE,
    );
    response
}

/// Live-Editor-Avatar: benutzt eine normale Textur und UV-Cropping, damit
/// Ziehen/Zoomen sofort sichtbar ist. Persistente runde PNGs bleiben für
/// Graph/Listen zuständig.
pub fn avatar_ui_live(
    ui: &mut egui::Ui,
    person: &Person,
    cache: &mut HashMap<String, TextureHandle>,
    media_base: &Path,
    size: f32,
) -> egui::Response {
    if let Some(texture) = photo_preview_texture(ui.ctx(), person, cache, media_base) {
        let tv = texture.size_vec2();
        let aspect = tv.x / tv.y.max(1.0);
        ui.add(
            egui::Image::from_texture(texture)
                .fit_to_exact_size(Vec2::splat(size))
                .maintain_aspect_ratio(false)
                .uv(cover_uv(aspect, person.photo_crop.as_ref()))
                .corner_radius(size / 2.0)
                .sense(Sense::click_and_drag()),
        )
    } else {
        draw_avatar_placeholder(ui, person, size, Sense::click_and_drag(), Vec2::ZERO)
    }
}

fn ensure_gallery_thumb(
    ctx: &egui::Context,
    media_base: &Path,
    person: &Person,
    size: u32,
) -> Option<PathBuf> {
    let key = format!("gallery-{}.png", thumb_key(person, size, false));
    let target = thumb_path(media_base, &key);
    if target.exists() {
        return Some(target);
    }
    let source = media_path(media_base, person.photo.as_deref()?);
    let request_key = format!("job:{key}");
    let target_for_job = target.clone();
    spawn_thumb_job(request_key, ctx.clone(), move || {
        if let Some(parent) = target_for_job.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(image) = image::open(source) {
            let _ = resize_short_side(image, size).save(target_for_job);
        }
    });
    None
}

/// Sichert eine 720p-Kopie (max. 1280px an der längeren Kante) als dauerhaftes Thumbnail auf der Festplatte.
/// Längste Kante der Vollbild-Variante für die Großansicht.
const LARGE_MAX_SIDE: u32 = 2560;

/// Dateiname der Vollbild-Variante (eine Stelle für alle Nutzer).
fn large_thumb_filename(person: &Person) -> String {
    format!("large-2k-{}.png", thumb_key(person, LARGE_MAX_SIDE, false))
}

pub fn ensure_large_thumb(
    ctx: &egui::Context,
    media_base: &Path,
    person: &Person,
) -> Option<PathBuf> {
    let key = large_thumb_filename(person);
    let target = thumb_path(media_base, &key);
    if target.exists() {
        return Some(target);
    }
    let source = media_path(media_base, person.photo.as_deref()?);
    let request_key = format!("job:{key}");
    let target_for_job = target.clone();
    spawn_thumb_job(request_key, ctx.clone(), move || {
        if let Some(parent) = target_for_job.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(image) = image::open(source) {
            let (w, h) = image.dimensions();
            let max_side = w.max(h);
            let resized = if max_side > LARGE_MAX_SIDE {
                let scale = LARGE_MAX_SIDE as f32 / max_side as f32;
                let nw = (w as f32 * scale).round().max(1.0) as u32;
                let nh = (h as f32 * scale).round().max(1.0) as u32;
                downscale(image, nw, nh)
            } else {
                image.to_rgba8()
            };
            let _ = resized.save(target_for_job);
        }
    });
    None
}

fn ensure_round_avatar(
    ctx: &egui::Context,
    media_base: &Path,
    person: &Person,
    size: u32,
) -> Option<PathBuf> {
    let key = format!("avatar2-{}.png", thumb_key(person, size, true));
    let target = thumb_path(media_base, &key);
    if target.exists() {
        return Some(target);
    }
    // Immer vom Original zuschneiden und erst danach herunterrechnen —
    // nie vom bereits verkleinerten Vorschaubild (Qualitätsverlust).
    let source = media_path(media_base, person.photo.as_deref()?);
    let crop = person.photo_crop;
    let request_key = format!("job:{key}");
    let target_for_job = target.clone();
    spawn_thumb_job(request_key, ctx.clone(), move || {
        if let Ok(image) = image::open(source) {
            let _ = write_round_avatar_image(image, crop, size, &target_for_job);
        }
    });
    None
}

fn write_round_avatar_image(
    image: image::DynamicImage,
    crop: Option<PhotoCrop>,
    size: u32,
    target: &Path,
) -> Option<()> {
    let (w, h) = image.dimensions();
    let aspect = w as f32 / h.max(1) as f32;
    let uv = cover_uv(aspect, crop.as_ref());
    let left = (uv.left().clamp(0.0, 1.0) * w as f32).round() as u32;
    let top = (uv.top().clamp(0.0, 1.0) * h as f32).round() as u32;
    let right = (uv.right().clamp(0.0, 1.0) * w as f32).round() as u32;
    let bottom = (uv.bottom().clamp(0.0, 1.0) * h as f32).round() as u32;
    let crop_w = right.saturating_sub(left).max(1);
    let crop_h = bottom.saturating_sub(top).max(1);
    let cropped = image.crop_imm(left, top, crop_w, crop_h);
    let mut avatar: RgbaImage = downscale(cropped, size, size);
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
    avatar.save(target).ok()?;
    Some(())
}

/// Avatar sofort aus dem kleinen Galerie-Cache erzeugen. Das ist der schnelle
/// Speichern-Pfad nach Crop-Aenderungen; kein Warten auf Originalbild-Jobs.
pub fn write_round_avatar_now(media_base: &Path, person: &Person) -> Option<()> {
    let avatar_size = 100;
    let target = thumb_path(
        media_base,
        &format!("avatar2-{}.png", thumb_key(person, avatar_size, true)),
    );
    // Immer vom Original zuschneiden und erst danach herunterrechnen.
    let source = media_path(media_base, person.photo.as_deref()?);
    write_round_avatar_image(
        image::open(source).ok()?,
        person.photo_crop,
        avatar_size,
        &target,
    )
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

fn cached_texture_with_prefix<'a>(
    cache: &'a HashMap<String, TextureHandle>,
    prefix: &str,
) -> Option<&'a TextureHandle> {
    cache
        .iter()
        .find_map(|(key, texture)| key.starts_with(prefix).then_some(texture))
}

/// Vorschau-Textur für kleine Listen/Thumbnails.
pub fn photo_preview_texture<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
) -> Option<&'a TextureHandle> {
    preview_texture_sized(ctx, person, cache, media_base, 150, "preview")
}

/// Kürzeste Kante der Kartenvariante für die herausgezoomte Baumansicht
/// (150px wirken dort auf Kartengröße hochskaliert matschig).
const CARD_THUMB_SHORT_SIDE: u32 = 480;

/// Dateiname der Kartenvariante (eine Stelle für alle Nutzer).
fn card_thumb_filename(person: &Person) -> String {
    format!(
        "card-480-{}.png",
        thumb_key(person, CARD_THUMB_SHORT_SIDE, false)
    )
}

/// Bildfüllende Textur für herausgezoomte Baumkarten (480px statt 150px).
pub fn photo_card_texture<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
) -> Option<&'a TextureHandle> {
    preview_texture_sized(ctx, person, cache, media_base, CARD_THUMB_SHORT_SIDE, "card")
}

fn preview_texture_sized<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
    thumb_size: u32,
    prefix: &str,
) -> Option<&'a TextureHandle> {
    let key_prefix = format!("{prefix}:{}:", person.id);
    let path = match ensure_gallery_thumb(ctx, media_base, person, thumb_size) {
        Some(path) => path,
        None => return cached_texture_with_prefix(cache, &key_prefix),
    };
    texture_from_file(
        ctx,
        cache,
        format!(
            "{prefix}:{}:{}",
            person.id,
            thumb_key(person, thumb_size, false)
        ),
        path,
    )
}

/// Zustand der Lightbox-Textur während des asynchronen Ladens.
pub enum LightboxState<'a> {
    /// Vollbild-Textur ist bereit.
    Ready(&'a TextureHandle),
    /// Noch nicht fertig; `preview` (klein, sofort) anzeigen wo verfügbar.
    Loading,
}

/// Ergebnis eines asynchronen Lightbox-Dekodiervorgangs.
pub struct AsyncImage {
    /// Cache-Schlüssel (`lightbox-<id>`).
    pub key: String,
    /// Dekodierte Pixel (volles Bild, unverkleinert).
    pub image: egui::ColorImage,
}

/// Fragt die vorbereitete Vorschau-Textur ab (blockiert nie) und stellt einen
/// asynchronen Vollbild-Dekodierauftrag; Ergebnis landet über den shared
/// Sender `tx`. Liefert `Ready`, sobald der Vollbild-Cache gefüllt ist.
pub fn lightbox_texture_async<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
    loading: &mut std::collections::HashSet<String>,
    tx: &mpsc::Sender<AsyncImage>,
) -> LightboxState<'a> {
    let cache_key = format!("lightbox-{}", person.id);
    if cache.contains_key(&cache_key) {
        return LightboxState::Ready(cache.get(&cache_key).unwrap());
    }
    // Sofort (nicht blockierend) das kleine Vorschaubild anzeigen.
    photo_preview_texture(ctx, person, cache, media_base);
    // Noch kein Job angelaufen → Vollbild im Hintergrund dekodieren.
    if !loading.contains(&cache_key) {
        // Wir laden AUSSCHLIESSLICH die HD-Variante (720p) für die Galerie.
        // Falls sie noch nicht existiert, stößt ensure_large_thumb die Generierung an.
        if let Some(large_path) = ensure_large_thumb(ctx, media_base, person) {
            if let Some(guard) = load_guard() {
                loading.insert(cache_key.clone());
                let tx = tx.clone();
                let key = cache_key.clone();
                let ctx_clone = ctx.clone();
                thread::spawn(move || {
                    let _guard = guard;
                    if let Ok(image) = image::open(&large_path) {
                        // Da die Datei selbst schon 720p (1280px max) ist, können wir sie direkt als ColorImage laden
                        // (kein weiteres resizen im RAM nötig, da sie schon perfekt skaliert ist!).
                        let size = [image.width() as usize, image.height() as usize];
                        let rgba = image.to_rgba8();
                        let ci = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                        let _ = tx.send(AsyncImage { key, image: ci });
                    }
                    ctx_clone.request_repaint();
                });
            }
        }
    }
    LightboxState::Loading
}

/// Liefert die Bereitschaft der Vollbild-Textur (ob schon gecacht) zurück.
#[allow(dead_code)]
pub fn lightbox_ready<'a>(
    person: &Person,
    cache: &'a HashMap<String, TextureHandle>,
) -> Option<&'a TextureHandle> {
    cache.get(&format!("lightbox-{}", person.id))
}

/// Alle eingegangenen asynchronen Bilder in den Textur-Cache übernehmen.
pub fn drain_lightbox_textures(
    rx: &mpsc::Receiver<AsyncImage>,
    cache: &mut HashMap<String, TextureHandle>,
    ctx: &egui::Context,
    loading: &mut std::collections::HashSet<String>,
) {
    while let Ok(AsyncImage { key, image }) = rx.try_recv() {
        loading.remove(&key);
        cache.insert(
            key.clone(),
            ctx.load_texture(key, image, Default::default()),
        );
    }
}

/// Sperre: begrenzt parallele asynchrone Bild-Dekodierungen auf
/// `MAX_ACTIVE_THUMBS`. Gibt `None`, wenn bereits genug laufen.
fn load_guard() -> Option<LoadGuard> {
    let active = ACTIVE_THUMBS.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |a| {
        (a < MAX_ACTIVE_THUMBS).then(|| a + 1)
    });
    match active {
        Ok(_) => Some(LoadGuard),
        Err(_) => None,
    }
}

struct LoadGuard;

impl Drop for LoadGuard {
    fn drop(&mut self) {
        ACTIVE_THUMBS.fetch_sub(1, Ordering::Relaxed);
    }
}

pub fn round_avatar_texture<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
) -> Option<&'a TextureHandle> {
    let avatar_size = 100;
    let prefix = format!("avatar:{}:", person.id);
    let path = match ensure_round_avatar(ctx, media_base, person, avatar_size) {
        Some(path) => path,
        None => return cached_texture_with_prefix(cache, &prefix),
    };
    texture_from_file(
        ctx,
        cache,
        format!(
            "avatar:{}:{}",
            person.id,
            thumb_key(person, avatar_size, true)
        ),
        path,
    )
}

/// Nur vorhandenes rundes Avatar-Thumbnail laden. Wichtig für den Graphen:
/// dort dürfen beim Zeichnen vieler Personen keine neuen Bildjobs entstehen.
pub fn round_avatar_texture_cached<'a>(
    ctx: &egui::Context,
    person: &Person,
    cache: &'a mut HashMap<String, TextureHandle>,
    media_base: &Path,
) -> Option<&'a TextureHandle> {
    let avatar_size = 100;
    let key = thumb_key(person, avatar_size, true);
    let texture_key = format!("avatar:{}:{key}", person.id);
    if cache.contains_key(&texture_key) {
        return cache.get(&texture_key);
    }
    let path = thumb_path(media_base, &format!("avatar2-{key}.png"));
    path.exists()
        .then_some(path)
        .and_then(|path| texture_from_file(ctx, cache, texture_key, path))
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
        let target_aspect = size.x / size.y.max(1.0);
        ui.add(
            egui::Image::from_texture(texture)
                .fit_to_exact_size(size)
                .maintain_aspect_ratio(false)
                .uv(cover_uv_to(aspect, target_aspect, None))
                .corner_radius(5.0)
                .sense(Sense::click()),
        )
    } else {
        ui.allocate_response(size, Sense::click())
    }
}

/// Initialen für den Platzhalter-Avatar (maximal 2 Buchstaben).
/// Sonderzeichen werden nie als Initial genommen, sondern übersprungen
/// (z. B. führende Klammer in „(Sophia)" → „S").
pub fn initials(person: &Person) -> String {
    fn first_letter(text: &str) -> Option<char> {
        text.chars().find(|c| c.is_alphabetic())
    }
    // Erster Buchstabe aus Rufname (falls vorhanden) sonst erstem Vornamen,
    // plus erster Buchstabe des Nachnamens.
    let first = {
        let call = person.call_name.trim();
        if call.is_empty() {
            person.given_name.split_whitespace().next().unwrap_or("")
        } else {
            call
        }
    };
    let family = person.family_name.split_whitespace().next().unwrap_or("");
    let mut out = String::new();
    if let Some(c) = first_letter(first) {
        out.extend(c.to_uppercase());
    }
    if let Some(c) = first_letter(family) {
        out.extend(c.to_uppercase());
    }
    if out.is_empty() {
        out = person
            .display_name()
            .chars()
            .find(|c| c.is_alphabetic())
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default();
    }
    out
}

/// Reserviert auch den Randabstand im Layout, damit der Avatar vollständig
/// innerhalb seines Zeilen- und Clipbereichs liegt.
fn allocate_avatar(
    ui: &mut egui::Ui,
    size: f32,
    sense: Sense,
    offset: Vec2,
) -> (egui::Response, egui::Rect) {
    let padding = offset.max(Vec2::ZERO);
    let (allocated, response) = ui.allocate_exact_size(Vec2::splat(size) + padding, sense);
    let rect = egui::Rect::from_min_size(allocated.min + padding, Vec2::splat(size));
    (response, rect)
}

/// Avatar-Vorschau: runder Avatar aus Cache oder Platzhalter.
/// `offset` reserviert zusätzlichen Platz links und oberhalb des Bildes.
pub fn avatar_ui_preview(
    ui: &mut egui::Ui,
    person: &Person,
    cache: &mut HashMap<String, TextureHandle>,
    media_base: &Path,
    size: f32,
    offset: Vec2,
) -> egui::Response {
    if let Some(texture) = round_avatar_texture(ui.ctx(), person, cache, media_base) {
        let (response, rect) = allocate_avatar(ui, size, Sense::click_and_drag(), offset);
        let painter = ui.painter();
        painter.circle_filled(
            rect.center(),
            size / 2.0,
            Color32::from_black_alpha(24),
        );
        painter.image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.circle_stroke(
            rect.center(),
            size / 2.0,
            egui::Stroke::new(1.0, Color32::from_white_alpha(40)),
        );
        response
    } else {
        draw_avatar_placeholder(ui, person, size, Sense::click_and_drag(), offset)
    }
}

/// Alle Texturen einer Person aus dem Cache entfernen.
pub fn clear_person_photo_cache(cache: &mut HashMap<String, TextureHandle>, person_id: &str) {
    cache.retain(|key, _| {
        key != person_id
            && !key.starts_with(&format!("preview:{person_id}:"))
            && !key.starts_with(&format!("card:{person_id}:"))
            && !key.starts_with(&format!("avatar:{person_id}:"))
            && !key.starts_with(&format!("gallery-{person_id}-"))
    });
}

/// Entfernt ungenutzte Lightbox-Vollbilder aus dem Cache, um GPU-Speicher freizugeben.
pub fn clear_lightbox_cache(
    cache: &mut HashMap<String, TextureHandle>,
    loading: &mut std::collections::HashSet<String>,
    keep_path: Option<&str>,
) {
    let keep_key = keep_path.map(|path| format!("lightbox-{path}"));
    cache.retain(|key, _| {
        if key.starts_with("lightbox-") {
            if let Some(ref keep) = keep_key {
                key == keep
            } else {
                false
            }
        } else {
            true
        }
    });
    loading.retain(|key| {
        if key.starts_with("lightbox-") {
            if let Some(ref keep) = keep_key {
                key == keep
            } else {
                false
            }
        } else {
            true
        }
    });
}

/// Scannt das Medienverzeichnis und löscht alle verwaisten (nicht mehr referenzierten) Originaldateien und Thumbnails.
pub fn cleanup_unused_media(media_base: &Path, data: &TreeData) {
    let mut referenced: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut valid_thumbs: std::collections::HashSet<String> = std::collections::HashSet::new();

    for p in &data.people {
        if let Some(ref photo) = p.photo {
            if let Some(filename) = Path::new(photo).file_name().and_then(|n| n.to_str()) {
                referenced.insert(filename.to_string());
            }
            // Berechne valide Thumbnail-Dateinamen für das Profilbild
            valid_thumbs.insert(format!("gallery-{}.png", thumb_key(p, 150, false)));
            valid_thumbs.insert(format!("avatar2-{}.png", thumb_key(p, 100, true)));
            valid_thumbs.insert(card_thumb_filename(p));
            valid_thumbs.insert(large_thumb_filename(p));
        }
        for entry in p
            .gallery
            .iter()
            .chain(p.documents.iter().map(|document| &document.path))
        {
            if let Some(filename) = Path::new(entry).file_name().and_then(|n| n.to_str()) {
                referenced.insert(filename.to_string());
            }
            let mut gp = p.clone();
            gp.photo = Some(entry.clone());
            // Berechne valide Thumbnail-Dateinamen für dieses Galeriebild
            valid_thumbs.insert(format!("gallery-{}.png", thumb_key(&gp, 150, false)));
            valid_thumbs.insert(card_thumb_filename(&gp));
            valid_thumbs.insert(large_thumb_filename(&gp));
        }
    }

    // 1. Verwaiste Originaldateien löschen
    // Hinweis: Automatische Deletion von Originaldateien deaktiviert, um Datenverlust bei mehreren Projekten im selben Speicherort zu verhindern!
    let media_dir = media_base.join("media");

    // 2. Verwaiste Thumbnails und Cache-Bilder löschen
    let thumbs_dir = media_dir.join(".thumbs");
    if let Ok(entries) = std::fs::read_dir(&thumbs_dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_file() {
                if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                    if !valid_thumbs.contains(filename) {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }
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

    #[test]
    fn rotate_swaps_image_dimensions() {
        use image::{GenericImageView, RgbaImage};
        let temp = std::env::temp_dir().join("minigramps-rotate-test");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(temp.join("media")).unwrap();
        RgbaImage::new(4, 2).save(temp.join("media").join("pic.png")).unwrap();
        assert!(rotate_image_file_90(&temp, "media/pic.png"));
        let rotated = image::open(temp.join("media").join("pic.png")).unwrap();
        assert_eq!(rotated.dimensions(), (2, 4));
        assert!(!rotate_image_file_90(&temp, "media/missing.png"));
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn rotate_ccw_swaps_image_dimensions() {
        use image::{GenericImageView, RgbaImage};
        let temp = std::env::temp_dir().join("minigramps-rotate-ccw-test");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(temp.join("media")).unwrap();
        RgbaImage::new(4, 2).save(temp.join("media").join("pic.png")).unwrap();
        assert!(rotate_image_file_ccw(&temp, "media/pic.png"));
        let rotated = image::open(temp.join("media").join("pic.png")).unwrap();
        assert_eq!(rotated.dimensions(), (2, 4));
        assert!(!rotate_image_file_ccw(&temp, "media/missing.png"));
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn median_removes_scanline_and_keeps_flat_areas() {
        use image::{GenericImageView, RgbaImage};
        // Graue Fläche mit einer weißen Scanlinie in der Mitte.
        let striped = RgbaImage::from_fn(7, 7, |_x, y| {
            if y == 3 {
                image::Rgba([255, 255, 255, 255])
            } else {
                image::Rgba([100, 100, 100, 255])
            }
        });
        let cleaned = median_3x3(&striped);
        assert_eq!(cleaned.dimensions(), (7, 7));
        // Linie getilgt (3 weiße gegen 6 graue Nachbarn verlieren).
        assert_eq!(cleaned.get_pixel(3, 3), &image::Rgba([100, 100, 100, 255]));
        // Fläche unverändert.
        assert_eq!(cleaned.get_pixel(0, 0), &image::Rgba([100, 100, 100, 255]));
        assert_eq!(cleaned.get_pixel(6, 6), &image::Rgba([100, 100, 100, 255]));
        // Reine Fläche bleibt exakt gleich.
        let flat = RgbaImage::from_pixel(5, 5, image::Rgba([42, 42, 42, 255]));
        assert_eq!(median_3x3(&flat), flat);
        // Zu kleine Bilder kommen unverändert zurück.
        let tiny = RgbaImage::new(2, 2);
        assert_eq!(median_3x3(&tiny), tiny);
    }

    #[test]
    fn detects_image_files_by_extension() {
        assert!(is_image_file(Path::new("foto.JPG")));
        assert!(is_image_file(Path::new("bild.webp")));
        assert!(!is_image_file(Path::new("baum.ged")));
        assert!(!is_image_file(Path::new("ohne_endung")));
    }

    #[test]
    fn thumb_deletion_counts_and_recreates() {
        let temp = std::env::temp_dir().join("minigramps-thumb-delete-test");
        let _ = fs::remove_dir_all(&temp);
        let thumbs = temp.join("media").join(".thumbs");
        fs::create_dir_all(&thumbs).unwrap();
        fs::write(thumbs.join("avatar2-abc.png"), b"a").unwrap();
        fs::write(thumbs.join("gallery-def.png"), b"g").unwrap();
        fs::write(thumbs.join("note.txt"), b"x").unwrap();
        assert_eq!(delete_avatar_thumbs(&temp), 1);
        assert!(!thumbs.join("avatar2-abc.png").exists());
        assert!(thumbs.join("gallery-def.png").exists());
        assert_eq!(delete_all_thumbs(&temp), 2);
        assert!(!thumbs.join("gallery-def.png").exists());
        assert_eq!(delete_all_thumbs(&temp.join("missing-dir")), 0);
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn downscale_hits_exact_target_size() {
        use image::RgbaImage;
        let big = image::DynamicImage::ImageRgba8(RgbaImage::new(512, 300));
        let small = downscale(big, 100, 100);
        assert_eq!((small.width(), small.height()), (100, 100));
        let odd = image::DynamicImage::ImageRgba8(RgbaImage::new(5, 5));
        let tiny = downscale(odd, 2, 2);
        assert_eq!((tiny.width(), tiny.height()), (2, 2));
    }

    #[test]
    fn lists_only_stored_images_newest_first() {
        let temp = std::env::temp_dir().join("minigramps-list-images-test");
        let _ = fs::remove_dir_all(&temp);
        let media = temp.join("media");
        fs::create_dir_all(media.join(".thumbs")).unwrap();
        fs::write(media.join("a.png"), b"a").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        fs::write(media.join("b.jpg"), b"b").unwrap();
        fs::write(media.join("notes.txt"), b"x").unwrap();
        let listed = list_media_images(&temp);
        assert_eq!(listed, vec!["media/b.jpg".to_string(), "media/a.png".to_string()]);
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn large_thumb_filename_is_shared_and_removable() {
        let mut p = crate::model::person("x", "A", "B", "", crate::model::Gender::Female);
        p.photo = Some("media/pic.png".into());
        let name = large_thumb_filename(&p);
        assert!(name.starts_with("large-2k-") && name.ends_with(".png"));
        let temp = std::env::temp_dir().join("minigramps-large-thumb-test");
        let _ = fs::remove_dir_all(&temp);
        let thumbs = temp.join("media").join(".thumbs");
        fs::create_dir_all(&thumbs).unwrap();
        fs::write(thumbs.join(&name), b"x").unwrap();
        delete_person_thumbs(&temp, &p);
        assert!(!thumbs.join(&name).exists());
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn card_thumb_filename_is_shared_and_removable() {
        let mut p = crate::model::person("x", "A", "B", "", crate::model::Gender::Female);
        p.photo = Some("media/pic.png".into());
        let name = card_thumb_filename(&p);
        assert!(name.starts_with("card-480-") && name.ends_with(".png"));
        let temp = std::env::temp_dir().join("minigramps-card-thumb-test");
        let _ = fs::remove_dir_all(&temp);
        let thumbs = temp.join("media").join(".thumbs");
        fs::create_dir_all(&thumbs).unwrap();
        fs::write(thumbs.join(&name), b"x").unwrap();
        delete_person_thumbs(&temp, &p);
        assert!(!thumbs.join(&name).exists());
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn photo_meta_reads_dimensions_without_exif() {
        use image::RgbaImage;
        let temp = std::env::temp_dir().join("minigramps-meta-test");
        let _ = fs::remove_dir_all(&temp);
        let media = temp.join("media");
        fs::create_dir_all(&media).unwrap();
        RgbaImage::new(40, 30).save(media.join("pic.png")).unwrap();
        let meta = read_photo_meta(&temp, "media/pic.png").unwrap();
        assert_eq!((meta.width, meta.height), (40, 30));
        assert!(meta.date.is_none() && meta.camera.is_none());
        assert!(meta.display_line().starts_with("40 × 30 · "));
        assert!(read_photo_meta(&temp, "media/missing.png").is_none());
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn initials_prefer_call_name() {
        let mut p = crate::model::person(
            "x",
            "Hans Jürgen",
            "Bauke",
            "",
            crate::model::Gender::Male,
        );
        assert_eq!(initials(&p), "HB");
        p.call_name = "Jürgen".into();
        assert_eq!(initials(&p), "JB");
    }

    #[test]
    fn initials_skip_non_letters() {
        let p = crate::model::person(
            "x",
            "(Sophia)",
            "Charlotte",
            "",
            crate::model::Gender::Female,
        );
        assert_eq!(initials(&p), "SC");
    }

    #[test]
    fn avatar_padding_is_reserved_in_layout() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let (response, image) = allocate_avatar(
                    ui, 64.0, Sense::hover(), Vec2::splat(3.0),
                );
                assert_eq!(response.rect.size(), Vec2::splat(67.0));
                assert_eq!(image.size(), Vec2::splat(64.0));
                assert!(response.rect.contains_rect(image));
                assert_eq!(image.min - response.rect.min, Vec2::splat(3.0));
            });
        });
    }
}
