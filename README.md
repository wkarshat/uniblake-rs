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

### Cross-language comparison

Six implementations on one machine, one shape, one methodology. Both harnesses
compute byte-identical digests (verified before timing), use the same geometry,
the same iteration counts and the same median-of-7 statistic, so the columns
are directly comparable.

**System and configuration.** Apple M4 Pro (14 cores: 10 performance,
4 efficiency), 48 GiB, macOS 26.3 (build 25D125), arm64.
C: Apple clang 21.0.0, `-O2 -std=c11`, libsodium 1.0.22 as the reference.
Rust: rustc 1.98.0 (2026-08-18), `--release` (`opt-level=3`, LTO off),
`blake2b_simd` 1.0.5 as the reference.
Geometry: 140-byte shared prefix absorbed once, 4-byte counter tail, 50-byte
digest, median of 7 reps. The two harnesses are run **interleaved**,
alternating C and Rust, and the first run of each is discarded; the figures
below are medians of the four remaining. Run-to-run spread at n=400 000 is
under 0.6 ns for every uniblake column. Parallel columns split the digest range across
**2 threads**; each digest is independent and the prefix state is read-only,
so the split needs no coordination.

Both references run a *portable scalar* path on aarch64: libsodium's NEON
dispatch is x86-only for BLAKE2b, and `blake2b_simd` has no NEON kernel. This
is scalar against scalar throughout; no SIMD kernel is involved in any column.

**State size.** All four hold the same BLAKE2b working set; the differences
are accounted for field by field (LP64/aarch64):

| | uniblake C | libsodium | uniblake-rs | blake2b_simd |
|---|--:|--:|--:|--:|
| chaining `h[8]` | 64 | 64 | 64 | 64 |
| counter `t` | 16 | 16 | 16 | 16 |
| flags `f[2]` | 16 | 16 | — | — |
| buffer | 128 | **256** | 128 | 128 |
| lengths/flags | 2 | 9 | 2 | 5 |
| payload | 226 | 361 | 210 | 213 |
| padding | 6 | 23 | 6 | 11 |
| **total** | **232** | **384** | **216** | **224** |

Each difference has a cause, not a preference:

- **libsodium is 384** because its buffer is `buf[2 * 128]` — it holds *two*
  blocks, deferring compression so the last block can be handled as final, and
  its `buflen` is a `size_t`. The internal struct is 361 bytes; the public
  `crypto_generichash_blake2b_state` rounds that to a 384-byte opaque array
  aligned to 64. That extra 128-byte buffer is copied on every state copy,
  which is what the leaf column measures.
- **uniblake-rs is the smallest at 216** because it carries no `f[2]` flag
  words: finalization passes `last` as an argument to `compress` rather than
  storing it in the state. That is 16 bytes off the C layout.
- **blake2b_simd is 224** because `Count = u128`, whose 16-byte alignment pads
  the tail. Its payload (213) is *larger* than ours only by the four extra
  configuration bytes it keeps (`last_node`, `hash_length`, `implementation`,
  `is_keyed`). Our crate stored a `u128` too until it was measured varying by
  target — see *ABI invariance* — so this is the same trade made differently.

**Leaf shape** — ns/digest, lower is better. `n` is digests per measured batch:

| n | C scalar | C 2 threads | C reference | Rust scalar | Rust 2 threads | Rust reference |
|--:|--:|--:|--:|--:|--:|--:|
| 10 000 | 78.1 | 43.8 | 180.4 | 75.1 | 41.4 | 79.5 |
| 100 000 | 78.4 | 40.4 | 185.5 | 75.4 | 38.1 | 79.4 |
| 400 000 | 78.7 | 40.4 | 186.2 | 74.8 | 38.2 | 79.8 |

**Bulk** — MB/s on one long message, higher is better:

| message | C scalar | C reference | Rust scalar | Rust reference |
|--:|--:|--:|--:|--:|
| 1 KiB | 1704 | 1502 | 1745 | 1728 |
| 16 KiB | 1723 | 1546 | 1751 | 1749 |
| 1 MiB | 1729 | 1541 | 1747 | 1754 |
| 16 MiB | 1720 | 1541 | 1750 | 1752 |

What the table shows:

- **Both implementations beat their own reference on the leaf shape**, which is
  the point of the library: C is 2.35x libsodium, Rust 1.07x `blake2b_simd`.
  The margins differ because the references differ — libsodium re-reads and
  re-initialises more per digest, while `blake2b_simd` is already a tuned
  scalar implementation.
