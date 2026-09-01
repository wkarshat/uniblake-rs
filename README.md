# uniblake-rs

BLAKE2b in Rust with eager absorption specified as interface behaviour, and a
shared-prefix fast path built on it. Zero dependencies, `no_std`,
`#![forbid(unsafe_code)]`.

Status: **M1-M4 complete, M5 not started.** See `docs/DESIGN.md` for goals,
acceptance criteria and milestones.

```rust
let hash = uniblake::Params::new()
    .hash_length(32)
    .personal(b"my-app-v1")
    .to_state()
    .update_owned(b"data")
    .finalize();
```

The repeated case, which is why this exists:

```rust
let base = uniblake::Params::new().hash_length(32).to_state()
    .update_owned(&prefix);
let a = base.clone().update_owned(&0u32.to_le_bytes()).finalize();
let b = base.clone().update_owned(&1u32.to_le_bytes()).finalize();
```

The prefix is absorbed once and each digest costs one compression.
`State::prefix_check(tail_len)` answers, before any hashing, whether the sizes
permit that.

## Naming

The package is `uniblake-rs`, matching the repository and distinguishing it
from the C library it was ported from. The library target is `uniblake`, so
callers write `uniblake::Params` rather than `uniblake_rs::Params`:

```toml
[dependencies]
uniblake-rs = "0.1"
```

```rust
use uniblake::Params;
```

## Build and test

Needs a Rust toolchain; nothing else. The crate itself has no dependencies,
and `blake2b_simd` is pulled in only as a dev-dependency oracle.

```
cargo test                       # 17 tests + 4 doctests
cargo clippy --all-targets       # must be warning-free
cargo run --release --example bench   # leaf shape, vs blake2b_simd
cargo run --release --example bulk    # 1 MiB throughput
```

On a fresh Linux host:

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
cd uniblake-rs && cargo test && cargo run --release --example bench
```

### Comparing against the C library on Linux

The two are separate builds and are compared by running each benchmark on the
same machine, back to back. They use the same shape -- 140-byte prefix, 4-byte
counter tail, 50-byte digest, median of 7 reps -- so the per-digest figures are
directly comparable.

```
# C: needs libsodium only for its oracle-based suites, not for the benchmark
sudo apt install build-essential libsodium-dev
cd ../uniblake && make && make bench SODIUM=/usr
#   -> "uniblake streaming   NN.N"

cd ../uniblake-rs && cargo run --release --example bench
#   -> "uniblake             NN.N"
```

Two cautions, both learned by getting them wrong here:

- **Interleave the runs.** Thermal and scheduling drift between invocations
  moved a bulk figure by 40% on this machine, enough to invert a comparison.
  Run four alternating pairs and compare ranges, not single samples.
- **Report medians and the spread.** A single sample of the leaf shape looked
  like a 4 ns difference in one direction; fifteen reps showed 3 ns in the
  other.

`make bench` also prints a libsodium row, which gives a common third reference
on any host where both are measured.

## Verified

Byte-identical to `blake2b_simd` across message lengths 0-600, digest lengths
1-64, key lengths 1-64, salt and personalization 1-16, thirteen update
chunkings, and the clone/prefix path.

The prefix property is verified by counting compressions, not inferred from
timing: a 140-byte prefix leaves 12 bytes pending and every tail up to 116
bytes costs exactly one compression, while 117 costs two and `prefix_check`
says so in advance.

Thirteen integration tests, four doctests. Zero dependencies (`cargo tree`),
clippy and rustfmt clean.

## Measured

Apple M4 Pro, rustc 1.90, `--release`. `blake2b_simd` runs its *portable* path
on aarch64 -- it has no NEON kernel -- so this is scalar against scalar.

| shape | uniblake-rs | blake2b_simd | ratio |
|---|--:|--:|--:|
| bulk, 1 MiB | 950 MB/s | 1025 MB/s | 1.01x |
| leaf: 140 B prefix, 4 B tail, 50 B digest | 82.9 ns | 79.3 ns | **1.05x** |

Both acceptance criteria pass: A3 against the C library at ~90 ns, and A4
within 10% of `blake2b_simd`.

The leaf figure was 98.0 ns at first. Three things moved it, each measured
rather than assumed:

| change | effect |
|---|---|
| unroll the twelve rounds | 98.0 -> 89.5 |
| load message words via `chunks_exact` | 89.5 -> 87.9 |
| use `clone()` + `update()` instead of `update_owned` per digest | 87.9 -> 82.9 |

The first is the *opposite* of the C result, where unrolling cost 27% to
register spilling; rustc allocates the two forms differently.

The third was a mistake in this crate's own API use, not codegen. Splitting the
leaf cost showed `clone + update` at 16.4 ns against `blake2b_simd`'s 4.9,
while `finalize` was within 4 ns. `update_owned` takes and returns `Self`, so
it moves the 224-byte state; `clone()` then `update()` in place is 4.8 ns.
`update_owned` is kept for building a prefix state once and is documented as
wrong for the inner loop.

Four other attempts moved nothing and are noted here so they are not retried:
`#[inline(always)]` on `compress`, building the working vector as a literal,
a mask instead of a branch for the finalization flag, and padding into a fresh
block rather than copying and zeroing. They remain in the code because they are
no worse and arguably clearer.

Reproduce with `cargo run --release --example bench` and `--example bulk`.

## Memory

| | uniblake-rs | blake2b_simd |
|---|--:|--:|
| `State` | 224 B | 224 B |
| `Params` | 34 B | 184 B |
| `Hash` | 65 B | 65 B |

`State` is the one that matters: it is cloned once per digest in the prefix
workload, and the two are the same size. `Hash` was 72 bytes until its length
field was narrowed from `usize` to `u8`, which removed the 8-byte alignment and
the padding it forced. `Params` holds only what the parameter block needs: two lengths, salt and
personalization. Key *material* is passed to `to_state_keyed` rather than
stored, because BLAKE2b absorbs the key as a padded first block and never
mixes it into the parameter block -- so keeping a 64-byte buffer in every
`Params` charged every caller for a path that no Zcash consumer uses. Tree
fields are out of scope. For comparison the C `ub_param` is 52 bytes: it also
stores no key, but does carry the tree fields. No allocation anywhere in the crate;
process RSS for the benchmark is 1.5 MB against 1.4 MB for an empty Rust
program.

## Portability

| target | data model | result |
|---|---|---|
| aarch64-apple-darwin | LP64 | all tests pass |
| x86_64-apple-darwin | LP64 | all tests pass |
| x86_64-pc-windows-gnu | LLP64 | builds |
| i686-pc-windows-gnu | ILP32 | builds |
| thumbv7em-none-eabi | ILP32, bare metal | builds |

`thumbv7em-none-eabi` has no `libstd` at all, so the `no_std` claim is tested
rather than asserted.

`State` is **224 bytes on every target**, checked by a `const` assertion that
fails the build if it is not -- so a state sized in one build is the right size
in another. Alignment is not invariant: i686 aligns `u128` to 4 where the LP64
targets use 8. That is legal and does not affect size, but a caller
serializing a state across targets should not assume padding.

## Not yet done

M5, the batch entry point, is not started. It is worth building only if a
consumer wants it: no Zcash-lineage crate uses `blake2b_simd::many::hash_many`,
and our own prior measurement found that API 3x *slower* per leaf on aarch64.
