use std::fs::File;
use std::io::Read;
use std::path::Path;

use md5::{Digest, Md5};
use sha1::Sha1;
use sha2::Sha256;

use crate::error::{Result, io};

const BUFFER: usize = 64 * 1024;

/// Every digest a resolver needs for one file: `md5` and `sha256` go into the
/// module, `sha1` is what Forge, NeoForge and Fabric manifests publish. They are
/// computed together because a library jar is otherwise read twice, once to
/// verify it and once to describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digests {
    pub size: u64,
    pub md5: String,
    pub sha1: String,
    pub sha256: String,
}

pub fn digests_of_bytes(bytes: &[u8]) -> Digests {
    Digests {
        size: bytes.len() as u64,
        md5: hex::encode(Md5::digest(bytes)),
        sha1: hex::encode(Sha1::digest(bytes)),
        sha256: hex::encode(Sha256::digest(bytes)),
    }
}

pub fn digests_of_file(path: &Path) -> Result<Digests> {
    let mut md5 = Md5::new();
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let size = stream(path, |chunk| {
        md5.update(chunk);
        sha1.update(chunk);
        sha256.update(chunk);
    })?;
    Ok(Digests {
        size,
        md5: hex::encode(md5.finalize()),
        sha1: hex::encode(sha1.finalize()),
        sha256: hex::encode(sha256.finalize()),
    })
}

pub fn sha1_hex_of_file(path: &Path) -> Result<String> {
    let mut sha1 = Sha1::new();
    stream(path, |chunk| sha1.update(chunk))?;
    Ok(hex::encode(sha1.finalize()))
}

fn stream(path: &Path, mut consume: impl FnMut(&[u8])) -> Result<u64> {
    let mut file = File::open(path).map_err(io(format!("opening {}", path.display())))?;
    let mut buffer = vec![0u8; BUFFER];
    let mut size = 0u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(io(format!("reading {}", path.display())))?;
        if read == 0 {
            return Ok(size);
        }
        consume(&buffer[..read]);
        size += read as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MD5_ABC: &str = "900150983cd24fb0d6963f7d28e17f72";
    const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const SHA1_ABC: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";

    #[test]
    fn hashes_bytes() {
        let digests = digests_of_bytes(b"abc");
        assert_eq!(digests.size, 3);
        assert_eq!(digests.md5, MD5_ABC);
        assert_eq!(digests.sha1, SHA1_ABC);
        assert_eq!(digests.sha256, SHA256_ABC);
    }

    #[test]
    fn hashes_files_the_same_way() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc.txt");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(digests_of_file(&path).unwrap(), digests_of_bytes(b"abc"));
        assert_eq!(sha1_hex_of_file(&path).unwrap(), SHA1_ABC);
    }

    #[test]
    fn hashes_payloads_larger_than_the_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.bin");
        let payload = vec![7u8; BUFFER * 2 + 13];
        std::fs::write(&path, &payload).unwrap();
        assert_eq!(digests_of_file(&path).unwrap(), digests_of_bytes(&payload));
    }
}
