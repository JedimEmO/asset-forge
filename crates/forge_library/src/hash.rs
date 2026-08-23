//! Content hashes, so integrity is auditable where reproduction is not.
//!
//! Neither MOSS nor ACE-Step is bit-reproducible: re-running a prompt with the
//! recorded seed gives a similar sound, not the same bytes. So "prove this
//! asset rebuilds" — which the clip audit can do — is not on offer for audio,
//! and Blender's glTF export is not byte-stable either, so it is not on offer
//! for bodies and models. What *is* on offer is "prove this asset is the one
//! that was measured, reviewed and approved", and that is a hash of the file.

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::{LibraryError, Result};

/// How much of a file to read at a time. Large enough that the syscall cost
/// disappears, small enough that hashing a 90-second track does not allocate a
/// copy of it.
const CHUNK: usize = 64 * 1024;

/// The prefix every hash this crate writes carries.
pub const PREFIX: &str = "sha256:";

/// The SHA-256 of a file, lower-case hex, prefixed with the algorithm.
///
/// The `sha256:` prefix is there so a future change of algorithm is a visible
/// difference in the record rather than a silent one — a bare hex string gives
/// a reader no way to tell which function produced it.
///
/// # Errors
///
/// Fails when the file cannot be opened or read, naming it — an integrity
/// check that runs over a whole library has to say *which* file it choked on.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).map_err(|e| LibraryError::io(path, e))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| LibraryError::io(path, e))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{PREFIX}{:x}", hasher.finalize()))
}

/// The SHA-256 of some bytes, in the same form as [`sha256_file`].
#[must_use]
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{PREFIX}{:x}", hasher.finalize())
}

/// A bare lower-case hex digest — the form `forge_rig` records in a
/// contract's `sources` — in the prefixed form everything here uses.
#[must_use]
pub fn prefixed(bare_hex: &str) -> String {
    if bare_hex.starts_with(PREFIX) {
        bare_hex.to_owned()
    } else {
        format!("{PREFIX}{bare_hex}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_reference_vector() {
        // The canonical SHA-256 of "abc".
        assert_eq!(
            sha256_bytes(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_file_hashes_the_same_as_its_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("payload.bin");
        // Deliberately longer than one read chunk, so the streaming loop runs
        // more than once — an off-by-one there would still pass on small files.
        let bytes: Vec<u8> = (0..CHUNK * 2 + 7).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &bytes).expect("write");
        assert_eq!(sha256_file(&path).expect("hash"), sha256_bytes(&bytes));
    }

    #[test]
    fn the_prefix_is_added_once() {
        assert_eq!(prefixed("ab"), "sha256:ab");
        assert_eq!(prefixed("sha256:ab"), "sha256:ab");
        assert_eq!(
            prefixed(&forge_rig::sha256_hex(b"abc")),
            sha256_bytes(b"abc")
        );
    }
}
