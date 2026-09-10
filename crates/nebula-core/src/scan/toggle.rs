//! Port of `ToggleableModule.struct.ts`: the three namespaces under a mod
//! container and how they map onto the `required` field.

use std::path::Path;

use crate::error::{Result, io};
use crate::spec::{Module, Required};

use super::{Candidate, discover};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    Required,
    OptionalOn,
    OptionalOff,
}

/// Declaration order matters: it is the order candidates are discovered in,
/// before the file-name sort (`Object.values(ToggleableNamespace)` upstream).
pub const NAMESPACES: [Namespace; 3] = [
    Namespace::Required,
    Namespace::OptionalOn,
    Namespace::OptionalOff,
];

impl Namespace {
    pub fn dir_name(self) -> &'static str {
        match self {
            Namespace::Required => "required",
            Namespace::OptionalOn => "optionalon",
            Namespace::OptionalOff => "optionaloff",
        }
    }

    /// `required/` leaves the field absent, `optionalon/` is `{ value: false }`
    /// and `optionaloff/` is `{ value: false, def: false }`.
    pub fn required(self) -> Option<Required> {
        match self {
            Namespace::Required => None,
            Namespace::OptionalOn => Some(Required {
                value: false,
                def: None,
            }),
            Namespace::OptionalOff => Some(Required {
                value: false,
                def: Some(false),
            }),
        }
    }
}

pub fn init_toggleable(container: &Path) -> Result<()> {
    for namespace in NAMESPACES {
        let dir = container.join(namespace.dir_name());
        std::fs::create_dir_all(&dir).map_err(io(format!("creating {}", dir.display())))?;
    }
    Ok(())
}

pub fn discover_toggleable(container: &Path) -> Result<Vec<(Namespace, Candidate)>> {
    let mut candidates = Vec::new();
    for namespace in NAMESPACES {
        for candidate in discover(&container.join(namespace.dir_name()))? {
            candidates.push((namespace, candidate));
        }
    }
    Ok(candidates)
}

/// The file name a module's artifact URL ends with; mods are ordered by it so
/// server owners can control load order.
pub fn url_file_name(module: &Module) -> &str {
    let url = module.artifact.url.as_str();
    match url.rfind('/') {
        Some(slash) => &url[slash + 1..],
        None => url,
    }
}

/// Upstream sorted with `String.prototype.localeCompare` (ICU collation). We
/// lowercase first and fall back to byte order, an accepted divergence that
/// only shows up for names differing in case or leading punctuation.
pub fn finish_modules(mut modules: Vec<Module>) -> Vec<Module> {
    // Cached: the lowercased name is otherwise rebuilt on every comparison.
    modules.sort_by_cached_key(|module| {
        let name = url_file_name(module);
        (name.to_lowercase(), name.to_owned())
    });
    modules
}
