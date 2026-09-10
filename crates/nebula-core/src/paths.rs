use std::path::Path;

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
    Ok(normalized
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>()
        .join("/"))
}

/// Port of `assertContainedFilesystemPath`: both sides are resolved through the
/// real filesystem so a symlink cannot point outside `root`.
pub fn assert_contained(root: &Path, candidate: &Path) -> Result<()> {
    let root_real =
        std::fs::canonicalize(root).map_err(io(format!("resolving {}", root.display())))?;
    let candidate_real = std::fs::canonicalize(candidate)
        .map_err(io(format!("resolving {}", candidate.display())))?;
    if candidate_real.starts_with(&root_real) {
        Ok(())
    } else {
        Err(Error::UnsafePath(format!(
            "Filesystem path escapes {}: {}",
            root.display(),
            candidate.display()
        )))
    }
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
        ] {
            assert!(
                clean_relative(candidate).is_err(),
                "{candidate} should be rejected"
            );
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
