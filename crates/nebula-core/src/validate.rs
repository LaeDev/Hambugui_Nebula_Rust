//! Launcher-contract validation.
//!
//! Everything here walks `serde_json::Value`, never [`crate::spec`]: valid
//! launcher inputs legitimately omit `rss`, `name`, `address` and friends, so
//! deserialising into the emitter structs would reject files the launchers
//! accept. The rules mirror `hambugui-distribution` (strict, non-legacy mode)
//! plus what Helios additionally tolerates.

use std::collections::HashSet;

use serde_json::Value;

use crate::error::Result;
use crate::maven::MavenId;
use crate::paths;
use crate::spec::{Distribution, ModuleType};

/// Helios has no NeoForge support at all and Hambugui needs a parser patch, so
/// a distribution using the Nebula extension types is worth one warning.
const NEOFORGE_WARNING: &str = "NeoForge 모듈은 Hambugui 런처에 NeoForge 파서 패치(docs/launcher/hambugui-neoforge.patch)가 필요하고 Helios는 지원하지 않습니다";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    /// JSON pointer in dotted form, e.g. `servers[0].modules[3].artifact.path`.
    pub location: String,
    pub message: String,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}: {}", self.severity, self.location, self.message)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub findings: Vec<Finding>,
    /// Artifacts seen, at every nesting level.
    pub artifacts: usize,
    /// How many of them carry a `SHA256`, which the Hambugui runtime requires.
    pub sha256: usize,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.count(Severity::Error)
    }

    pub fn warnings(&self) -> usize {
        self.count(Severity::Warning)
    }

    /// No errors. Warnings are allowed.
    pub fn is_ok(&self) -> bool {
        self.errors() == 0
    }

    fn count(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    }
}

pub fn validate(distribution: &Distribution, strict: bool) -> Result<Report> {
    Ok(validate_value(&serde_json::to_value(distribution)?, strict))
}

pub fn validate_json(bytes: &[u8], strict: bool) -> Result<Report> {
    Ok(validate_value(&serde_json::from_slice(bytes)?, strict))
}

pub fn validate_value(value: &Value, strict: bool) -> Report {
    let mut cx = Validator {
        strict,
        report: Report::default(),
    };
    cx.distribution(value);
    cx.report
}

struct Validator {
    strict: bool,
    report: Report,
}

impl Validator {
    fn add(&mut self, severity: Severity, location: impl Into<String>, message: impl Into<String>) {
        self.report.findings.push(Finding {
            severity,
            location: location.into(),
            message: message.into(),
        });
    }

    fn error(&mut self, location: impl Into<String>, message: impl Into<String>) {
        self.add(Severity::Error, location, message);
    }

    fn warn(&mut self, location: impl Into<String>, message: impl Into<String>) {
        self.add(Severity::Warning, location, message);
    }

    /// Error when `--strict`, warning otherwise.
    fn strict_error(&mut self, location: impl Into<String>, message: impl Into<String>) {
        let severity = match self.strict {
            true => Severity::Error,
            false => Severity::Warning,
        };
        self.add(severity, location, message);
    }

    fn distribution(&mut self, value: &Value) {
        if value.as_object().is_none() {
            self.error("$", "the distribution root must be a JSON object");
            return;
        }
        // Both launchers compare it against the one they have cached, so an
        // absent or empty version makes every distribution look unchanged.
        match value.get("version").and_then(Value::as_str) {
            Some(version) if !version.is_empty() => {}
            _ => self.error(
                "version",
                "the distribution version must be a non-empty string",
            ),
        }
        // Nebula's rollback serial. Optional, because Helios-style files have
        // none and Helios ignores the key; when present, the Hambugui launcher
        // compares it as an unsigned 64-bit integer, so nothing else will do.
        if let Some(serial) = value.get("serial")
            && serial.as_u64().is_none()
        {
            self.error(
                "serial",
                "serial must be an unsigned integer (0 to 18446744073709551615)",
            );
        }
        let Some(servers) = value.get("servers").and_then(Value::as_array) else {
            self.error("servers", "servers must be an array");
            return;
        };

        let mut ids: HashSet<&str> = HashSet::new();
        for (index, server) in servers.iter().enumerate() {
            let location = format!("servers[{index}]");
            if server.as_object().is_none() {
                self.error(location, "a server must be a JSON object");
                continue;
            }
            match server.get("id").and_then(Value::as_str) {
                Some(id) => {
                    if !ids.insert(id) {
                        self.error(
                            format!("{location}.id"),
                            format!("duplicate server id {id}"),
                        );
                    }
                    // The launcher joins it onto its instance directory.
                    if !paths::is_single_segment(id) {
                        self.error(
                            format!("{location}.id"),
                            "a server id must be a single path segment (no / \\ : or ..)",
                        );
                    }
                }
                None => self.error(format!("{location}.id"), "a server id must be a string"),
            }

            let mut neoforge = false;
            match server.get("modules").and_then(Value::as_array) {
                Some(modules) => {
                    self.modules(modules, &format!("{location}.modules"), &mut neoforge)
                }
                None => self.error(
                    format!("{location}.modules"),
                    "server modules must be an array",
                ),
            }
            if neoforge {
                self.warn(location, NEOFORGE_WARNING);
            }
        }
    }

