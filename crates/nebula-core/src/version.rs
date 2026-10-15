use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy)]
pub struct MinecraftVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: Option<u32>,
}

impl MinecraftVersion {
    pub fn new(major: u32, minor: u32, patch: Option<u32>) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    fn key(&self) -> (u32, u32, u32) {
        (self.major, self.minor, self.patch.unwrap_or(0))
    }
}

impl PartialEq for MinecraftVersion {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl Eq for MinecraftVersion {}

impl PartialOrd for MinecraftVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MinecraftVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

impl fmt::Display for MinecraftVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)?;
        if let Some(patch) = self.patch {
            write!(f, ".{patch}")?;
        }
        Ok(())
    }
}

impl FromStr for MinecraftVersion {
    type Err = Error;

    /// Port of upstream `/(\d+).(\d+).?(\d+)?/`: unanchored, and the separators
    /// are unescaped dots, so any single character joins the components. The
    /// leading `\d+` backtracks, which is why `120` parses as `1.0`.
    fn from_str(s: &str) -> Result<Self> {
        let chars: Vec<char> = s.chars().collect();
        let digits_end = |from: usize| {
            let mut i = from;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            i
        };

        for start in 0..chars.len() {
            let run_end = digits_end(start);
            for major_end in (start + 1..=run_end).rev() {
                // The separator must exist and at least one digit must follow it.
                let minor_start = major_end + 1;
                if major_end >= chars.len() {
                    continue;
                }
                let minor_end = digits_end(minor_start);
                if minor_end == minor_start {
                    continue;
                }
                let patch_start = minor_end + 1;
                let patch_end = digits_end(patch_start.min(chars.len()));
                return Ok(Self {
                    major: number(&chars[start..major_end])?,
                    minor: number(&chars[minor_start..minor_end])?,
                    patch: if patch_end > patch_start {
                        Some(number(&chars[patch_start..patch_end])?)
                    } else {
                        None
                    },
                });
            }
        }

        Err(Error::Invalid(format!(
            "{s} is not a valid minecraft version!"
        )))
    }
}

fn number(chars: &[char]) -> Result<u32> {
    let text: String = chars.iter().collect();
    text.parse()
        .map_err(|_| Error::Invalid(format!("{text} is not a version number")))
}

/// Upstream `VersionUtil.versionGte` threw on a component-count mismatch and
/// returned true as soon as *any* component was greater (so `0.9.9 >= 1.0.0`
/// held); we pad with zeros and compare component by component.
pub fn version_gte(version: &str, min: &str) -> bool {
    let components =
        |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    let (left, right) = (components(version), components(min));
    for i in 0..left.len().max(right.len()) {
        let (l, r) = (
            left.get(i).copied().unwrap_or(0),
            right.get(i).copied().unwrap_or(0),
        );
        if l != r {
            return l > r;
        }
    }
    true
}

/// Forge library versions up to `14.23.5.2847` build with ForgeGradle 2.
pub fn is_one_dot_twelve_fg2(forge_version: &str) -> bool {
    const MAX_FG2: [u64; 4] = [14, 23, 5, 2847];
    !forge_version
        .split('.')
        .zip(MAX_FG2)
        .any(|(part, max)| part.parse::<u64>().is_ok_and(|v| v > max))
}

/// NeoForge (`net.neoforged:neoforge`) starts at Minecraft 1.20.2. Legacy
/// NeoForge for 1.20.1 (`net.neoforged:forge`) is out of scope; use Forge 47.x.
pub fn is_neoforge_supported(mc: &MinecraftVersion) -> bool {
    (mc.major == 1 && mc.minor == 20 && mc.patch.unwrap_or(0) >= 2)
        || (mc.major == 1 && mc.minor >= 21)
        || mc.major >= 26
}

/// The NeoForge version prefix for a Minecraft version: `1.A.B` -> `A.B.`
/// (a missing patch counts as 0, so `1.21` -> `21.0.`), and year-based `A.B`
/// -> `A.B.`.
pub fn neoforge_prefix(mc: &MinecraftVersion) -> String {
    if mc.major == 1 {
        format!("{}.{}.", mc.minor, mc.patch.unwrap_or(0))
    } else {
        format!("{}.{}.", mc.major, mc.minor)
    }
}