- **The two implementations are within ~5% of each other** (74.75 vs 78.70 ns
  interleaved medians), with Rust ahead. The gap is *not* in the compression
  function. All four kernels were disassembled and counted rather than read
  from source:

  | | instrs | rotates/block | sigma `ldrb` | spill | mem | branches | frame |
  |---|--:|--:|--:|--:|--:|--:|--:|
  | uniblake C | 1516 | 380 | 0 | 104 | 131 | 0 | 304 |
  | uniblake-rs | 1546 | 380 | 0 | 107 | 127 | 0 | 304 |
  | libsodium | 1565 | 384 | 0 | 133 | 155 | 3 | 368 |
  | blake2b_simd | 3155 | 380 | 1 | 182 | 250 | 10 | 432 |

  **All four are fully unrolled**, verified three ways: 12 rounds x 8 G x 4
  rotates = 384 rotate sites, zero `ldrb` (so every sigma index is a
  compile-time constant, not a runtime table read), and no loop branches in
  either uniblake body. libsodium reaches this from a *rolled* `ROUND(r)`
  source — clang unrolls it — so source shape does not predict codegen, which
  is why this was counted rather than assumed. `blake2b_simd`'s 3155
  instructions and 760 rotates are two block bodies: its `compress1_loop`
  is 2x-unrolled over blocks, so per block it matches at 380.

  **The 380-vs-384 difference is not elision.** Both uniblake bodies show 96
  each of `ror #16/#24/#32` but only 92 of `ror #63`. The missing four are the
  final round's diagonal `G2` rotates, which aarch64 folds into the output XOR
  as a free shifted operand — `eor x9, x9, x17, ror #63`. The work happens; it
  costs no instruction. libsodium keeps 96 discrete rotates because its output
  loop is not fused the same way.

  So no implementation skips the last round's redundant half, and none of the
  four exploits the fact that round 12's diagonal results feed only the final
  XOR. That optimisation is available in all four and taken by none.

  Locating the gap by phase instead:

  | phase | C | Rust |
  |---|--:|--:|
  | clone + update | 8.62 ns | 7.05 ns |
  | finalize | 73.00 ns | 70.34 ns |

  The clone half tracks state size — 232 bytes copied against 216. The rest is
  finalization overhead around an equivalent kernel, not the kernel itself.

- **Two threads give ~1.95x on both**, close to linear, as an embarrassingly
  parallel range should. The C parallel column is slightly ahead in absolute
  terms despite the slower serial figure.
- **Bulk is a near-tie** (~1720-1750 MB/s) across four orders of magnitude of
  message size. At bulk the per-digest setup that the prefix path optimises is
  amortised away, so all four converge; only the C reference trails, at ~1540.

Reproduce with `cargo run --release --example compare` here and
`make bench-compare SODIUM=<prefix>` in the C repository. Both print the
configuration header shown above.

Two cautions, both learned by getting them wrong here:

- **Absorb the startup transient, and check it by reordering.** On an idle
  machine the first measured block ran ~2.5x slow (196 ns/digest against a
  steady-state 78). It is positional, not a property of the implementation:
  reordering the harness so the reference measures first moved the penalty onto
  the reference (388 ns) and left ours at 114. Whichever block runs first pays
  it. Both harnesses now spin for 300 ms before timing; a per-loop warmup is
  not enough, since the transient spans the process rather than the loop.
- **Check the timer against the work.** A single 1 KiB hash is below clock
  resolution and reported an implausibly exact 1024 MB/s for *both*
  implementations. The harnesses now repeat small messages until the timed
  region is at least 20 ms.

### Single-implementation history

Apple M4 Pro, rustc 1.98, `--release`.

| shape | uniblake-rs | blake2b_simd | ratio |
|---|--:|--:|--:|
| bulk, 1 MiB | 1747 MB/s | 1754 MB/s | 1.00x |
| leaf: 140 B prefix, 4 B tail, 50 B digest | 74.5 ns | 79.6 ns | **0.94x** |

Both acceptance criteria pass: A3 against the C library, now 78.5 ns on this
machine, and A4 within 10% of `blake2b_simd` -- currently ahead of it.

The leaf figure was 98.0 ns at first. Three things moved it, each measured
rather than assumed:

| change | effect |
|---|---|
| unroll the twelve rounds | 98.0 -> 89.5 |
| load message words via `chunks_exact` | 89.5 -> 87.9 |
| use `clone()` + `update()` instead of `update_owned` per digest | 87.9 -> 82.9 |

Measured again on rustc 1.98 the same shape is 74.5 ns; the 82.9 above was
rustc 1.90. The ordering of the three changes is unaffected.

The first is the *opposite* of the C result, where unrolling cost 27% to
register spilling; rustc allocates the two forms differently.

The third was a mistake in this crate's own API use, not codegen. Splitting the
leaf cost showed `clone + update` at 16.4 ns against `blake2b_simd`'s 4.9,
while `finalize` was within 4 ns. `update_owned` takes and returns `Self`, so
it moves the 216-byte state; `clone()` then `update()` in place is 4.8 ns.
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
