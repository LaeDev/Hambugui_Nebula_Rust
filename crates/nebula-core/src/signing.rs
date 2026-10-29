//! The detached signature the Hambugui launcher requires next to
//! `distribution.json`.
//!
//! `<name>.json.sig` holds the Ed25519 signature as 128 lowercase hex
//! characters and one `\n`. It signs [`SIGNATURE_DOMAIN`] followed by the
//! document's bytes exactly as written, which is what the launcher checks with
//! `verify_strict` under its `HAMBUGUI_DISTRIBUTION_PUBLIC_KEY`
//! (`hambugui-distribution/src/signature.rs`).
//!
//! The signing key is a 32-byte Ed25519 seed written as 64 hex characters. It
//! comes from, in this order: a key file named on the command line,
//! `NEBULA_DISTRIBUTION_SIGNING_KEY` (the hex itself) or
//! `NEBULA_DISTRIBUTION_SIGNING_KEY_FILE` (a path), and finally
//! `distribution-signing.key` in Nebula's per-user config directory, which is
//! where `nebula keygen` puts it. The seed is never printed, logged, or written
//! anywhere but the key file `keygen` creates; errors name the source, never
//! the text.

use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use zeroize::Zeroizing;

use crate::error::{Error, Result, io};

/// The message prefix that keeps this signature from being valid for any other
/// Ed25519 use of the same key. Byte for byte the launcher's constant.
pub const SIGNATURE_DOMAIN: &[u8] = b"hambugui-distribution-v1\n";

/// The seed as 64 hex characters.
pub const KEY_ENV: &str = "NEBULA_DISTRIBUTION_SIGNING_KEY";
/// A path to a file holding the seed as 64 hex characters.
pub const KEY_FILE_ENV: &str = "NEBULA_DISTRIBUTION_SIGNING_KEY_FILE";
/// The file `nebula keygen` writes in the config directory when given no path.
pub const DEFAULT_KEY_FILE_NAME: &str = "distribution-signing.key";

/// A key file is 64 hex characters and a line ending; anything much larger is
/// not one, and is not read into memory.
const MAX_KEY_FILE_BYTES: u64 = 256;

/// An Ed25519 signing key for distributions. `Debug` shows the public key only.
#[derive(Clone)]
pub struct DistributionKey(SigningKey);

impl std::fmt::Debug for DistributionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DistributionKey")
            .field("public_key", &self.public_key_hex())
            .finish_non_exhaustive()
    }
}

impl DistributionKey {
    /// Parses a seed: 64 hex characters, surrounding whitespace (a key file's
    /// line ending) ignored. `source` names where it came from in the error,
    /// which never repeats the text.
    pub fn from_seed_hex(text: &str, source: &str) -> Result<Self> {
        let text = text.trim();
        let mut seed = Zeroizing::new([0u8; 32]);
        if text.len() != 64 || hex::decode_to_slice(text, seed.as_mut()).is_err() {
            return Err(Error::Config(format!(
                "{source} is not a distribution signing key: expected 64 hex characters \
                 (a 32-byte Ed25519 seed, as `nebula keygen` writes it)"
            )));
        }
        Self::from_seed(&seed, source)
    }

    fn from_seed(seed: &[u8; 32], source: &str) -> Result<Self> {
        // An all-zero seed is what an unset or wiped buffer looks like, never a
        // key anybody generated.
        if seed.iter().all(|byte| *byte == 0) {
            return Err(Error::Config(format!(
                "{source} holds the all-zero seed, which is refused; run `nebula keygen` \
                 for a real key"
            )));
        }
        Ok(Self(SigningKey::from_bytes(seed)))
    }

