//! Project and jar icons, decoded once into the 72x72 the list rows show.
//!
//! Modrinth icons are cached as PNG under `<root>/cache/icons`, which is
//! outside every directory a distribution scanner walks (reliability rule 5).
//! CurseForge icons **never touch disk**: their terms forbid caching API data,
//! so they live in [`Catalog::icon_memory`] for the session only (D2).
//!
//! The format is sniffed from the magic bytes, never from the URL's
//! extension, because Modrinth serves `.webp` behind `.png` names (D5).

use std::path::{Path, PathBuf};

use image::imageops::FilterType;

use super::{Catalog, CatalogError, ProviderId, http, install};
use crate::error::Error;
use crate::hash;
use crate::layout::Root;

/// The avatar size every row uses (§8.3).
pub const ICON_SIZE: u32 = 72;

/// Decoded RGBA an icon the GUI builds `slint::Image::from_rgba8` from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconBytes {
    pub rgba: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

/// Where an icon lives: a cached PNG path, or bytes that may not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconRef {
    /// A `.png` under `<root>/cache/icons`. The GUI only ever loads these.
    Path(PathBuf),
    /// A CurseForge icon, kept in memory for this session only (D2).
    Rgba(IconBytes),
}

/// Fetches one project icon.
///
/// `key` is the project id, which is what the CurseForge memory cache is keyed
/// by. A Modrinth icon already on disk is used **without a request**, which
/// also keeps Slint's `(path, mtime-second)` image cache from serving a stale
/// bitmap.
pub fn fetch(
    catalog: &Catalog,
    root: &Root,
    provider: ProviderId,
    key: &str,
    icon_url: &str,
) -> Result<IconRef, CatalogError> {
    if provider == ProviderId::CurseForge {
        if let Some(cached) = memory(catalog).get(key) {
            return Ok(IconRef::Rgba(cached.clone()));
        }
        let bytes = http::get_bytes(
            provider,
            icon_url,
            &install::provider_headers(catalog, provider),
        )?;
        let icon = decode(&bytes, icon_url)?;
        memory(catalog).insert(key.to_owned(), icon.clone());
        return Ok(IconRef::Rgba(icon));
    }

    let path = root.icons_cache_dir().join(format!(
        "{}.png",
        hash::digests_of_bytes(icon_url.as_bytes()).sha1
    ));
    if path.is_file() {
        return Ok(IconRef::Path(path));
    }
    let bytes = http::get_bytes(provider, icon_url, &[])?;
    write_png(&decode(&bytes, icon_url)?, &path)?;
    Ok(IconRef::Path(path))
}

/// Caches a jar's own icon, keyed by the digest of the jar so a replaced file
/// never shows the old picture.
pub fn local_icon(root: &Root, jar_path: &Path, bytes: &[u8]) -> Result<PathBuf, CatalogError> {
    let digest = hash::sha1_hex_of_file(jar_path)?;
    let path = root.icons_cache_dir().join(format!("local-{digest}.png"));
    if path.is_file() {
        return Ok(path);
    }
    write_png(&decode(bytes, &jar_path.display().to_string())?, &path)?;
    Ok(path)
}

/// Decodes and resizes to [`ICON_SIZE`]. Lanczos3 because these are
/// downscales of 256-1024 px art and the rows sit next to sharp text.
fn decode(bytes: &[u8], source: &str) -> Result<IconBytes, CatalogError> {
    let decoded = image::load_from_memory(bytes)
        .map_err(|error| Error::Invalid(format!("{source}: {error}")))?
        .resize_exact(ICON_SIZE, ICON_SIZE, FilterType::Lanczos3)
        .into_rgba8();
    Ok(IconBytes {
        w: decoded.width(),
        h: decoded.height(),
        rgba: decoded.into_raw(),
    })
}

/// Writes the icon exactly once. An existing file is never overwritten, so a
/// path handed to Slint keeps its bytes and its mtime.
fn write_png(icon: &IconBytes, path: &Path) -> Result<(), CatalogError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let buffer: image::RgbaImage = image::ImageBuffer::from_raw(icon.w, icon.h, icon.rgba.clone())
        .ok_or_else(|| {
            Error::Invalid(format!("{}: icon buffer is the wrong size", path.display()))
        })?;
    buffer
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|error| Error::Invalid(format!("{}: {error}", path.display())))?;
    Ok(())
}

/// A poisoned icon cache must not take the process down with it.
fn memory(
    catalog: &Catalog,
) -> std::sync::MutexGuard<'_, std::collections::HashMap<String, IconBytes>> {
    catalog
        .icon_memory
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
