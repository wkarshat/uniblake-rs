//! ABI invariance of the state.
//!
//! Size must be identical on every target: a caller that allocates a state in
//! one build and reads it in another must agree on how big it is. Alignment
//! may legitimately differ -- i686 aligns u128 to 4 where LP64 targets use 8 --
//! so it is reported, not fixed.
const _: () = assert!(core::mem::size_of::<uniblake::State>() == 224);

#[test]
fn state_size_is_abi_invariant() {
    assert_eq!(core::mem::size_of::<uniblake::State>(), 224);
}

#[test]
fn state_alignment_is_at_least_word() {
    assert!(core::mem::align_of::<uniblake::State>() >= 4);
}