    /// Reads a key file. On Unix, a file anybody but its owner can read is
    /// used but reported.
    pub fn from_file(path: &Path) -> Result<Self> {
        let source = format!("the key file {}", path.display());
        let metadata = std::fs::metadata(path).map_err(io(format!("reading {source}")))?;
        if !metadata.is_file() || metadata.len() > MAX_KEY_FILE_BYTES {
            return Err(Error::Config(format!(
                "{source} is not a distribution signing key file"
            )));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                log::warn!(
                    "{} can be read by other users; restrict it with `chmod 600`",
                    path.display()
                );
            }
        }
        let text =
            Zeroizing::new(std::fs::read_to_string(path).map_err(io(format!("reading {source}")))?);
        Self::from_seed_hex(&text, &source)
    }

    /// A fresh key from the operating system's random source.
    pub fn generate() -> Result<Self> {
        let mut seed = Zeroizing::new([0u8; 32]);
        getrandom::fill(seed.as_mut())
            .map_err(|error| Error::Config(format!("the OS random source failed: {error}")))?;
        Self::from_seed(&seed, "the OS random source")
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.0.verifying_key()
    }

    /// The 64 lowercase hex characters the launcher is built with as
    /// `HAMBUGUI_DISTRIBUTION_PUBLIC_KEY`.
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.verifying_key().as_bytes())
    }

    /// The signature over `SIGNATURE_DOMAIN || body` as 128 lowercase hex
    /// characters.
    pub fn sign(&self, body: &[u8]) -> String {
        let message = [SIGNATURE_DOMAIN, body].concat();
        hex::encode(self.0.sign(&message).to_bytes())
    }

    /// The seed as key file text. Only `write_new_key_file` calls it.
    fn seed_file_text(&self) -> Zeroizing<String> {
        let seed = Zeroizing::new(self.0.to_bytes());
        Zeroizing::new(format!("{}\n", hex::encode(seed.as_slice())))
    }
}

/// Where a document's signature goes: the same path with `.sig` appended,
/// which is also the URL rule the launcher fetches it by.
pub fn signature_path(document: &Path) -> PathBuf {
    let mut name = document.as_os_str().to_owned();
    name.push(".sig");
    PathBuf::from(name)
}

/// Signs `body` and writes `<document>.sig` in the launcher's format. `body`
/// must be the exact bytes of `document`; the caller wrote or read them.
pub fn write_signature(key: &DistributionKey, document: &Path, body: &[u8]) -> Result<PathBuf> {
    let path = signature_path(document);
    let text = format!("{}\n", key.sign(body));
    std::fs::write(&path, text).map_err(io(format!("writing {}", path.display())))?;
    Ok(path)
}

/// Signs the file at `document` as it is on disk now and writes its `.sig`.
/// Returns the bytes that were signed, so the caller uploads what it signed.
pub fn sign_file(key: &DistributionKey, document: &Path) -> Result<(PathBuf, Vec<u8>)> {
    let body = std::fs::read(document).map_err(io(format!("reading {}", document.display())))?;
    let path = write_signature(key, document, &body)?;
    Ok((path, body))
}

/// Removes a `.sig` left from an earlier signed run, so an unsigned document
/// never sits next to a signature for other bytes.
pub fn remove_stale_signature(document: &Path) -> Result<()> {
    let path = signature_path(document);
    match std::fs::remove_file(&path) {
        Ok(()) => {
            log::warn!("Removed the stale signature {}", path.display());
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Io {
            context: format!("removing {}", path.display()),
            source: error,
        }),
    }
}

/// The key to sign with, and a description of where it came from that is safe
/// to log.
#[derive(Debug, Clone)]
pub struct ResolvedKey {
    pub key: DistributionKey,
    pub source: String,
}

/// Finds the signing key. `explicit` is a key file named on the command line;
/// `default_file` is `<config dir>/distribution-signing.key`, used only when it
/// exists. A named source that is missing or unreadable is an error rather
/// than a silent fall through to unsigned output.
pub fn resolve_key(
    explicit: Option<&Path>,
    default_file: Option<&Path>,
) -> Result<Option<ResolvedKey>> {
    if let Some(path) = explicit {
        return file_key(path).map(Some);
    }
    let env_key = env_value(KEY_ENV);
    let env_file = env_value(KEY_FILE_ENV);
    match (env_key, env_file) {
        (Some(_), Some(_)) => Err(Error::Config(format!(
            "both {KEY_ENV} and {KEY_FILE_ENV} are set; set only one"
        ))),
        (Some(text), None) => {
            let text = Zeroizing::new(text);
            Ok(Some(ResolvedKey {
                key: DistributionKey::from_seed_hex(&text, KEY_ENV)?,
                source: KEY_ENV.to_owned(),
            }))
        }
        (None, Some(path)) => file_key(Path::new(&path)).map(Some),
        (None, None) => match default_file {
            Some(path) if path.exists() => file_key(path).map(Some),
            _ => Ok(None),
        },
    }
}