    fn modules(&mut self, modules: &[Value], prefix: &str, neoforge: &mut bool) {
        for (index, module) in modules.iter().enumerate() {
            let location = format!("{prefix}[{index}]");
            if module.as_object().is_none() {
                self.error(location, "a module must be a JSON object");
                continue;
            }

            let module_type = match module.get("type").and_then(Value::as_str) {
                Some(raw) => {
                    let known = module_type_of(raw);
                    if known.is_none() {
                        self.error(
                            format!("{location}.type"),
                            format!("unknown module type {raw}"),
                        );
                    }
                    if matches!(known, Some(ModuleType::NeoForge | ModuleType::NeoForgeMod)) {
                        *neoforge = true;
                    }
                    known
                }
                None => {
                    self.error(format!("{location}.type"), "a module type must be a string");
                    None
                }
            };

            let id = module.get("id").and_then(Value::as_str);
            match id {
                Some("") => self.error(format!("{location}.id"), "a module id must not be empty"),
                // Nebula shipped these until the FG2 resolver started unpacking:
                // the Hambugui runtime verifies the bytes it downloads and never
                // unpacks anything, so the file it stores is unusable.
                Some(id) if id.ends_with("@jar.pack.xz") => self.error(
                    format!("{location}.id"),
                    "pack.xz artifacts cannot be verified by the Hambugui runtime; regenerate \
                     with this Nebula version",
                ),
                Some(_) => {}
                None => self.error(format!("{location}.id"), "a module id must be a string"),
            }

            self.required(module.get("required"), &location);

            match module.get("artifact") {
                Some(artifact) if artifact.is_object() => {
                    self.artifact(artifact, &location, module_type, id)
                }
                Some(_) => self.error(
                    format!("{location}.artifact"),
                    "a module artifact must be a JSON object",
                ),
                None => self.error(format!("{location}.artifact"), "the module has no artifact"),
            }

            // Helios writes `subModules`; the Hambugui parser also accepts the
            // all-lowercase spelling seen in hand-written distributions.
            for key in ["subModules", "submodules"] {
                if let Some(children) = module.get(key) {
                    match children.as_array() {
                        Some(children) => {
                            self.modules(children, &format!("{location}.{key}"), neoforge)
                        }
                        None => self.error(format!("{location}.{key}"), "must be an array"),
                    }
                }
            }
        }
    }

    fn required(&mut self, required: Option<&Value>, location: &str) {
        let Some(required) = required else { return };
        if required.is_boolean() {
            return;
        }
        let Some(object) = required.as_object() else {
            self.error(
                format!("{location}.required"),
                "required must be a boolean or an object",
            );
            return;
        };
        // `parse_required` in the launcher takes them as booleans or not at all.
        for key in ["value", "def"] {
            if object.get(key).is_some_and(|field| !field.is_boolean()) {
                self.error(
                    format!("{location}.required.{key}"),
                    format!("required.{key} must be a boolean"),
                );
            }
        }
        if object.get("value").and_then(Value::as_bool) == Some(true)
            && object.get("def").and_then(Value::as_bool) == Some(false)
        {
            self.warn(
                format!("{location}.required"),
                "required is on but defaults to off; the launcher will not enable it",
            );
        }
    }

