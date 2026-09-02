//! Cross-language comparison harness.
//!
//! Deliberately mirrors bench/bench_compare.c in the C repository: same
//! geometry, same iteration counts, same median-of-REPS statistic, same
//! reported units. Anything measured here should be comparable to the C
//! number of the same name; if the two harnesses drift, the table they feed
//! stops meaning anything.
use std::thread;
use std::time::Instant;

const PRE: usize = 140; // shared prefix, absorbed once
const OUT: usize = 50; // digest bytes
const REPS: usize = 7;
const THREADS: usize = 2;

/// Leaf points: digests per measured batch.
const LEAF_N: [usize; 3] = [10_000, 100_000, 400_000];
/// Bulk points: message bytes.
const BULK_N: [usize; 4] = [1 << 10, 1 << 14, 1 << 20, 1 << 24];

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// Byte pattern identical to the C harness: pre[i] = i*7+1 (mod 256).
fn prefix() -> Vec<u8> {
    (0..PRE).map(|i| (i.wrapping_mul(7) + 1) as u8).collect()
}

fn main() {
    let pre = prefix();
    let mut sink = 0u8;

    println!("# uniblake-rs comparison harness");
    println!("# prefix={PRE}B digest={OUT}B reps={REPS} threads={THREADS} (median ns/digest)");
    println!(
        "# state: uniblake={}B blake2b_simd={}B",
        core::mem::size_of::<uniblake::State>(),
        core::mem::size_of::<blake2b_simd::State>()
    );

    // Absorb the process-startup transient before any timing. On an idle
    // machine the first measured block runs ~2.5x slow (196 ns/digest against
    // a steady-state 78) and decays over the first few milliseconds.
    //
    // It is an artifact of *position in the run*, not of any implementation:
    // reordering the harness so the reference measures first moves the penalty
    // onto the reference (388 ns, 2.1x) and leaves ours at 114. Whichever
    // block runs first pays it, so without this spin the first column of the
    // table would be silently penalised. A per-loop warmup is not enough --
    // the transient spans the process, not the loop.
    {
        let base = uniblake::Params::new()
            .hash_length(OUT)
            .to_state()
            .update_owned(&pre);
        let t0 = Instant::now();
        while t0.elapsed().as_secs_f64() < 0.3 {
            for _ in 0..1000 {
                let mut s = base.clone();
                s.update(&[0u8; 4]);
                sink ^= s.finalize().as_bytes()[0];
            }
        }
    }

    // ---- leaf: shared prefix, 4-byte counter tail ----
    println!("\n[leaf]");
    println!("n,rust_ours_ns,rust_ours_2t_ns,rust_ref_ns");
    for &n in &LEAF_N {
        // ours, serial. One untimed warmup pass first: at the smallest n the
        // first rep otherwise pays page faults and frequency ramp, which
        // showed up as 196 ns against a steady-state 78 in the C harness.
        {
            let base = uniblake::Params::new()
                .hash_length(OUT)
                .to_state()
                .update_owned(&pre);
            for i in 0..n as u32 {
                let mut s = base.clone();
                s.update(&i.to_le_bytes());
                sink ^= s.finalize().as_bytes()[0];
            }
        }
        let mut ours = Vec::new();
        for _ in 0..REPS {
            let base = uniblake::Params::new()
                .hash_length(OUT)
                .to_state()
                .update_owned(&pre);
            let t0 = Instant::now();
            for i in 0..n as u32 {
                let mut s = base.clone();
                s.update(&i.to_le_bytes());
                sink ^= s.finalize().as_bytes()[0];
            }
            ours.push(t0.elapsed().as_nanos() as f64 / n as f64);
        }

        // ours, THREADS-way split of the same range -- the Rust analogue of
        // backends/hash_n_threads.c: each digest is independent and the
        // prefix state is read-only, so the range splits with no coordination.
        let mut ours_par = Vec::new();
        for _ in 0..REPS {
            let base = uniblake::Params::new()
                .hash_length(OUT)
                .to_state()
                .update_owned(&pre);
            let t0 = Instant::now();
            let chunk = (n + THREADS - 1) / THREADS; // not div_ceil: MSRV is 1.63
            let acc: u8 = thread::scope(|sc| {
                let hs: Vec<_> = (0..THREADS)
                    .map(|t| {
                        let base = base.clone();
                        let lo = t * chunk;
                        let hi = ((t + 1) * chunk).min(n);
                        sc.spawn(move || {
                            let mut local = 0u8;
                            for i in lo..hi {
                                let mut s = base.clone();
                                s.update(&(i as u32).to_le_bytes());
                                local ^= s.finalize().as_bytes()[0];
                            }
                            local
                        })
                    })
                    .collect();
                hs.into_iter()
                    .map(|h| h.join().unwrap())
                    .fold(0, |a, b| a ^ b)
            });
            sink ^= acc;
            ours_par.push(t0.elapsed().as_nanos() as f64 / n as f64);
        }

        // reference: blake2b_simd, same shape
        let mut refr = Vec::new();
        for _ in 0..REPS {
            let mut p = blake2b_simd::Params::new();
            p.hash_length(OUT);
            let base = p.to_state().update(&pre).clone();
            let t0 = Instant::now();
            for i in 0..n as u32 {
                let mut s = base.clone();
                s.update(&i.to_le_bytes());
                sink ^= s.finalize().as_bytes()[0];
            }
            refr.push(t0.elapsed().as_nanos() as f64 / n as f64);
        }

        println!(
            "{n},{:.1},{:.1},{:.1}",
            median(ours),
            median(ours_par),
            median(refr)
        );
    }

    // ---- bulk: one long message, MB/s ----
    println!("\n[bulk]");
    println!("bytes,rust_ours_mbs,rust_ref_mbs");
    for &sz in &BULK_N {
        let data = vec![7u8; sz];
        // Small messages are far below clock resolution for a single hash --
        // 1 KiB reported an implausibly exact 1024 MB/s for both
        // implementations. Repeat until the timed region is >= ~20 ms.
        let iters = ((20 << 20) / sz).max(1);
        let mut ours = Vec::new();
        let mut refr = Vec::new();
        for _ in 0..REPS {
            let t0 = Instant::now();
            for _ in 0..iters {
                let h = uniblake::Params::new()
                    .to_state()
                    .update_owned(&data)
                    .finalize();
                sink ^= h.as_bytes()[0];
            }
            let d = t0.elapsed().as_secs_f64();
            ours.push((sz * iters) as f64 / d / 1e6);

            let t0 = Instant::now();
            for _ in 0..iters {
                let h2 = blake2b_simd::Params::new()
                    .to_state()
                    .update(&data)
                    .finalize();
                sink ^= h2.as_bytes()[0];
            }
            let d2 = t0.elapsed().as_secs_f64();
            refr.push((sz * iters) as f64 / d2 / 1e6);
        }
        println!("{sz},{:.0},{:.0}", median(ours), median(refr));
    }

    let _ = sink;
}
