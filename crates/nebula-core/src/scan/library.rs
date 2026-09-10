//! Port of `Library.struct.ts`.

use crate::error::Result;
use crate::spec::{Module, ModuleType};

use super::{
    ScanContext, UntrackedMatcher, build_artifact, crude_inference, discover, maven_module_id,
};

pub const STRUCT_ROOT: &str = "libraries";

pub fn scan_libraries(ctx: &ScanContext) -> Result<Vec<Module>> {
    const TYPE: ModuleType = ModuleType::Library;
    // Upstream filters on `TypeMetadata[Library].defaultExtension` ("jar"),
    // without a leading dot.
    let extension = TYPE.default_extension().unwrap_or("jar");

    let container = ctx.server.libraries_dir();
    let matcher = UntrackedMatcher::new(STRUCT_ROOT, ctx.untracked)?;
    let mut modules = Vec::new();
    for candidate in discover(&container)? {
        if !candidate.file_name.to_lowercase().ends_with(extension) {
            continue;
        }
        let (name, version) = crude_inference(&candidate.file_name);
        let untracked = matcher.is_untracked(&candidate.file_name);
        let artifact = build_artifact(
            &container,
            &candidate.path,
            candidate.size,
            ctx.server
                .url(&format!("{STRUCT_ROOT}/{}", candidate.file_name)),
            None,
            untracked,
            &ctx.policy,
        )?;
        modules.push(Module {
            id: maven_module_id(&TYPE.default_group(), &name, &version, TYPE),
            name,
            module_type: TYPE,
            classpath: None,
            required: None,
            untracked: untracked.then_some(true),
            artifact,
            sub_modules: None,
        });
    }
    Ok(modules)
}
