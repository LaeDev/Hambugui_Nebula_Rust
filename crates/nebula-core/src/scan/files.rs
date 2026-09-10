//! Port of `File.struct.ts` (`MiscFileStructure`) including the hardening
//! fork's symlink rejection.

use std::path::Path;

use crate::error::{Error, Result, io};
use crate::spec::{Module, ModuleType};

use super::{FILE_NAME_BLACKLIST, ScanContext, UntrackedMatcher, build_artifact};

pub const STRUCT_ROOT: &str = "files";

pub fn scan_files(ctx: &ScanContext) -> Result<Vec<Module>> {
    let container = ctx.server.files_dir();
    if !container.exists() {
        return Ok(Vec::new());
    }
    let matcher = UntrackedMatcher::new(STRUCT_ROOT, ctx.untracked)?;
    let mut modules = Vec::new();
    scan_dir(ctx, &container, &container, "", &matcher, &mut modules)?;
    Ok(modules)
}

fn scan_dir(
    ctx: &ScanContext,
    container: &Path,
    dir: &Path,
    prefix: &str,
    matcher: &UntrackedMatcher,
    modules: &mut Vec<Module>,
) -> Result<()> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(io(format!("reading {}", dir.display())))? {
        let entry = entry.map_err(io(format!("reading {}", dir.display())))?;
        entries.push((
            entry.file_name().to_string_lossy().into_owned(),
            entry.path(),
        ));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    for (file_name, path) in entries {
        let meta = std::fs::symlink_metadata(&path)
            .map_err(io(format!("inspecting {}", path.display())))?;
        if meta.is_symlink() {
            return Err(Error::UnsafePath(format!(
                "Unsafe symbolic link in files structure: {}",
                path.display()
            )));
        }
        let rel = if prefix.is_empty() {
            file_name.clone()
        } else {
            format!("{prefix}/{file_name}")
        };
        if meta.is_dir() {
            scan_dir(ctx, container, &path, &rel, matcher, modules)?;
            continue;
        }
        if FILE_NAME_BLACKLIST.contains(&file_name.as_str()) {
            continue;
        }
        let untracked = matcher.is_untracked(&rel);
        let artifact = build_artifact(
            container,
            &path,
            meta.len(),
            ctx.server.url(&format!("{STRUCT_ROOT}/{rel}")),
            Some(rel),
            untracked,
            &ctx.policy,
        )?;
        modules.push(Module {
            id: file_name.clone(),
            name: file_name,
            module_type: ModuleType::File,
            classpath: None,
            required: None,
            untracked: untracked.then_some(true),
            artifact,
            sub_modules: None,
        });
    }
    Ok(())
}
