use std::path::PathBuf;

use url::Url;

use crate::maven::MavenId;

/// Single owner of the on-disk directory layout and of the URLs that mirror it.
#[derive(Debug, Clone)]
pub struct Root {
    pub path: PathBuf,
    pub base_url: Url,
}

impl Root {
    pub fn new(path: impl Into<PathBuf>, base_url: Url) -> Self {
        Self {
            path: path.into(),
            base_url,
        }
    }

    pub fn meta_dir(&self) -> PathBuf {
        self.path.join("meta")
    }

    pub fn distro_meta_file(&self) -> PathBuf {
        self.meta_dir().join("distrometa.json")
    }

    pub fn schemas_dir(&self) -> PathBuf {
        self.path.join("schemas")
    }

    pub fn servers_dir(&self) -> PathBuf {
        self.path.join("servers")
    }

    pub fn repo_lib_dir(&self) -> PathBuf {
        self.path.join("repo").join("lib")
    }

    pub fn repo_versions_dir(&self) -> PathBuf {
        self.path.join("repo").join("versions")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.path.join("cache")
    }

    pub fn forge_cache_dir(&self, artifact_version: &str) -> PathBuf {
        self.cache_dir().join("forge").join(artifact_version)
    }

    pub fn neoforge_cache_dir(&self, version: &str) -> PathBuf {
        self.cache_dir().join("neoforge").join(version)
    }

    pub fn temp_dir(&self) -> PathBuf {
        self.path.join("temp")
    }

    pub fn work_java_dir(&self) -> PathBuf {
        self.path.join("work").join("java")
    }

    pub fn modpacks_dir(&self) -> PathBuf {
        self.path.join("modpacks")
    }

    pub fn modpacks_curseforge_dir(&self) -> PathBuf {
        self.modpacks_dir().join("curseforge")
    }

    pub fn modpacks_prism_dir(&self) -> PathBuf {
        self.modpacks_dir().join("prism")
    }

    /// Segments are pushed one at a time so the url crate percent-encodes them.
    /// `Url::join` would truncate a file name at `#` or `?`.
    pub fn url(&self, relative: &str) -> String {
        let mut url = self.base_url.clone();
        if let Ok(mut segments) = url.path_segments_mut() {
            segments
                .pop_if_empty()
                .extend(relative.split(['/', '\\']).filter(|s| !s.is_empty()));
        }
        url.to_string()
    }

    pub fn lib_path(&self, id: &MavenId) -> PathBuf {
        self.repo_lib_dir().join(id.path())
    }

    pub fn lib_url(&self, id: &MavenId) -> String {
        self.url(&format!("repo/lib/{}", id.path()))
    }

    pub fn version_manifest_path(&self, name: &str) -> PathBuf {
        self.repo_versions_dir()
            .join(name)
            .join(format!("{name}.json"))
    }

    pub fn version_manifest_url(&self, name: &str) -> String {
        self.url(&format!("repo/versions/{name}/{name}.json"))
    }
}

#[derive(Debug, Clone)]
pub struct ServerDir {
    pub root: Root,
    pub dir_name: String,
    pub path: PathBuf,
}

impl ServerDir {
    pub fn new(root: Root, dir_name: impl Into<String>) -> Self {
        let dir_name = dir_name.into();
        let path = root.servers_dir().join(&dir_name);
        Self {
            root,
            dir_name,
            path,
        }
    }

    pub fn server_meta_file(&self) -> PathBuf {
        self.path.join("servermeta.json")
    }

    pub fn files_dir(&self) -> PathBuf {
        self.path.join("files")
    }

    pub fn libraries_dir(&self) -> PathBuf {
        self.path.join("libraries")
    }

    pub fn forgemods_dir(&self) -> PathBuf {
        self.path.join("forgemods")
    }

    pub fn neoforgemods_dir(&self) -> PathBuf {
        self.path.join("neoforgemods")
    }

    pub fn fabricmods_dir(&self) -> PathBuf {
        self.path.join("fabricmods")
    }

    pub fn relative(&self, sub: &str) -> String {
        format!("servers/{}/{}", self.dir_name, sub)
    }

    pub fn url(&self, sub: &str) -> String {
        self.root.url(&self.relative(sub))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(base: &str) -> Root {
        Root::new("H:/nebula", Url::parse(base).unwrap())
    }

    #[test]
    fn appends_to_a_base_with_a_path() {
        assert_eq!(
            root("https://host/dist/").url("servers/x"),
            "https://host/dist/servers/x"
        );
        assert_eq!(
            root("https://host/").url("servers/x"),
            "https://host/servers/x"
        );
    }

    #[test]
    fn percent_encodes_every_segment() {
        let root = root("https://host/");
        assert_eq!(root.url("files/a b.cfg"), "https://host/files/a%20b.cfg");
        assert_eq!(root.url("files/a#b.cfg"), "https://host/files/a%23b.cfg");
        assert_eq!(root.url("files/a?b.cfg"), "https://host/files/a%3Fb.cfg");
        assert_eq!(root.url("files/a%b.cfg"), "https://host/files/a%25b.cfg");
        assert_eq!(
            root.url("files/한글.cfg"),
            "https://host/files/%ED%95%9C%EA%B8%80.cfg"
        );
    }

    #[test]
    fn never_leaks_windows_separators() {
        let url = root("https://host/dist/").url("files\\config\\a.cfg");
        assert_eq!(url, "https://host/dist/files/config/a.cfg");
        assert!(!url.contains('\\'));
    }

    #[test]
    fn builds_maven_and_manifest_locations() {
        let root = root("https://host/dist/");
        let id = MavenId::parse("net.minecraftforge:forge:1.20.1-47.2.0:universal", "jar").unwrap();
        assert_eq!(
            root.lib_url(&id),
            "https://host/dist/repo/lib/net/minecraftforge/forge/1.20.1-47.2.0/forge-1.20.1-47.2.0-universal.jar"
        );
        assert!(root.lib_path(&id).starts_with(root.repo_lib_dir()));
        assert_eq!(
            root.version_manifest_url("1.20.1-forge-47.2.0"),
            "https://host/dist/repo/versions/1.20.1-forge-47.2.0/1.20.1-forge-47.2.0.json"
        );
        assert!(
            root.version_manifest_path("1.20.1-forge-47.2.0")
                .ends_with("1.20.1-forge-47.2.0.json")
        );
    }

    #[test]
    fn resolves_server_paths_and_urls() {
        let server = ServerDir::new(root("https://host/dist/"), "Test-1.20.1");
        assert_eq!(
            server.relative("files/a.cfg"),
            "servers/Test-1.20.1/files/a.cfg"
        );
        assert_eq!(
            server.url("files/config/a.cfg"),
            "https://host/dist/servers/Test-1.20.1/files/config/a.cfg"
        );
        assert!(server.files_dir().ends_with("files"));
        assert!(server.server_meta_file().ends_with("servermeta.json"));
    }

    #[test]
    fn resolves_neoforge_locations() {
        let root = root("https://host/dist/");
        let cache = root.neoforge_cache_dir("21.1.77");
        assert!(cache.starts_with(root.cache_dir().join("neoforge")));
        assert!(cache.ends_with("21.1.77"));

        let server = ServerDir::new(root, "Test-1.21.1");
        assert!(server.neoforgemods_dir().starts_with(&server.path));
        assert!(server.neoforgemods_dir().ends_with("neoforgemods"));
    }
}
