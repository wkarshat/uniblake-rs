//! Byte agreement with an independent implementation.
//!
//! `blake2b_simd` is the oracle: a separate codebase, the crate every
//! Zcash-lineage Rust consumer already depends on. Agreement across the
//! parameter space is stronger evidence than any fixed vector list, because it
//! covers shapes nobody thought to tabulate.

fn oracle(outlen: usize, key: &[u8], salt: &[u8], personal: &[u8], input: &[u8]) -> Vec<u8> {
    let mut p = blake2b_simd::Params::new();
    p.hash_length(outlen);
    if !key.is_empty() {
        p.key(key);
    }
    if !salt.is_empty() {
        p.salt(salt);
    }
    if !personal.is_empty() {
        p.personal(personal);
    }
    p.hash(input).as_bytes().to_vec()
}

fn ours(outlen: usize, key: &[u8], salt: &[u8], personal: &[u8], input: &[u8]) -> Vec<u8> {
    let mut p = uniblake::Params::new().hash_length(outlen);
    if !key.is_empty() {
        p = p.key_length(key.len());
    }
    if !salt.is_empty() {
        p = p.salt(salt);
    }
    if !personal.is_empty() {
        p = p.personal(personal);
    }
    let mut st = if key.is_empty() {
        p.to_state()
    } else {
        p.to_state_keyed(key)
    };
    st.update(input);
    st.finalize().as_bytes().to_vec()
}

fn seq(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 + 1) as u8).collect()
}

#[test]
fn agrees_across_message_lengths() {
    // 0 through 600 spans empty, sub-block, the 128-byte boundary, and
    // several whole blocks. Counter mistakes surface past one block.
    for n in 0..=600 {
        let input = seq(n);
        assert_eq!(
            ours(64, &[], &[], &[], &input),
            oracle(64, &[], &[], &[], &input),
            "message length {n}"
        );
    }
}

#[test]
fn agrees_across_digest_lengths() {
    let input = seq(200);
    for outlen in 1..=64 {
        assert_eq!(
            ours(outlen, &[], &[], &[], &input),
            oracle(outlen, &[], &[], &[], &input),
            "digest length {outlen}"
        );
    }
}

#[test]
fn agrees_across_key_lengths() {
    let input = seq(200);
    for keylen in 1..=64 {
        let key = seq(keylen);
        assert_eq!(
            ours(32, &key, &[], &[], &input),
            oracle(32, &key, &[], &[], &input),
            "key length {keylen}"
        );
    }
}

#[test]
fn agrees_on_salt_and_personalization() {
    let input = seq(300);
    for n in 1..=16 {
        let salt = seq(n);
        let personal: Vec<u8> = (0..n).map(|i| 0xA0u8.wrapping_add(i as u8)).collect();
        assert_eq!(
            ours(48, &[], &salt, &personal, &input),
            oracle(48, &[], &salt, &personal, &input),
            "salt/personal length {n}"
        );
        // Keyed together with both, the case adapters most often get wrong.
        let key = seq(32);
        assert_eq!(
            ours(48, &key, &salt, &personal, &input),
            oracle(48, &key, &salt, &personal, &input),
            "keyed salt/personal length {n}"
        );
    }
}

#[test]
fn chunked_updates_match_one_shot() {
    // Absorbing in pieces must equal absorbing at once. The chunk sizes
    // straddle the block boundary in both directions.
    let input = seq(500);
    let want = oracle(32, &[], &[], &[], &input);
    for chunk in [1, 2, 7, 63, 64, 65, 127, 128, 129, 200, 256, 499, 500] {
        let mut s = uniblake::Params::new().hash_length(32).to_state();
        for piece in input.chunks(chunk) {
            s.update(piece);
        }
        assert_eq!(s.finalize().as_bytes(), &want[..], "chunk size {chunk}");
    }
}

#[test]
fn clone_continues_independently() {
    // The prefix mechanism: one absorbed state, many tails.
    let prefix = seq(140);
    let base = uniblake::Params::new()
        .hash_length(50)
        .personal(b"uniblake-test")
        .to_state()
        .update_owned(&prefix);

    for i in 0u32..64 {
        let mut whole = prefix.clone();
        whole.extend_from_slice(&i.to_le_bytes());
        let want = oracle(50, &[], &[], b"uniblake-test", &whole);

        let got = base.clone().update_owned(&i.to_le_bytes()).finalize();
        assert_eq!(got.as_bytes(), &want[..], "counter {i}");
    }
}

#[test]
fn finalize_does_not_consume() {
    // Consumers finalize through a &mut and keep hashing; librustzcash's FFI
    // shim depends on it.
    let mut s = uniblake::Params::new().hash_length(32).to_state();
    s.update(b"abc");
    let first = s.finalize();
    let second = s.finalize();
    assert_eq!(first, second);
    s.update(b"def");
    assert_ne!(s.finalize(), first);
}