pub fn is_version_acceptable(version: &MinecraftVersion, minors: &[u32]) -> bool {
    version.major == 1 && minors.contains(&version.minor)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Promotion {
    Latest,
    Recommended,
}

impl Promotion {
    pub fn as_str(self) -> &'static str {
        match self {
            Promotion::Latest => "latest",
            Promotion::Recommended => "recommended",
        }
    }
}

impl FromStr for Promotion {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "latest" => Ok(Promotion::Latest),
            "recommended" => Ok(Promotion::Recommended),
            other => Err(Error::Invalid(format!("{other} is not a promotion type"))),
        }
    }
}

impl fmt::Display for Promotion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> MinecraftVersion {
        s.parse().unwrap()
    }

    #[test]
    fn parses_minecraft_versions() {
        assert_eq!(parse("1.20.1"), MinecraftVersion::new(1, 20, Some(1)));
        assert_eq!(parse("1.7.10"), MinecraftVersion::new(1, 7, Some(10)));
        assert_eq!(parse("1.20").patch, None);
        assert_eq!(parse("forge-1.12.2"), MinecraftVersion::new(1, 12, Some(2)));
        // Upstream backtracking quirk: the separator matches any character.
        assert_eq!(parse("120"), MinecraftVersion::new(1, 0, None));
        assert!("nope".parse::<MinecraftVersion>().is_err());
    }

    #[test]
    fn displays_without_a_missing_patch() {
        assert_eq!(parse("1.20").to_string(), "1.20");
        assert_eq!(parse("1.20.1").to_string(), "1.20.1");
    }

    #[test]
    fn orders_a_missing_patch_as_zero() {
        assert_eq!(parse("1.20"), parse("1.20.0"));
        assert!(parse("1.20.1") > parse("1.20"));
        assert!(parse("1.9") < parse("1.10"));
    }

    #[test]
    fn compares_dotted_versions() {
        assert!(version_gte("0.14.18", "0.12.3"));
        assert!(version_gte("0.12.3", "0.12.3"));
        assert!(!version_gte("0.11.9", "0.12.3"));
        assert!(version_gte("1.20", "1.19.4"));
        assert!(!version_gte("0.9.9", "1.0.0"));
        assert!(version_gte("14.23.5.2860", "14.23.5.2857"));
        assert!(!version_gte("14.23.5.2768", "14.23.5.2857"));
    }

    #[test]
    fn detects_forge_gradle_two() {
        assert!(is_one_dot_twelve_fg2("14.23.5.2847"));
        assert!(is_one_dot_twelve_fg2("14.23.5.2768"));
        assert!(!is_one_dot_twelve_fg2("14.23.5.2860"));
        assert!(!is_one_dot_twelve_fg2("47.2.0"));
    }

    #[test]
    fn accepts_only_major_one() {
        assert!(is_version_acceptable(
            &parse("1.12.2"),
            &[7, 8, 9, 10, 11, 12]
        ));
        assert!(!is_version_acceptable(
            &parse("1.13"),
            &[7, 8, 9, 10, 11, 12]
        ));
        assert!(!is_version_acceptable(
            &MinecraftVersion::new(2, 12, None),
            &[12]
        ));
    }

    #[test]
    fn supports_neoforge_from_one_twenty_two() {
        assert!(!is_neoforge_supported(&parse("1.20")));
        assert!(!is_neoforge_supported(&parse("1.20.1")));
        assert!(is_neoforge_supported(&parse("1.20.2")));
        assert!(is_neoforge_supported(&parse("1.21")));
        assert!(is_neoforge_supported(&parse("1.21.1")));
        assert!(is_neoforge_supported(&parse("26.2")));
        assert!(!is_neoforge_supported(&parse("1.19.4")));
    }

    #[test]
    fn builds_the_neoforge_version_prefix() {
        assert_eq!(neoforge_prefix(&parse("1.20.2")), "20.2.");
        assert_eq!(neoforge_prefix(&parse("1.21")), "21.0.");
        assert_eq!(neoforge_prefix(&parse("1.21.1")), "21.1.");
        assert_eq!(neoforge_prefix(&parse("26.2")), "26.2.");
    }

    #[test]
    fn parses_promotions() {
        assert_eq!("LATEST".parse::<Promotion>().unwrap(), Promotion::Latest);
        assert_eq!(
            "Recommended".parse::<Promotion>().unwrap(),
            Promotion::Recommended
        );
        assert!("stable".parse::<Promotion>().is_err());
    }
}
