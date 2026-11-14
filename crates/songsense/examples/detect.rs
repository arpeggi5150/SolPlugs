//! Prints Song Sense's reading of WAV files: `cargo run -p solplugs-songsense --example detect -- a.wav b.wav`.
fn main() {
    for p in std::env::args().skip(1) {
        let Ok(mut r) = hound::WavReader::open(&p) else {
            println!("{p}: can't read");
            continue;
        };
        let spec = r.spec();
        let ch = usize::from(spec.channels.max(1));
        let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(8, 32) - 1)) as f32;
        let all: Vec<f32> = r.samples::<i32>().filter_map(Result::ok).map(|v| v as f32 * scale).collect();
        let mono: Vec<f32> = all.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
        println!("{p}: {:?}", solplugs_songsense::detect(&mono, f64::from(spec.sample_rate)));
    }
}
