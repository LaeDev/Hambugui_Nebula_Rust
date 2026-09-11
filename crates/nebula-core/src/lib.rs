//! Nebula core: distribution.json generation for Helios/Hambugui launchers.

pub mod catalog;
pub mod config;
pub mod curseforge;
pub mod distribution;
pub mod error;
pub mod hash;
pub mod import;
pub mod java;
pub mod layout;
pub mod maven;
pub mod meta;
pub mod paths;
pub mod publish;
pub mod remote;
pub mod resolver;
pub mod scan;
pub mod server;
pub mod spec;
pub mod validate;
pub mod version;

pub use config::Config;
pub use distribution::{GenerateOptions, GenerateResult, generate_distribution, init_root};
pub use error::{Error, Result};
pub use import::{ImportFormat, ImportReport, ManualDownload, detect_format, import_modpack};
pub use java::{ClaritasMeta, JarKind, Java};
pub use layout::{Root, ServerDir};
pub use maven::MavenId;
pub use meta::{DistroMeta, ServerMeta, ServerMetaOptions, UntrackedFilesOption};
pub use server::{
    CreateServerOptions, CreatedServer, LoaderKind, ServerSummary, build_server_spec,
    create_server, list_servers,
};
pub use spec::{Artifact, Distribution, Module, ModuleType, Required, Server};
pub use validate::{Finding, Report, Severity, validate, validate_json, validate_value};
pub use version::{MinecraftVersion, Promotion};
