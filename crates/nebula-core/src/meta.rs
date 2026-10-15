//! Nebula's own metadata files (`meta/distrometa.json`, `servermeta.json`) and
//! the JSON schemas editors use to annotate them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{Result, io};
use crate::layout::Root;
use crate::spec::{DistroDiscord, ServerDiscord};
use crate::version::MinecraftVersion;

pub const DISTRO_META_SCHEMA: &str = "DistroMetaSchema";
pub const SERVER_META_SCHEMA: &str = "ServerMetaSchema";

const PLACEHOLDER: &str = "<FILL IN OR REMOVE DISCORD OBJECT>";

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct DistroMeta {
    /// URL to the JSON schema for this type of file.
    /// This is used by editors to validate and annotate the data.
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Distribution metadata to be forwarded to the distribution file.
    pub meta: DistroMetaInner,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct DistroMetaInner {
    pub rss: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discord: Option<DistroDiscord>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct ServerMeta {
    /// URL to the JSON schema for this type of file.
    /// This is used by editors to validate and annotate the data.
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Server metadata to be forwarded to the distribution file.
    pub meta: ServerMetaInner,
    /// Properties related to Forge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forge: Option<LoaderMeta>,
    /// Properties related to Fabric.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fabric: Option<LoaderMeta>,
    /// Properties related to NeoForge. The version does NOT include the
    /// minecraft version. Ex. 21.1.77 for Minecraft 1.21.1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub neoforge: Option<LoaderMeta>,
    /// A list of option objects defining patterns for untracked files.
    #[serde(rename = "untrackedFiles", skip_serializing_if = "Option::is_none")]
    pub untracked_files: Option<Vec<UntrackedFilesOption>>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServerMetaInner {
    pub version: String,
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discord: Option<ServerDiscord>,
    pub main_server: bool,
    pub autoconnect: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java_options: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct LoaderMeta {
    /// The loader version. This does NOT include the minecraft version.
    /// Ex. 14.23.5.2854 for Forge, 0.14.18 for Fabric.
    pub version: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UntrackedFilesOption {
    /// The subdirectories these patterns will be applied to. Ex.
    /// [ "files", "forgemods" ]
    pub applies_to: Vec<String>,
    /// Glob patterns to match against the file.
    pub patterns: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ServerMetaOptions {
    pub version: Option<String>,
    pub forge_version: Option<String>,
    pub fabric_version: Option<String>,
    pub neoforge_version: Option<String>,
}

pub fn default_distro_meta() -> DistroMeta {
    DistroMeta {
        schema: None,
        meta: DistroMetaInner {
            rss: "<LINK TO RSS FEED>".into(),
            discord: Some(DistroDiscord {
                client_id: PLACEHOLDER.into(),
                small_image_text: PLACEHOLDER.into(),
                small_image_key: PLACEHOLDER.into(),
            }),
        },
    }
}

pub fn default_server_meta(
    id: &str,
    mc_version: &MinecraftVersion,
    options: &ServerMetaOptions,
) -> ServerMeta {
    let mut description = format!("{id} Running Minecraft {mc_version}");
    if let Some(forge) = &options.forge_version {
        description.push_str(&format!(" (Forge v{forge})"));
    }
    if let Some(fabric) = &options.fabric_version {
        description.push_str(&format!(" (Fabric v{fabric})"));
    }
    if let Some(neoforge) = &options.neoforge_version {
        description.push_str(&format!(" (NeoForge v{neoforge})"));
    }

    ServerMeta {
        schema: None,
        meta: ServerMetaInner {
            version: options.version.clone().unwrap_or_else(|| "1.0.0".into()),
            name: format!("{id} (Minecraft {mc_version})"),
            description,
            icon: Some(
                "How to set the server icon: https://github.com/dscalzi/Nebula#setting-the-server-icon"
                    .into(),
            ),
            address: "localhost:25565".into(),
            discord: Some(ServerDiscord {
                short_id: PLACEHOLDER.into(),
                large_image_text: PLACEHOLDER.into(),
                large_image_key: PLACEHOLDER.into(),
            }),
            main_server: false,
            autoconnect: false,
            java_options: None,
        },
        forge: options.forge_version.clone().map(|version| LoaderMeta { version }),
        fabric: options.fabric_version.clone().map(|version| LoaderMeta { version }),
        neoforge: options.neoforge_version.clone().map(|version| LoaderMeta { version }),
        untracked_files: Some(Vec::new()),
    }
}

pub fn schema_url(root: &Root, type_name: &str) -> String {
    let path = root.schemas_dir().join(format!("{type_name}.schema.json"));
    Url::from_file_path(&path).map_or_else(|_| format!("file://{}", path.display()), String::from)
}

pub fn generate_schemas(root: &Root) -> Result<()> {
    let dir = root.schemas_dir();
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(io(format!("removing {}", dir.display())))?;
    }
    std::fs::create_dir_all(&dir).map_err(io(format!("creating {}", dir.display())))?;

    for (type_name, schema) in [
        (DISTRO_META_SCHEMA, schemars::schema_for!(DistroMeta)),
        (SERVER_META_SCHEMA, schemars::schema_for!(ServerMeta)),
    ] {
        let path = dir.join(format!("{type_name}.schema.json"));
        std::fs::write(&path, serde_json::to_string(&schema)?)
            .map_err(io(format!("writing {}", path.display())))?;
        log::info!("Schema for {type_name} saved to {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mc() -> MinecraftVersion {
        "1.20.1".parse().unwrap()
    }

    #[test]
    fn reproduces_the_upstream_default_server_meta() {
        let meta = default_server_meta(
            "Test",
            &mc(),
            &ServerMetaOptions {
                forge_version: Some("47.2.0".into()),
                ..Default::default()
            },
        );
        assert_eq!(meta.meta.version, "1.0.0");
        assert_eq!(meta.meta.name, "Test (Minecraft 1.20.1)");
        assert_eq!(
            meta.meta.description,
            "Test Running Minecraft 1.20.1 (Forge v47.2.0)"
        );
        assert_eq!(meta.meta.address, "localhost:25565");
        assert_eq!(meta.forge.as_ref().unwrap().version, "47.2.0");
        assert!(meta.fabric.is_none());
        assert_eq!(meta.untracked_files, Some(Vec::new()));
        assert_eq!(
            meta.meta.discord.as_ref().unwrap().short_id,
            "<FILL IN OR REMOVE DISCORD OBJECT>"
        );
    }

    #[test]
    fn appends_the_fabric_description_suffix() {
        let meta = default_server_meta(
            "Fab",
            &mc(),
            &ServerMetaOptions {
                fabric_version: Some("0.14.18".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            meta.meta.description,
            "Fab Running Minecraft 1.20.1 (Fabric v0.14.18)"
        );
        assert_eq!(meta.fabric.as_ref().unwrap().version, "0.14.18");
    }

    #[test]
    fn appends_the_neoforge_description_suffix() {
        let meta = default_server_meta(
            "Neo",
            &"1.21.1".parse().unwrap(),
            &ServerMetaOptions {
                neoforge_version: Some("21.1.77".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            meta.meta.description,
            "Neo Running Minecraft 1.21.1 (NeoForge v21.1.77)"
        );
        assert_eq!(meta.neoforge.as_ref().unwrap().version, "21.1.77");
        assert!(meta.forge.is_none());
        assert!(meta.fabric.is_none());
        assert_eq!(
            serde_json::to_value(&meta).unwrap()["neoforge"]["version"],
            "21.1.77"
        );
    }

    #[test]
    fn serialises_the_schema_key_first() {
        let mut meta = default_distro_meta();
        meta.schema = Some("file:///root/schemas/DistroMetaSchema.schema.json".into());
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.starts_with(r#"{"$schema":"#), "{json}");

        let mut server = default_server_meta("Test", &mc(), &ServerMetaOptions::default());
        server.schema = Some("file:///root/schemas/ServerMetaSchema.schema.json".into());
        let json = serde_json::to_string(&server).unwrap();
        assert!(json.starts_with(r#"{"$schema":"#), "{json}");
    }

    #[test]
    fn writes_both_schemas() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path(), Url::parse("https://host/").unwrap());
        generate_schemas(&root).unwrap();

        let distro =
            std::fs::read_to_string(root.schemas_dir().join("DistroMetaSchema.schema.json"))
                .unwrap();
        assert!(distro.contains("$schema"));
        assert!(distro.contains("rss"));

        let server =
            std::fs::read_to_string(root.schemas_dir().join("ServerMetaSchema.schema.json"))
                .unwrap();
        assert!(server.contains("untrackedFiles"));
        assert!(server.contains("forge"));
        assert!(server.contains("neoforge"));
        assert!(server.contains("mainServer"));

        assert_eq!(
            schema_url(&root, SERVER_META_SCHEMA),
            Url::from_file_path(root.schemas_dir().join("ServerMetaSchema.schema.json"))
                .unwrap()
                .to_string()
        );
    }
}