fn file_key(path: &Path) -> Result<ResolvedKey> {
    Ok(ResolvedKey {
        key: DistributionKey::from_file(path)?,
        source: format!("key file {}", path.display()),
    })
}

fn env_value(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// `nebula keygen`: creates a new key file that only its owner can read (on
/// Unix; on Windows it inherits the folder's access list, and the default
/// folder is per-user). An existing file is never replaced: losing the key the
/// launcher was built with means rebuilding the launcher.
pub fn write_new_key_file(path: &Path) -> Result<DistributionKey> {
    use std::io::Write;

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(io(format!("creating {}", parent.display())))?;
    }
    let key = DistributionKey::generate()?;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            Error::Config(format!(
                "{} already exists; Nebula never overwrites a signing key. Move it away \
                 first if you really mean to replace it (the launcher must then be rebuilt \
                 with the new public key)",
                path.display()
            ))
        } else {
            Error::Io {
                context: format!("creating {}", path.display()),
                source: error,
            }
        }
    })?;
    let written = file
        .write_all(key.seed_file_text().as_bytes())
        .and_then(|()| file.sync_all());
    if let Err(error) = written {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(Error::Io {
            context: format!("writing {}", path.display()),
            source: error,
        });
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The vector the launcher's own signature tests use (contract C2).
    const SEED_42: &str = "4242424242424242424242424242424242424242424242424242424242424242";
    const PUBLIC_42: &str = "2152f8d19b791d24453242e15f2eab6cb7cffa7b6a5ed30097960e069881db12";

    #[test]
    fn parses_a_seed_and_derives_the_launcher_vector_key() {
        let key = DistributionKey::from_seed_hex(SEED_42, "test").unwrap();
        assert_eq!(key.public_key_hex(), PUBLIC_42);
        // Upper case and a key file's line ending are accepted.
        let upper = DistributionKey::from_seed_hex(&format!("{}\r\n", SEED_42.to_uppercase()), "t");
        assert_eq!(upper.unwrap().public_key_hex(), PUBLIC_42);
    }

    #[test]
    fn refuses_malformed_and_all_zero_seeds_without_echoing_them() {
        for bad in [
            "",
            "42",
            &SEED_42[..63],
            &format!("{SEED_42}00"),
            &format!("{}zz", &SEED_42[..62]),
        ] {
            let error = DistributionKey::from_seed_hex(bad, "SRC")
                .unwrap_err()
                .to_string();
            assert!(
                error.starts_with("SRC is not a distribution signing key"),
                "{error}"
            );
            if bad.len() > 8 {
                assert!(!error.contains(bad), "{error}");
            }
        }
        let zero = "0".repeat(64);
        let error = DistributionKey::from_seed_hex(&zero, "SRC")
            .unwrap_err()
            .to_string();
        assert!(error.contains("all-zero"), "{error}");
        assert!(!error.contains(&zero));
    }

    #[test]
    fn debug_never_shows_the_seed() {
        let key = DistributionKey::from_seed_hex(SEED_42, "test").unwrap();
        let shown = format!("{key:?}");
        assert!(shown.contains(PUBLIC_42), "{shown}");
        assert!(!shown.contains(SEED_42), "{shown}");
    }

    #[test]
    fn the_signature_path_appends_sig() {
        assert_eq!(
            signature_path(Path::new("/r/distribution.json")),
            Path::new("/r/distribution.json.sig")
        );
        assert_eq!(
            signature_path(Path::new("dist.gdrive.json")),
            Path::new("dist.gdrive.json.sig")
        );
    }
}