    fn artifact(
        &mut self,
        artifact: &Value,
        location: &str,
        module_type: Option<ModuleType>,
        id: Option<&str>,
    ) {
        self.report.artifacts += 1;
        let at = format!("{location}.artifact");

        match artifact.get("size").and_then(Value::as_u64) {
            Some(0) => self.warn(format!("{at}.size"), "artifact size is 0"),
            Some(_) => {}
            None => self.error(
                format!("{at}.size"),
                "artifact size must be an unsigned integer",
            ),
        }

        match artifact.get("url").and_then(Value::as_str) {
            Some(url) => {
                let lower = url.to_ascii_lowercase();
                if lower.starts_with("http://") {
                    self.strict_error(
                        format!("{at}.url"),
                        "artifact url is plain http; the launcher runtime requires https",
                    );
                } else if !lower.starts_with("https://") {
                    self.error(
                        format!("{at}.url"),
                        "artifact url must be an http or https URL",
                    );
                }
            }
            None => self.error(format!("{at}.url"), "artifact url must be a string"),
        }

        let (md5, _) = self.digest(artifact, &at, "MD5", 32);
        let (sha256, sha256_usable) = self.digest(artifact, &at, "SHA256", 64);
        if sha256_usable {
            self.report.sha256 += 1;
        }
        match (md5, sha256) {
            // `hambugui-distribution::parse_digest` rejects this outright and
            // Nebula output is never in the launcher's legacy mode.
            (false, false) => self.error(
                at.clone(),
                "artifact has neither MD5 nor SHA256; no launcher can verify it",
            ),
            (true, false) => self.strict_error(
                at.clone(),
                "artifact has no SHA256; the Hambugui runtime requires one",
            ),
            (false, true) => self.warn(
                at.clone(),
                "artifact has no MD5; Helios cannot verify this artifact",
            ),
            (true, true) => {}
        }

        match artifact.get("path") {
            Some(Value::String(path)) => {
                if let Err(error) = paths::clean_relative(path) {
                    self.error(format!("{at}.path"), error.to_string());
                }
            }
            Some(_) => self.error(format!("{at}.path"), "artifact path must be a string"),
            // Without a path the launcher derives the location from the id.
            None => self.pathless_id(location, module_type, id),
        }
    }

    fn pathless_id(&mut self, location: &str, module_type: Option<ModuleType>, id: Option<&str>) {
        let (Some(module_type), Some(id)) = (module_type, id) else {
            return;
        };
        match module_type {
            ModuleType::File => self.error(
                format!("{location}.artifact.path"),
                "a File module must declare artifact.path",
            ),
            // The launcher joins the id onto `repo/versions` as a directory and
            // a file name, so `:` is out too: it is not a legal one on Windows.
            ModuleType::VersionManifest => {
                if !paths::is_single_segment(id) {
                    self.error(
                        format!("{location}.id"),
                        "a pathless VersionManifest id must be a single safe path segment",
                    );
                }
            }
            _ => {
                if let Err(error) = MavenId::parse(id, "jar") {
                    self.error(format!("{location}.id"), error.to_string());
                }
            }
        }
    }

    /// `(declared, well formed)`. A malformed digest is still a declared one,
    /// so the presence rules above stay stable, but only a well formed one is
    /// counted in [`Report::sha256`]: that number says how many artifacts the
    /// Hambugui runtime can actually verify.
    fn digest(&mut self, artifact: &Value, at: &str, key: &str, length: usize) -> (bool, bool) {
        let Some(value) = artifact.get(key) else {
            return (false, false);
        };
        match value.as_str() {
            Some(hex) if hex.len() == length && hex.chars().all(|c| c.is_ascii_hexdigit()) => {
                return (true, true);
            }
            Some(_) => self.error(
                format!("{at}.{key}"),
                format!("{key} must be {length} hexadecimal characters"),
            ),
            None => self.error(format!("{at}.{key}"), format!("{key} must be a string")),
        }
        (true, false)
    }
}

