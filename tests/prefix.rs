//! A2: the prefix property, verified by counting compressions rather than by
//! inferring it from timing.
//!
//! `State::count()` reports bytes absorbed, which advances by exactly 128 per
//! compression. Comparing it before and after a tail gives the compression
//! count for that digest directly.

/// Compressions performed while producing one digest from `base` plus `tail`.
fn compressions_for_digest(base: &uniblake::State, tail: &[u8]) -> u128 {
    let before = base.count();
    let mut s = base.clone();
    s.update(tail);
    // finalize() compresses the pending block once more, which count() does
    // not observe, so add it explicitly.
    let absorbed = (s.count() - before) / 128;
    absorbed + 1
}

#[test]
fn one_compression_per_digest_when_geometry_allows() {
    // 140-byte prefix leaves 12 bytes pending, so a tail up to 116 bytes fits
    // in the final block and the digest costs exactly one compression.
    let base = uniblake::Params::new()
        .hash_length(32)
        .to_state()
        .update_owned(&[7u8; 140]);

    assert_eq!(base.pending(), 12);
    for tail_len in [0usize, 1, 4, 8, 100, 116] {
        assert!(base.prefix_check(tail_len), "tail {tail_len} should fit");
        assert_eq!(
            compressions_for_digest(&base, &vec![0u8; tail_len]),
            1,
            "tail length {tail_len}"
        );
    }
}

#[test]
fn geometry_check_rejects_a_tail_that_does_not_fit() {
    let base = uniblake::Params::new()
        .hash_length(32)
        .to_state()
        .update_owned(&[7u8; 140]);

    // 117 overflows the 116 bytes remaining, so it costs two compressions.
    assert!(!base.prefix_check(117));
    assert_eq!(compressions_for_digest(&base, &[0u8; 117]), 2);
}

#[test]
fn a_block_multiple_prefix_admits_no_tail() {
    // BLAKE2b retains a full trailing block rather than flushing it, because
    // finalization must mark the last block. So a prefix that is a positive
    // multiple of 128 leaves 128 pending, not 0, and no tail fits.
    let base = uniblake::Params::new()
        .hash_length(32)
        .to_state()
        .update_owned(&[7u8; 256]);

    assert_eq!(base.pending(), 128);
    assert!(base.prefix_check(0));
    assert!(!base.prefix_check(1));
    assert_eq!(compressions_for_digest(&base, &[0u8; 1]), 2);
}

#[test]
fn empty_prefix_admits_a_whole_block() {
    let base = uniblake::Params::new().hash_length(32).to_state();
    assert_eq!(base.pending(), 0);
    assert!(base.prefix_check(128));
    assert!(!base.prefix_check(129));
}
