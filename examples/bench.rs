//! Leaf shape: 140-byte prefix absorbed once, 4-byte counter tail, 50-byte
//! digest. Median of 7 reps x 200k. Same shape the C library benchmarks.
use std::time::Instant;

const PRE: usize = 140;
const OUT: usize = 50;
const N: usize = 200_000;
const REPS: usize = 7;

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn main() {
    let prefix = [7u8; PRE];
    let mut sink = 0u8;

    let mut ours = Vec::new();
    for _ in 0..REPS {
        let base = uniblake::Params::new()
            .hash_length(OUT)
            .to_state()
            .update_owned(&prefix);
        let t0 = Instant::now();
        for i in 0..N as u32 {
            let mut s = base.clone();
            s.update(&i.to_le_bytes());
            sink ^= s.finalize().as_bytes()[0];
        }
        ours.push(t0.elapsed().as_nanos() as f64 / N as f64);
    }

    let mut simd = Vec::new();
    for _ in 0..REPS {
        let mut p = blake2b_simd::Params::new();
        p.hash_length(OUT);
        let base = p.to_state().update(&prefix).clone();
        let t0 = Instant::now();
        for i in 0..N as u32 {
            let mut s = base.clone();
            s.update(&i.to_le_bytes());
            sink ^= s.finalize().as_bytes()[0];
        }
        simd.push(t0.elapsed().as_nanos() as f64 / N as f64);
    }

    let u = median(ours);
    let s = median(simd);
    println!("prefix={PRE}B digest={OUT}B N={N} reps={REPS} (median ns/digest)\n");
    println!("  uniblake        {u:7.1}");
    println!("  blake2b_simd    {s:7.1}   ratio {:.2}x", u / s);
    let _ = sink;
}
