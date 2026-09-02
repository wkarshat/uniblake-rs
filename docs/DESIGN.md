# uniblake-rs: goals, method, acceptance

A native Rust BLAKE2b with the same shape as the C `uniblake`: eager absorption
specified as interface behaviour, and a prefix layer built on it. Not a binding
to the C library, and not a fork of an existing crate.

This document is the plan. It states what must be true before any code is
written, so that "done" is a measurement rather than an opinion.

## 1. Why this exists

`blake2b_simd` is the incumbent for Zcash-lineage Rust. It is good: pure Rust,
full parameter block, runtime SIMD dispatch, and the crate Zebra and
librustzcash already depend on. Replacing it needs a reason.

The reason is the same one the C library has. A workload that hashes

    H(prefix || 0), H(prefix || 1), H(prefix || 2), ...

should absorb `prefix` once. Whether an implementation *reaches* that state is
not specified by any BLAKE2 API: `update` may buffer rather than compress, and
the caller cannot tell, cannot request otherwise, and gets no diagnostic when
the cost silently triples. `blake2b_simd` handles this by exposing `State:
Clone`, which librustzcash's Equihash component uses through an FFI shim
(`components/equihash/src/blake2b.rs`: `blake2b_init`, `blake2b_clone`,
`blake2b_free`). That works, but it makes the caller responsible for a property
the library could guarantee and check.

So the contribution is not speed. It is:

- **absorption as a contract**, not an implementation accident;
- **a geometry check** that answers, before any hashing, whether the caller's
  sizes permit one-compression digests;
- **a batch entry point** that takes the whole digest range and the output
  layout in one call, so a parallel or SIMD implementation has something to
  attach to.

If those are not worth having, the correct answer is to keep using
`blake2b_simd` and this project should not exist. That judgement is revisited
at each milestone below, not deferred.

## 2. Non-goals

- **Not a `blake2b_simd` replacement by default.** No claim of being faster
  until measured faster on the same machine and shape.
- **No BLAKE2s, BLAKE2X, BLAKE3, or tree modes.** BLAKE2b only, matching the C
  library's scope.
- **No `unsafe` in the portable path.** SIMD, if it is ever added, is the only
  place `unsafe` is permitted, behind a feature flag and a runtime probe.
- **No async, no allocation in the hot path, no global state.**

## 3. Consumer requirements, from actual call sites

Read from the local Zebra and librustzcash trees, not assumed.

Zebra (`blake2b_simd = "1.0"`, used in `zebra-chain` and `zebra-consensus`)
calls exactly one shape, in three places:

```rust
blake2b_simd::Params::new()
    .hash_length(32)
    .personal(b"ZcashBlockCommit")
    .to_state()
    .update(&a).update(&b)
    .finalize()
```

librustzcash additionally uses `State` directly and requires `State: Clone` --
its Equihash component clones a prefix-absorbed state per leaf, which is
precisely this library's target workload.

The API must therefore provide, at minimum:

| requirement | source |
|---|---|
| builder with `hash_length` and `personal` | Zebra, all three call sites |
| `to_state()` producing a streaming state | Zebra |
| `update` chainable, `finalize` returning a hash | Zebra |
| a `Hash` type with `as_bytes` / `Into<[u8; N]>` | Zebra, librustzcash |
| `State: Clone` with independent continuation | librustzcash Equihash |
| `salt`, `key` | BLAKE2b completeness |

Anything beyond that list is this library's own addition and must justify
itself separately.

## 4. Design commitments

**Compactness over generality.** One state type. No trait hierarchy, no
generic digest-length parameter, no `Digest` trait implementation in the core
crate. A `RustCrypto` adapter can live behind a feature if a consumer needs it.

**Errors are values where the caller can act, panics where the caller has a
bug.** A geometry that admits no tail is a `Result`; a 65-byte digest length is
a builder-time panic, because it cannot vary at runtime in correct code.