/// Accepts every launcher type name, plus the lowercase `file` spelling the
/// Hambugui parser tolerates.
fn module_type_of(raw: &str) -> Option<ModuleType> {
    const TYPES: [ModuleType; 12] = [
        ModuleType::Forge,
        ModuleType::ForgeHosted,
        ModuleType::Fabric,
        ModuleType::LiteLoader,
        ModuleType::Library,
        ModuleType::ForgeMod,
        ModuleType::LiteMod,
        ModuleType::FabricMod,
        ModuleType::File,
        ModuleType::VersionManifest,
        ModuleType::NeoForge,
        ModuleType::NeoForgeMod,
    ];
    if raw == "file" {
        return Some(ModuleType::File);
    }
    TYPES.into_iter().find(|known| known.as_str() == raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every fixture below carries a root version, so only the rule under test
    /// produces findings.
    fn report(json: &str, strict: bool) -> Report {
        let json = json.replacen('{', r#"{"version":"1.0.0","#, 1);
        validate_json(json.as_bytes(), strict).unwrap()
    }

    #[test]
    fn rejects_shapes_that_are_not_a_distribution() {
        assert_eq!(report("[]", false).errors(), 1);
        assert_eq!(report(r#"{"servers":{}}"#, false).errors(), 1);
        assert_eq!(report(r#"{"servers":[{}]}"#, false).errors(), 2);
    }

    #[test]
    fn requires_a_distribution_version() {
        for json in [
            r#"{"servers":[]}"#,
            r#"{"version":"","servers":[]}"#,
            r#"{"version":1,"servers":[]}"#,
        ] {
            let report = validate_json(json.as_bytes(), false).unwrap();
            assert_eq!(report.errors(), 1, "{json}");
            assert_eq!(report.findings[0].location, "version");
        }
        assert!(
            validate_json(br#"{"version":"1.0.0","servers":[]}"#, false)
                .unwrap()
                .is_ok()
        );
    }

    #[test]
    fn rejects_duplicate_server_ids() {
        let json = r#"{"servers":[{"id":"a","modules":[]},{"id":"a","modules":[]}]}"#;
        let report = report(json, false);
        assert_eq!(report.errors(), 1);
        assert_eq!(report.findings[0].location, "servers[1].id");
    }

    #[test]
    fn rejects_server_ids_that_are_not_a_single_path_segment() {
        for id in ["", "../evil", "a/b", "C:/tmp/x"] {
            let json = format!(r#"{{"servers":[{{"id":"{id}","modules":[]}}]}}"#);
            let report = report(&json, false);
            assert_eq!(report.errors(), 1, "{id}");
            assert_eq!(report.findings[0].location, "servers[0].id");
        }
    }

    #[test]
    fn rejects_pack_xz_module_ids() {
        let json = r#"{"servers":[{"id":"a","modules":[{"id":"g:a:1@jar.pack.xz","type":"Library",
            "artifact":{"size":1,"url":"https://h/x","MD5":"00112233445566778899aabbccddeeff",
            "SHA256":"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"}}]}]}"#;
        let report = report(json, false);
        assert_eq!(report.errors(), 1);
        assert_eq!(report.findings[0].location, "servers[0].modules[0].id");
        assert!(report.findings[0].message.contains("pack.xz"));
    }

    #[test]
    fn rejects_a_required_object_that_is_not_booleans() {
        let json = r#"{"servers":[{"id":"a","modules":[{"id":"x.cfg","type":"File",
            "required":{"value":"yes","def":1},
            "artifact":{"size":1,"url":"https://h/x","MD5":"00112233445566778899aabbccddeeff",
            "SHA256":"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
            "path":"x"}}]}]}"#;
        let report = report(json, false);
        assert_eq!(report.errors(), 2);
        assert_eq!(
            report.findings[0].location,
            "servers[0].modules[0].required.value"
        );
        assert_eq!(
            report.findings[1].location,
            "servers[0].modules[0].required.def"
        );
    }

    #[test]
    fn counts_only_a_usable_sha256() {
        let json = r#"{"servers":[{"id":"a","modules":[{"id":"x.cfg","type":"File",
            "artifact":{"size":1,"url":"https://h/x","SHA256":"zz","path":"x"}}]}]}"#;
        let report = report(json, false);
        // One error for the bad hex, one warning for the missing MD5 -- but no
        // "neither digest" error, because one was declared.
        assert_eq!(report.errors(), 1);
        assert_eq!(report.warnings(), 1);
        // Nothing can verify this artifact, so it does not count as covered.
        assert_eq!(report.sha256, 0);
        assert_eq!(report.artifacts, 1);
    }

    #[test]
    fn escalates_with_strict() {
        let json = r#"{"servers":[{"id":"a","modules":[{"id":"x.cfg","type":"File",
            "artifact":{"size":1,"url":"http://h/x","MD5":"00112233445566778899aabbccddeeff","path":"x"}}]}]}"#;
        assert_eq!(report(json, false).errors(), 0);
        assert_eq!(report(json, false).warnings(), 2);
        assert_eq!(report(json, true).errors(), 2);
    }
}
