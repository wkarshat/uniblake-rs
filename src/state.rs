//! Streaming state.

use crate::compress::compress;
use crate::{Hash, BLOCK_BYTES, OUT_BYTES};

/// A hashing computation in progress.
///
/// `update` compresses every whole block as soon as more input follows,
/// keeping at most one block pending. That is what makes a shared prefix cost
/// one absorption rather than one per digest, and it is a property of this
/// interface rather than of the implementation behind it.
///
/// `Clone` is the prefix mechanism: cloning a prefix-absorbed state and
/// appending a tail costs a single compression per digest.
///
/// ```
/// let base = uniblake::Params::new().hash_length(32).to_state()
///     .update_owned(&[7u8; 140]);
/// let a = base.clone().update_owned(&0u32.to_le_bytes()).finalize();
/// let b = base.clone().update_owned(&1u32.to_le_bytes()).finalize();
/// assert_ne!(a, b);
/// ```
#[derive(Clone, Debug)]
pub struct State {
    h: [u64; 8],
    // Two u64 rather than one u128, deliberately. u128's alignment is
    // target-dependent -- 8 on x86-64 before rustc 1.77, 16 after, and 16 on
    // aarch64 throughout -- and the tail padding that follows from it changed
    // size_of::<State>() between 216 and 224 across targets and toolchains.
    // Nothing else in this struct has ABI-dependent alignment, so removing the
    // u128 makes the size genuinely invariant instead of merely asserted. This
    // mirrors the C library, whose state holds `uint64_t t[2]` for the same
    // reason. Order is [low, high].
    t: [u64; 2],
    buf: [u8; BLOCK_BYTES],
    // u8 rather than usize: both are bounded by 128 and 64, and the state is
    // cloned once per digest in the prefix workload, so its size is on the
    // hot path.
    buflen: u8,
    outlen: u8,
}

impl State {
    /// A state with default parameters and a 64-byte digest.
    ///
    /// Equivalent to `Params::new().to_state()`.
    #[must_use]
    pub fn new() -> Self {
        crate::Params::new().to_state()
    }

    pub(crate) fn from_parts(h: [u64; 8], outlen: usize) -> Self {
        Self {
            h,
            t: [0, 0],
            buf: [0; BLOCK_BYTES],
            buflen: 0,
            outlen: outlen as u8,
        }
    }

    /// Compress one full block, advancing the counter. Not finalization.
    pub(crate) fn absorb(&mut self, block: &[u8; BLOCK_BYTES]) {
        self.t[0] = self.t[0].wrapping_add(BLOCK_BYTES as u64);
        self.t[1] = self.t[1].wrapping_add(u64::from(self.t[0] < BLOCK_BYTES as u64));
        compress(&mut self.h, block, self.t, false);
    }

    /// Absorb input, compressing whole blocks eagerly.
    ///
    /// A full block is retained rather than compressed only when nothing
    /// follows it: finalization has to mark the last block, so the trailing
    /// block cannot be flushed early.
    pub fn update(&mut self, mut input: &[u8]) -> &mut Self {
        // Fill and flush the pending block, but only if more input follows.
        if self.buflen > 0 {
            let want = BLOCK_BYTES - self.buflen as usize;
            if input.len() > want {
                self.buf[self.buflen as usize..].copy_from_slice(&input[..want]);
                input = &input[want..];
                self.buflen = 0;
                let block = self.buf;
                self.absorb(&block);
            }
        }

        // Whole blocks, keeping the last one pending.
        while input.len() > BLOCK_BYTES {
            let mut block = [0u8; BLOCK_BYTES];
            block.copy_from_slice(&input[..BLOCK_BYTES]);
            self.absorb(&block);
            input = &input[BLOCK_BYTES..];
        }

        let at = self.buflen as usize;
        self.buf[at..at + input.len()].copy_from_slice(input);
        self.buflen += input.len() as u8;
        self
    }

    /// `update` by value, for chaining from a builder.
    ///
    /// Convenience only, and **not** for the per-digest inner loop: taking and
    /// returning `Self` moves the 224-byte state, measured at 16.4 ns against
    /// 4.8 ns for `clone()` then `update()` in place. Use it to build a prefix
    /// state once; use `update` for each tail.
    #[must_use]
    pub fn update_owned(mut self, input: &[u8]) -> Self {
        self.update(input);
        self
    }

    /// The digest, without consuming the state.
    ///
    /// Taking `&self` matches what consumers expect and lets a caller
    /// finalize a clone while continuing to hash the original.
    #[must_use]
    pub fn finalize(&self) -> Hash {
        let mut h = self.h;

        // Pad into a fresh block rather than copying the whole buffer and
        // zeroing the tail: only `buflen` bytes are live, and buflen is
        // typically small in the prefix workload.
        let n = self.buflen as usize;
        let mut block = [0u8; BLOCK_BYTES];
        block[..n].copy_from_slice(&self.buf[..n]);

        // The pending bytes count toward the length in the final block only.
        let mut t = self.t;
        t[0] = t[0].wrapping_add(u64::from(self.buflen));
        t[1] = t[1].wrapping_add(u64::from(t[0] < u64::from(self.buflen)));
        compress(&mut h, &block, t, true);

        let mut out = [0u8; OUT_BYTES];
        for (i, word) in h.iter().enumerate() {
            out[i * 8..i * 8 + 8].copy_from_slice(&word.to_le_bytes());
        }
        Hash::new(out, self.outlen as usize)
    }

    /// Bytes absorbed so far, excluding any pending partial block.
    #[must_use]
    pub fn count(&self) -> u128 {
        u128::from(self.t[0]) | (u128::from(self.t[1]) << 64)
    }

    /// Bytes pending in the unflushed block, 0 to 128.
    ///
    /// With `prefix_check`, this is what tells a caller whether the geometry
    /// admits one-compression digests.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.buflen as usize
    }

    /// Whether a tail of `tail_len` bytes can be appended and finalized in a
    /// single compression.
    ///
    /// A digest costs one compression exactly when
    /// `pending() + tail_len <= 128`. BLAKE2b retains a *full* trailing block
    /// rather than flushing it, so a prefix that is a positive multiple of 128
    /// leaves 128 bytes pending and admits no tail at all: hash one byte less,
    /// or accept two compressions per digest.
    #[must_use]
    pub fn prefix_check(&self, tail_len: usize) -> bool {
        tail_len <= BLOCK_BYTES - self.buflen as usize
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}
