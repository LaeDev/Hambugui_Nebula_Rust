use std::fs::File;
use std::io::Read;
use std::path::Path;

use md5::{Digest, Md5};
use sha1::Sha1;
use sha2::{Sha256, Sha512};

use crate::error::{Result, io};

const BUFFER: usize = 64 * 1024;

/// MurmurHash2 constants, from Austin Appleby's public-domain reference.
const MURMUR2_M: u32 = 0x5bd1_e995;
const MURMUR2_R: u32 = 24;
/// CurseForge forces this seed (facts §B.6).
const MURMUR2_SEED: u32 = 1;

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

/// Modrinth's preferred download digest. A separate streaming pass rather than
/// a fourth [`Digests`] field, so every resolver read does not grow a hasher.
pub fn sha512_hex_of_file(path: &Path) -> Result<String> {
    let mut sha512 = Sha512::new();
    stream(path, |chunk| sha512.update(chunk))?;
    Ok(hex::encode(sha512.finalize()))
}

/// [`sha512_hex_of_file`] for bytes already in memory.
pub fn sha512_hex(bytes: &[u8]) -> String {
    hex::encode(Sha512::digest(bytes))
}

/// CurseForge's MurmurHash2 variant over the bytes of `path` (facts §B.6).
///
/// Two passes: the seed is `1 ^ filtered_len`, so the filtered length has to be
/// known before the first mix. Folding it into [`digests_of_file`] is therefore
/// impossible, and buffering the whole jar to avoid the second read is not
/// worth the memory.
pub fn curseforge_fingerprint_of_file(path: &Path) -> Result<u32> {
    let mut filtered_len = 0u32;
    stream(path, |chunk| {
        let kept = chunk.iter().filter(|byte| !is_dropped(**byte)).count();
        filtered_len = filtered_len.wrapping_add(kept as u32);
    })?;
    let mut hasher = Murmur2::new(filtered_len);
    stream(path, |chunk| hasher.update(chunk))?;
    Ok(hasher.finish())
}

/// [`curseforge_fingerprint_of_file`] for bytes already in memory.
pub fn curseforge_fingerprint(bytes: &[u8]) -> u32 {
    let filtered_len = bytes.iter().filter(|byte| !is_dropped(**byte)).count() as u32;
    let mut hasher = Murmur2::new(filtered_len);
    hasher.update(bytes);
    hasher.finish()
}

/// Tab, LF, CR and space are dropped from the stream before hashing.
fn is_dropped(byte: u8) -> bool {
    matches!(byte, 0x09 | 0x0a | 0x0d | 0x20)
}

/// Incremental MurmurHash2 that consumes the *unfiltered* stream and drops the
/// whitespace itself, so the caller can feed it arbitrary read chunks.
struct Murmur2 {
    hash: u32,
    tail: [u8; 4],
    tail_len: usize,
}

impl Murmur2 {
    fn new(filtered_len: u32) -> Self {
        Self {
            hash: MURMUR2_SEED ^ filtered_len,
            tail: [0; 4],
            tail_len: 0,
        }
    }

    fn update(&mut self, chunk: &[u8]) {
        for byte in chunk.iter().copied().filter(|byte| !is_dropped(*byte)) {
            self.tail[self.tail_len] = byte;
            self.tail_len += 1;
            if self.tail_len == 4 {
                self.mix(u32::from_le_bytes(self.tail));
                self.tail_len = 0;
            }
        }
    }

    fn mix(&mut self, block: u32) {
        let mut block = block.wrapping_mul(MURMUR2_M);
        block ^= block >> MURMUR2_R;
        block = block.wrapping_mul(MURMUR2_M);
        self.hash = self.hash.wrapping_mul(MURMUR2_M) ^ block;
    }

    /// The reference tail switch falls through, so the trailing `h *= m` runs
    /// once for any tail of one to three bytes and not at all for none.
    fn finish(self) -> u32 {
        let mut hash = self.hash;
        for index in (0..self.tail_len).rev() {
            hash ^= u32::from(self.tail[index]) << (8 * index);
        }
        if self.tail_len > 0 {
            hash = hash.wrapping_mul(MURMUR2_M);
        }
        hash ^= hash >> 13;
        hash = hash.wrapping_mul(MURMUR2_M);
        hash ^= hash >> 15;
        hash
    }
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
    const SHA512_ABC: &str = concat!(
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a",
        "2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
    );
    /// Derived by hand, not by this module: an input that filters to nothing
    /// has no body and no tail, so the whole algorithm is
    /// `avalanche(1 ^ 0)` = `0x5bd1e995 ^ (0x5bd1e995 >> 15)` = `0x5bd15e36`.
    const MURMUR2_EMPTY: u32 = 0x5bd1_5e36;

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

    #[test]
    fn hashes_sha512_from_bytes_and_from_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc.txt");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(sha512_hex(b"abc"), SHA512_ABC);
        assert_eq!(sha512_hex_of_file(&path).unwrap(), SHA512_ABC);
    }

    #[test]
    fn fingerprints_an_input_that_filters_away_to_nothing() {
        assert_eq!(curseforge_fingerprint(b""), MURMUR2_EMPTY);
        // Only dropped bytes, so it is the same hash as the empty input.
        assert_eq!(curseforge_fingerprint(b" \t\r\n \t"), MURMUR2_EMPTY);
    }

    #[test]
    fn fingerprints_chunked_reads_like_one_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.jar");
        // Whitespace at irregular offsets so the filter shifts every block
        // boundary and the tail carried between chunks is exercised.
        let payload: Vec<u8> = (0..BUFFER * 3 + 7)
            .map(|index| match index % 7 {
                0 => b' ',
                3 => b'\n',
                other => b'a' + (other as u8),
            })
            .collect();
        std::fs::write(&path, &payload).unwrap();
        assert_eq!(
            curseforge_fingerprint_of_file(&path).unwrap(),
            curseforge_fingerprint(&payload)
        );
    }
}
