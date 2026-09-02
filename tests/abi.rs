//! ABI invariance of the state.
//!
//! Size must be identical on every target: a caller that allocates a state in
//! one build and reads it in another must agree on how big it is.
//!
//! This is a property the struct has to be *built* for, not one it gets for
//! free. The counter was a `u128` until it was measured varying: u128's
//! alignment is 8 on x86-64 before rustc 1.77 and 16 from 1.77 on, and 16 on
//! aarch64 throughout, so tail padding gave size_of::<State>() == 216 on
//! x86_64-unknown-linux-gnu under rustc 1.69 against 224 everywhere else.
//! Storing the counter as `[u64; 2]` removes the only ABI-dependent field, so
//! every field is now fixed-width and fixed-alignment.
//!
//! Adding a field whose size or alignment varies with the target -- `usize`,
//! a pointer, `u128`, or a struct containing one -- reintroduces that. If you
//! change these fields, re-measure on a 64- and a 32-bit target: `cargo test`
//! on one host does not catch it.
const _: () = assert!(core::mem::size_of::<uniblake::State>() == 216);

#[test]
fn state_size_is_abi_invariant() {
    assert_eq!(core::mem::size_of::<uniblake::State>(), 216);
}

#[test]
fn state_alignment_is_at_least_word() {
    assert!(core::mem::align_of::<uniblake::State>() >= 4);
}
