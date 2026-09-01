//! The BLAKE2b parameter block (RFC 7693 §2.5).

use crate::compress::IV;
use crate::state::State;
use crate::{KEY_BYTES, OUT_BYTES, PERSONAL_BYTES, SALT_BYTES};

/// What is being computed, as distinct from the message.
///
/// The parameter block is mixed into the initial state rather than into the
/// message, so two hashes with different personalization are unrelated
/// functions at no per-message cost.
///
/// ```
/// let h = uniblake::Params::new()
///     .hash_length(32)
///     .personal(b"ZcashBlockCommit")
///     .to_state()
///     .update(b"data")
///     .finalize();
/// assert_eq!(h.len(), 32);
/// ```
#[derive(Clone, Debug)]
pub struct Params {
    // u8: both are bounded by 64, and usize would force 8-byte alignment and
    // pad the type.
    hash_length: u8,
    key_length: u8,
    salt: [u8; SALT_BYTES],
    personal: [u8; PERSONAL_BYTES],
}

impl Default for Params {
    fn default() -> Self {
        Self::new()
    }
}

impl Params {
    /// Sequential-mode defaults with a 64-byte digest.
    #[must_use]
    pub fn new() -> Self {
        Self {
            hash_length: OUT_BYTES as u8,
            key_length: 0,
            salt: [0; SALT_BYTES],
            personal: [0; PERSONAL_BYTES],
        }
    }

    /// Digest length in bytes, 1 to 64.
    ///
    /// # Panics
    /// Outside that range. The length is fixed at init in correct code, so a
    /// bad value is a programming error rather than a runtime condition.
    #[must_use]
    pub fn hash_length(mut self, n: usize) -> Self {
        assert!(
            (1..=OUT_BYTES).contains(&n),
            "hash_length must be 1..=64, got {n}"
        );
        self.hash_length = n as u8;
        self
    }

    /// Declare a keyed hash of `key_length` bytes.
    ///
    /// The key material is supplied to [`Params::to_state_keyed`], not stored
    /// here: BLAKE2b absorbs the key as a zero-padded first block rather than
    /// mixing it into the parameter block, so only its length belongs in these
    /// parameters. Keeping a 64-byte buffer in every `Params` would cost every
    /// caller for a path most do not use.
    ///
    /// # Panics
    /// If `key_length` exceeds 64.
    #[must_use]
    pub fn key_length(mut self, key_length: usize) -> Self {
        assert!(key_length <= KEY_BYTES, "key_length must be at most 64");
        self.key_length = key_length as u8;
        self
    }

    /// Salt, up to 16 bytes, zero-padded.
    ///
    /// # Panics
    /// If longer than 16 bytes.
    #[must_use]
    pub fn salt(mut self, salt: &[u8]) -> Self {
        assert!(salt.len() <= SALT_BYTES, "salt must be at most 16 bytes");
        self.salt = [0; SALT_BYTES];
        self.salt[..salt.len()].copy_from_slice(salt);
        self
    }

    /// Personalization, up to 16 bytes, zero-padded. Domain separation.
    ///
    /// # Panics
    /// If longer than 16 bytes.
    #[must_use]
    pub fn personal(mut self, personal: &[u8]) -> Self {
        assert!(
            personal.len() <= PERSONAL_BYTES,
            "personal must be at most 16 bytes"
        );
        self.personal = [0; PERSONAL_BYTES];
        self.personal[..personal.len()].copy_from_slice(personal);
        self
    }

    /// A streaming state with these parameters absorbed.
    ///
    /// # Panics
    /// If `key_length` was set: use [`Params::to_state_keyed`] instead.
    #[must_use]
    pub fn to_state(&self) -> State {
        assert!(
            self.key_length == 0,
            "key_length was set; use to_state_keyed"
        );
        self.build(&[])
    }

    /// A streaming state with the key absorbed as the first block.
    ///
    /// # Panics
    /// If `key.len()` differs from the declared `key_length`.
    #[must_use]
    pub fn to_state_keyed(&self, key: &[u8]) -> State {
        assert_eq!(
            key.len(),
            self.key_length as usize,
            "key length differs from the declared key_length"
        );
        self.build(key)
    }

    fn build(&self, key: &[u8]) -> State {
        let mut h = IV;

        // Serialize the 64-byte block field by field and xor it in. Building
        // it as bytes keeps the layout explicit and endian-neutral rather
        // than depending on struct packing.
        let mut blk = [0u8; 64];
        blk[0] = self.hash_length;
        blk[1] = self.key_length;
        blk[2] = 1; // fanout
        blk[3] = 1; // depth
        blk[32..48].copy_from_slice(&self.salt);
        blk[48..64].copy_from_slice(&self.personal);

        for (i, word) in h.iter_mut().enumerate() {
            let mut b = [0u8; 8];
            b.copy_from_slice(&blk[i * 8..i * 8 + 8]);
            *word ^= u64::from_le_bytes(b);
        }

        let mut state = State::from_parts(h, self.hash_length as usize);
        if !key.is_empty() {
            let mut block = [0u8; crate::BLOCK_BYTES];
            block[..key.len()].copy_from_slice(key);
            state.absorb(&block);
        }
        state
    }
}
