//! Emitter model for `distribution.json`. Field order is the serialised key
//! order and must match `fixtures/launcher/nebula-hardening-generated.json`.
//! The validator works on `serde_json::Value` and never deserialises launcher
//! fixtures into these structs.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleType {
    Forge,
    ForgeHosted,
    Fabric,
    LiteLoader,
    Library,
    ForgeMod,
    LiteMod,
    FabricMod,
    File,
    VersionManifest,
    /// Nebula extension: no Helios/`helios-distribution-types` equivalent.
    /// Hambugui needs `docs/launcher/hambugui-neoforge.patch`.
    NeoForge,
    /// Nebula extension, see [`ModuleType::NeoForge`].
    NeoForgeMod,
}

impl ModuleType {
    pub fn as_str(self) -> &'static str {
        match self {
            ModuleType::Forge => "Forge",
            ModuleType::ForgeHosted => "ForgeHosted",
            ModuleType::Fabric => "Fabric",
            ModuleType::LiteLoader => "LiteLoader",
            ModuleType::Library => "Library",
            ModuleType::ForgeMod => "ForgeMod",
            ModuleType::LiteMod => "LiteMod",
            ModuleType::FabricMod => "FabricMod",
            ModuleType::File => "File",
            ModuleType::VersionManifest => "VersionManifest",
            ModuleType::NeoForge => "NeoForge",
            ModuleType::NeoForgeMod => "NeoForgeMod",
        }
    }

    pub fn default_extension(self) -> Option<&'static str> {
        match self {
            ModuleType::LiteMod => Some("litemod"),
            ModuleType::VersionManifest => Some("json"),
            ModuleType::File => None,
            _ => Some("jar"),
        }
    }

    pub fn default_group(self) -> String {
        format!("generated.{}", self.as_str().to_lowercase())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Artifact {
    pub size: u64,
    pub url: String,
    #[serde(rename = "MD5", skip_serializing_if = "Option::is_none")]
    pub md5: Option<String>,
    #[serde(rename = "SHA256", skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Required {
    pub value: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub def: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Module {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub module_type: ModuleType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classpath: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Required>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub untracked: Option<bool>,
    pub artifact: Artifact,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_modules: Option<Vec<Module>>,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerDiscord {
    pub short_id: String,
    pub large_image_text: String,
    pub large_image_key: String,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DistroDiscord {
    pub client_id: String,
    pub small_image_text: String,
    pub small_image_key: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Always serialised; upstream emits `null` when no icon was found.
    pub icon: Option<String>,
    pub version: String,
    pub address: String,
    pub minecraft_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discord: Option<ServerDiscord>,
    pub main_server: bool,
    pub autoconnect: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java_options: Option<serde_json::Value>,
    pub modules: Vec<Module>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Distribution {
    pub version: String,
    /// Strictly increases with every distribution Nebula writes, so a launcher
    /// can refuse an older signed copy (rollback). Covered by the signature
    /// like every other byte. A Hambugui extension: Helios ignores unknown
    /// keys, and a hand-written or upstream file has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<u64>,
    pub rss: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discord: Option<DistroDiscord>,
    pub servers: Vec<Server>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact() -> Artifact {
        Artifact {
            size: 15,
            url: "https://cdn.example.invalid/servers/Hambugui-1.20.1/files/config/client.cfg"
                .into(),
            md5: Some("7576e668d81fb915409b43910f0779d1".into()),
            sha256: Some("f4a3565faf92888ae918bcaa41771ed55f0b9949a1e5193ed0c68a907706a71e".into()),
            path: Some("config/client.cfg".into()),
        }
    }

    #[test]
    fn serialises_launcher_key_names_in_order() {
        let module = Module {
            id: "generated.forgemod:jei:1.0@jar".into(),
            name: "Jei".into(),
            module_type: ModuleType::ForgeMod,
            classpath: Some(false),
            required: Some(Required {
                value: false,
                def: Some(false),
            }),
            untracked: Some(true),
            artifact: artifact(),
            sub_modules: Some(vec![]),
        };

        let json = serde_json::to_string(&module).unwrap();
        assert_eq!(
            json,
            r#"{"id":"generated.forgemod:jei:1.0@jar","name":"Jei","type":"ForgeMod","classpath":false,"required":{"value":false,"def":false},"untracked":true,"artifact":{"size":15,"url":"https://cdn.example.invalid/servers/Hambugui-1.20.1/files/config/client.cfg","MD5":"7576e668d81fb915409b43910f0779d1","SHA256":"f4a3565faf92888ae918bcaa41771ed55f0b9949a1e5193ed0c68a907706a71e","path":"config/client.cfg"},"subModules":[]}"#
        );
    }

    #[test]
    fn serialises_every_type_as_its_own_name() {
        for module_type in [
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
        ] {
            assert_eq!(
                serde_json::to_value(module_type).unwrap(),
                module_type.as_str()
            );
        }
    }

    #[test]
    fn reproduces_the_golden_fixture_byte_for_byte() {
        const PLACEHOLDER: &str = "<FILL IN OR REMOVE DISCORD OBJECT>";
        let distribution = Distribution {
            version: "1.0.0".into(),
            serial: None,
            rss: "<LINK TO RSS FEED>".into(),
            discord: Some(DistroDiscord {
                client_id: PLACEHOLDER.into(),
                small_image_text: PLACEHOLDER.into(),
                small_image_key: PLACEHOLDER.into(),
            }),
            servers: vec![Server {
                id: "Hambugui-1.20.1".into(),
                name: "Hambugui (Minecraft 1.20.1)".into(),
                description: "Hambugui Running Minecraft 1.20.1".into(),
                icon: None,
                version: "1.0.0".into(),
                address: "localhost:25565".into(),
                minecraft_version: "1.20.1".into(),
                discord: Some(ServerDiscord {
                    short_id: PLACEHOLDER.into(),
                    large_image_text: PLACEHOLDER.into(),
                    large_image_key: PLACEHOLDER.into(),
                }),
                main_server: false,
                autoconnect: false,
                java_options: None,
                modules: vec![Module {
                    id: "client.cfg".into(),
                    name: "client.cfg".into(),
                    module_type: ModuleType::File,
                    classpath: None,
                    required: None,
                    untracked: None,
                    artifact: artifact(),
                    sub_modules: None,
                }],
            }],
        };

        let golden = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/launcher/nebula-hardening-generated.json"
        ));
        assert_eq!(
            serde_json::to_string_pretty(&distribution).unwrap(),
            golden.replace("\r\n", "\n").trim_end()
        );
    }

    #[test]
    fn derives_type_metadata() {
        assert_eq!(ModuleType::ForgeMod.default_group(), "generated.forgemod");
        assert_eq!(ModuleType::Library.default_group(), "generated.library");
        assert_eq!(ModuleType::LiteMod.default_extension(), Some("litemod"));
        assert_eq!(
            ModuleType::VersionManifest.default_extension(),
            Some("json")
        );
        assert_eq!(ModuleType::File.default_extension(), None);
        assert_eq!(ModuleType::ForgeHosted.default_extension(), Some("jar"));
        assert_eq!(ModuleType::NeoForge.default_group(), "generated.neoforge");
        assert_eq!(
            ModuleType::NeoForgeMod.default_group(),
            "generated.neoforgemod"
        );
        assert_eq!(ModuleType::NeoForge.default_extension(), Some("jar"));
        assert_eq!(ModuleType::NeoForgeMod.default_extension(), Some("jar"));
    }
}