**No dependencies.** The core crate depends on nothing, not even `arrayvec` or
`constant_time_eq`. `#![no_std]` with no `alloc` requirement. Dev-dependencies
for tests and benches are unconstrained.

**Const-generic digest length where it is free.** `Hash<const N: usize>` avoids
a length field and a runtime check at every use, and matches how consumers
actually spell it (`[u8; 32]`).

## 5. Acceptance criteria

Numbered so a milestone can cite them.

**A1 Correctness.** Byte-identical to the BLAKE2 authors' published vectors --
input lengths 0..255, unkeyed and keyed -- and to `blake2b_simd` across digest
lengths 1..64, key lengths 0..64, salt and personalization, and at least 20
update chunkings. The C `uniblake` test suite is the model; its
`tests/vendor/kat_blake2b.h` data is reusable.

**A2 The prefix property.** For a prefix of length `p` and tail of length `t`
with `pending(p) + t <= 128`, a digest costs exactly one compression. Verified
by counting compressions through a test-only instrumented kernel, not inferred
from timing.

**A3 Parity with C uniblake.** Within 10% of the C library on the same machine
and shape: prefix 140 B, digest 50 B, median of 7 reps. C measures ~90 ns/digest
on an Apple M4 Pro. A Rust result outside 99 ns fails this criterion and the
gap must be explained before proceeding.

**A4 Parity with `blake2b_simd`.** Within 10% on the leaf shape it is measured
at here: 86-91 ns/leaf on aarch64. Being slower is permitted only with a
recorded reason.

**A5 No dependencies.** `cargo tree` for the default feature set shows the
crate alone.

**A6 Portability.** `cargo build` and `cargo test` clean on aarch64-apple-darwin
and x86_64-unknown-linux-gnu; `cargo build` clean for
x86_64-pc-windows-gnu and i686-pc-windows-gnu. `#![forbid(unsafe_code)]` holds
in the portable path.

**A7 Lint cleanliness.** `cargo clippy -- -D warnings` and `cargo fmt --check`
pass. `#![deny(missing_docs)]` on the public API.

## 6. Phases

Each phase ends with its tests passing and its criteria measured. No phase
begins before the previous one's criteria are recorded.

| # | delivers | criteria |
|---|---|---|
| P1 | compression function, `State`, `update`, `finalize`, published vectors | A1 (vectors), A5, A7 |
| P2 | `Params` builder: `hash_length`, `key`, `salt`, `personal`; oracle tests against `blake2b_simd` | A1 (full), A6 |
| P3 | prefix layer: `Prefix`, `hash_tail`, geometry check; compression counting | A2 |
| P4 | benchmarks against C uniblake and `blake2b_simd` | A3, A4 |
| P5 | batch entry point `hash_n`, output stride and slicing | A2 at batch scale |

Phases P1-P3 are the library. P4 decides whether P5 is worth building: if
A3 and A4 fail badly and the cause is structural rather than a fixable
mistake, the honest outcome is to record that and stop.

## 7. Coding standards

- `rustfmt` defaults, no overrides. `clippy::pedantic` advisory, not enforced.
- Public items documented with an example that compiles as a doctest.
- No `unwrap` or `expect` outside tests and doctests.
- Module layout mirrors the C library so the two can be read side by side:
  `compress.rs`, `state.rs`, `params.rs`, `prefix.rs`.
- Comments state the durable fact, not the history. Measured evidence that
  justifies a constant stays; the narrative around it does not.
- Test names say what is being asserted, not what is being called.

## 8. Validation method

Three kinds of evidence, in decreasing strength, matching the C library's rule:

1. **Byte agreement with an independent implementation** -- `blake2b_simd` as a
   dev-dependency oracle, across the full parameter space.
2. **Published vectors** -- the BLAKE2 authors' KAT, which needs no second
   implementation and so runs anywhere.
3. **A negative test** -- a deliberately wrong compression function that every
   comparison must reject. A suite that cannot fail proves nothing.

Benchmarks use `criterion` as a dev-dependency, report medians, and state the
machine, toolchain and shape with every figure.
