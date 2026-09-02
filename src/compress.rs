//! The BLAKE2b compression function (RFC 7693 §3.2).
//!
//! Portable, no `unsafe`, no target-specific code. Everything above this
//! module is written against `compress` alone, so a SIMD replacement swaps one
//! function.

/// Initialization vector, RFC 7693 §2.6. The first eight fractional parts of
/// the square roots of the first eight primes, as SHA-512 uses.
pub(crate) const IV: [u64; 8] = [
    0x6a09_e667_f3bc_c908,
    0xbb67_ae85_84ca_a73b,
    0x3c6e_f372_fe94_f82b,
    0xa54f_f53a_5f1d_36f1,
    0x510e_527f_ade6_82d1,
    0x9b05_688c_2b3e_6c1f,
    0x1f83_d9ab_fb41_bd6b,
    0x5be0_cd19_137e_2179,
];

/// Message word permutation, RFC 7693 §2.7. Rounds 10 and 11 repeat rows 0
/// and 1: BLAKE2b has twelve rounds over ten distinct permutations.
const SIGMA: [[usize; 16]; 12] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
];

/// The G mixing function, RFC 7693 §3.1.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn g(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
    v[d] = (v[d] ^ v[a]).rotate_right(32);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(24);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(63);
}

/// Compress one 128-byte block into `h`.
///
/// `t` is the byte counter *including* this block and `last` sets the
/// finalization flag; both are the caller's to set, because finalization
/// compresses a block whose counter must not advance.
///
/// The mixing function and the SIGMA schedule are transcribed from RFC 7693 --
/// G from s3.1, whose pseudocode fixes both the operation order and the
/// parameter names, and SIGMA from s2.7. Any conforming implementation has
/// these same lines; they are not adapted from another crate, and this crate
/// contains no third-party code. `blake2b_simd` appears only as a
/// dev-dependency oracle in tests.
///
/// The twelve rounds are fully unrolled. Partial unrolling was measured and is
/// worse at every factor tried, on an Apple M4 Pro:
///
/// | rounds per iteration | ns/digest |
/// |---|--:|
/// | 12 (full) | 84 |
/// | 4 | 98 |
/// | 3 | 99 |
/// | 2 | 95 |
/// | 6 | 107 |
///
/// The reason is the message schedule, not register pressure. A rolled loop
/// must load `SIGMA[r][i]` at runtime and compute an address per message word;
/// unrolled, every index is a compile-time constant and no `ldrb` remains.
///
/// The comparison against C that once lived here is superseded. It measured a
/// *rolled* C body and concluded the two languages diverged. The C library has
/// since been hand-unrolled, and on aarch64 the two kernels are now
/// assembly-equivalent: 1516 instructions against 1546, 380 rotates each
/// (identical `{16:96, 24:96, 32:96, 63:92}` histograms, including the four
/// final-round rotates both compilers fold into the output XOR), zero sigma
/// byte-loads each, and real spill traffic of 0.150 against 0.153 ops per
/// rotate once callee-saved registers are excluded.
///
/// So the ~4.9 ns this crate leads by at the leaf shape is **not** in this
/// function. It is in the surrounding path: `State::finalize` takes `&self`
/// and works on local copies, where C's `ub_final` mutates its 232-byte state
/// in place. Porting that shape to C measured 2.5 ns *slower* there, so the
/// difference is not portable in either direction. See README.md and the C
/// library's `docs/INTERNALS.md`.
#[inline(always)]
pub(crate) fn compress(h: &mut [u64; 8], block: &[u8; 128], t: [u64; 2], last: bool) {
    // Chunk-based load: `chunks_exact(8)` yields slices the compiler can prove
    // are 8 bytes, so `try_into` is a compile-time-checked reinterpret rather
    // than a bounds-checked copy.
    let mut m = [0u64; 16];
    for (word, chunk) in m.iter_mut().zip(block.chunks_exact(8)) {
        *word = u64::from_le_bytes(chunk.try_into().unwrap());
    }

    // Built as one literal rather than zeroed and overwritten: the compiler
    // then has no store-forwarding dependency to resolve, and `last` becomes a
    // mask rather than a branch.
    let last_mask = if last { !0u64 } else { 0 };
    let mut v = [
        h[0],
        h[1],
        h[2],
        h[3],
        h[4],
        h[5],
        h[6],
        h[7],
        IV[0],
        IV[1],
        IV[2],
        IV[3],
        IV[4] ^ t[0],
        IV[5] ^ t[1],
        IV[6] ^ last_mask,
        IV[7],
    ];

    #[inline(always)]
    fn round(v: &mut [u64; 16], m: &[u64; 16], s: &[usize; 16]) {
        g(v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
        g(v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
        g(v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
        g(v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
        g(v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
        g(v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
        g(v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
        g(v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
    }
    round(&mut v, &m, &SIGMA[0]);
    round(&mut v, &m, &SIGMA[1]);
    round(&mut v, &m, &SIGMA[2]);
    round(&mut v, &m, &SIGMA[3]);
    round(&mut v, &m, &SIGMA[4]);
    round(&mut v, &m, &SIGMA[5]);
    round(&mut v, &m, &SIGMA[6]);
    round(&mut v, &m, &SIGMA[7]);
    round(&mut v, &m, &SIGMA[8]);
    round(&mut v, &m, &SIGMA[9]);
    round(&mut v, &m, &SIGMA[10]);
    round(&mut v, &m, &SIGMA[11]);

    for i in 0..8 {
        h[i] ^= v[i] ^ v[i + 8];
    }
}
