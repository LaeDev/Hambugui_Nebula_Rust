use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MavenId {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub classifier: Option<String>,
    pub extension: String,
}

impl MavenId {
    pub fn parse(id: &str, default_extension: &str) -> Result<Self> {
        let (coordinates, extension) = match id.split_once('@') {
            Some((coordinates, extension)) => (coordinates, extension),
            None => (id, default_extension),
        };

        let parts: Vec<&str> = coordinates.split(':').collect();
        let (group, artifact, version, classifier) = match parts[..] {
            [group, artifact, version] => (group, artifact, version, None),
            [group, artifact, version, classifier] => (group, artifact, version, Some(classifier)),
            _ => return Err(Error::Invalid(format!("{id} is not a maven identifier"))),
        };

        if [group, artifact, version, extension]
            .iter()
            .any(|p| p.is_empty())
        {
            return Err(Error::Invalid(format!("{id} is not a maven identifier")));
        }

        Ok(Self {
            group: group.to_owned(),
            artifact: artifact.to_owned(),
            version: version.to_owned(),
            classifier: classifier.map(str::to_owned),
            extension: extension.to_owned(),
        })
    }

    pub fn id(&self) -> String {
        format!("{}@{}", self.id_without_extension(), self.extension)
    }

    pub fn id_without_extension(&self) -> String {
        let mut id = format!("{}:{}:{}", self.group, self.artifact, self.version);
        if let Some(classifier) = &self.classifier {
            id.push(':');
            id.push_str(classifier);
        }
        id
    }

    pub fn path(&self) -> String {
        let classifier = match &self.classifier {
            Some(classifier) => format!("-{classifier}"),
            None => String::new(),
        };
        format!(
            "{}/{}/{}/{}-{}{}.{}",
            self.group.replace('.', "/"),
            self.artifact,
            self.version,
            self.artifact,
            self.version,
            classifier,
            self.extension
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_three_part_identifiers() {
        let id = MavenId::parse("net.minecraftforge:forge:1.20.1-47.2.0", "jar").unwrap();
        assert_eq!(id.classifier, None);
        assert_eq!(id.extension, "jar");
        assert_eq!(id.id(), "net.minecraftforge:forge:1.20.1-47.2.0@jar");
        assert_eq!(
            id.path(),
            "net/minecraftforge/forge/1.20.1-47.2.0/forge-1.20.1-47.2.0.jar"
        );
    }

    #[test]
    fn parses_four_part_identifiers() {
        let id = MavenId::parse("net.minecraft:client:1.20.1:srg", "jar").unwrap();
        assert_eq!(id.classifier.as_deref(), Some("srg"));
        assert_eq!(id.id_without_extension(), "net.minecraft:client:1.20.1:srg");
        assert_eq!(
            id.path(),
            "net/minecraft/client/1.20.1/client-1.20.1-srg.jar"
        );
    }

    #[test]
    fn keeps_multi_dot_extensions() {
        let id = MavenId::parse("a.b:c:1.0@jar.pack.xz", "jar").unwrap();
        assert_eq!(id.extension, "jar.pack.xz");
        assert_eq!(id.path(), "a/b/c/1.0/c-1.0.jar.pack.xz");
    }

    #[test]
    fn rejects_malformed_identifiers() {
        assert!(MavenId::parse("group:artifact", "jar").is_err());
        assert!(MavenId::parse("a:b:c:d:e", "jar").is_err());
        assert!(MavenId::parse("a::1.0", "jar").is_err());
    }
}
