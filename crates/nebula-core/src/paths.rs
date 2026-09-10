use std::path::{Path, PathBuf};

use crate::error::{Error, Result, io};

/// Port of `assertCleanRelativePath` from the Hambugui hardening fork.
pub fn clean_relative(path: &str) -> Result<String> {
    let normalized = path.replace('\\', "/");
    if normalized.is_empty() || normalized.starts_with('/') || has_drive_prefix(&normalized) {
        return Err(Error::UnsafePath(format!(
            "Unsafe absolute artifact path: {path}"
        )));
    }
    if normalized
        .split('/')
        .any(|segment| segment == ".." || segment.contains('\0'))
    {
        return Err(Error::UnsafePath(format!(
            "Unsafe relative artifact path: {path}"
        )));
    }
    let cleaned = normalized
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>()
        .join("/");
    // `.` and `./` name the container, not a file in it. `normalize.rs` rejects
    // them rather than resolving to the root, and so do we.
    if cleaned.is_empty() {
        return Err(Error::UnsafePath(format!(
            "Unsafe relative artifact path: {path}"
        )));
    }
    Ok(cleaned)
}

/// Whether `id` names one directory entry: no separator, no drive letter, no
/// `..`. Shared by `server::create_server` and the `servers[].id` rule in
/// `validate`, which must agree about what a server directory may be called.
pub fn is_single_segment(id: &str) -> bool {
    !id.is_empty() && id != ".." && !id.contains(['/', '\\', ':', '\0'])
}

/// Port of `assertContainedFilesystemPath`: both sides are resolved through the
/// real filesystem so a symlink cannot point outside `root`.
pub fn assert_contained(root: &Path, candidate: &Path) -> Result<()> {
    assert_contained_in(&real_path(root)?, candidate)
}

/// [`assert_contained`] with the container already resolved, so a scan of a
/// thousand files canonicalizes it once instead of a thousand times.
pub fn assert_contained_in(root_real: &Path, candidate: &Path) -> Result<()> {
    let candidate_real = real_path(candidate)?;
    if candidate_real.starts_with(root_real) {
        Ok(())
    } else {
        Err(Error::UnsafePath(format!(
            "Filesystem path escapes {}: {}",
            root_real.display(),
            candidate.display()
        )))
    }
}

pub fn real_path(path: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(path).map_err(io(format!("resolving {}", path.display())))
}

fn has_drive_prefix(path: &str) -> bool {
    let mut chars = path.chars();
    matches!((chars.next(), chars.next()), (Some(letter), Some(':')) if letter.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_clean_relative_paths() {
        assert_eq!(
            clean_relative("config/example.cfg").unwrap(),
            "config/example.cfg"
        );
        assert_eq!(
            clean_relative("config\\example.cfg").unwrap(),
            "config/example.cfg"
        );
        assert_eq!(clean_relative("./a//b.cfg").unwrap(), "a/b.cfg");
    }

    #[test]
    fn rejects_unsafe_candidates() {
        for candidate in [
            "../escape.jar",
            "/abs.jar",
            "C:/abs.jar",
            "C:drive-relative.jar",
            "//server/share.jar",
            "\\\\server\\share.jar",
            "\\?\\C:\\bad.jar",
            "",
            "a/\0/b.jar",
            // Cleans away to nothing, which would otherwise name the container.
            ".",
            "./",
            "././.",
        ] {
            assert!(
                clean_relative(candidate).is_err(),
                "{candidate} should be rejected"
            );
        }
    }

    #[test]
    fn accepts_only_single_path_segments() {
        for id in ["Test", "my-pack", "a.b", "한글"] {
            assert!(is_single_segment(id), "{id} should be accepted");
        }
        for id in [
            "", "..", "../evil", "a/b", "a\\b", "C:/tmp/x", "a:b", "a\0b",
        ] {
            assert!(!is_single_segment(id), "{id} should be rejected");
        }
    }

    #[test]
    fn detects_escapes_from_a_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let inside_file = root.path().join("inside.jar");
        let outside_file = outside.path().join("payload.jar");
        std::fs::write(&inside_file, b"in").unwrap();
        std::fs::write(&outside_file, b"out").unwrap();

        assert_contained(root.path(), &inside_file).unwrap();
        assert!(assert_contained(root.path(), &outside_file).is_err());
    }
}
