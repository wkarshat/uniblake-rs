//! BLAKE2b with specified eager absorption and a shared-prefix fast path.
//!
//! Hashing one message is the usual three calls:
//!
//! ```
//! let hash = uniblake::Params::new().hash_length(32).to_state()
//!     .update(b"abc")
//!     .finalize();
//! assert_eq!(hash.as_bytes().len(), 32);
//! ```
//!
//! What this crate adds is the repeated case: `H(prefix || 0)`,
//! `H(prefix || 1)`, and so on. Whole blocks are compressed as soon as more
//! input follows, so a shared prefix is absorbed once and each digest costs a
//! single compression. The standard BLAKE2 interface cannot express that
//! requirement and gives no diagnostic when an implementation buffers instead.
//!
//! ```
//! let prefix = [7u8; 140];
//! let state = uniblake::Params::new()
//!     .hash_length(32)
//!     .personal(b"my-app-v1")
//!     .to_state()
//!     .update_owned(&prefix);
//!
//! // The prefix is absorbed once; each tail continues from it.
//! let a = state.clone().update_owned(&0u32.to_le_bytes()).finalize();
//! let b = state.clone().update_owned(&1u32.to_le_bytes()).finalize();
//! assert_ne!(a, b);
//! ```

#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod compress;
mod params;
mod state;

pub use params::Params;
pub use state::State;

/// Compression block size in bytes.
pub const BLOCK_BYTES: usize = 128;
/// Largest digest this hash produces.
pub const OUT_BYTES: usize = 64;
/// Largest key.
pub const KEY_BYTES: usize = 64;
/// Salt field width.
pub const SALT_BYTES: usize = 16;
/// Personalization field width.
pub const PERSONAL_BYTES: usize = 16;

/// A finalized digest.
///
/// Comparison is by value and is not constant-time; a digest is public data.
/// Callers comparing a MAC should use a constant-time primitive of their own.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Hash {
    bytes: [u8; OUT_BYTES],
    // u8, not usize: the length is 1..=64, and a usize here would force
    // 8-byte alignment and pad the type from 65 bytes to 72.
    len: u8,
}

impl Hash {
    /// The digest bytes, exactly `hash_length` of them.
    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    /// The digest length in bytes.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// Whether the digest is empty. Never true: lengths are 1..=64.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn new(bytes: [u8; OUT_BYTES], len: usize) -> Self {
        Self {
            bytes,
            len: len as u8,
        }
    }
}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl From<Hash> for [u8; 32] {
    /// # Panics
    /// If the digest is not 32 bytes. A caller that asked for 32 gets 32.
    fn from(h: Hash) -> Self {
        let mut out = [0u8; 32];
        out.copy_from_slice(h.as_bytes());
        out
    }
}

impl From<Hash> for [u8; 64] {
    /// # Panics
    /// If the digest is not 64 bytes.
    fn from(h: Hash) -> Self {
        let mut out = [0u8; 64];
        out.copy_from_slice(h.as_bytes());
        out
    }
}
