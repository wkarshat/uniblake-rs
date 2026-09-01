use std::time::Instant;
fn main() {
    let data = vec![7u8; 1 << 20];
    let (mut bu, mut bs) = (f64::MAX, f64::MAX);
    let mut sink = 0u8;
    for _ in 0..9 {
        let t = Instant::now();
        let h = uniblake::Params::new()
            .to_state()
            .update_owned(&data)
            .finalize();
        let d = t.elapsed().as_secs_f64();
        sink ^= h.as_bytes()[0];
        if d < bu {
            bu = d;
        }
        let t = Instant::now();
        let h2 = blake2b_simd::Params::new()
            .to_state()
            .update(&data)
            .finalize();
        let d2 = t.elapsed().as_secs_f64();
        sink ^= h2.as_bytes()[0];
        if d2 < bs {
            bs = d2;
        }
    }
    let mb = 1.048576;
    println!(
        "bulk 1 MiB  uniblake {:.0} MB/s   blake2b_simd {:.0} MB/s   ratio {:.2}x",
        mb / bu,
        mb / bs,
        bu / bs
    );
    let _ = sink;
}
