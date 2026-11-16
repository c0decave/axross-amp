//! How fast each file decodes (as the player does it: resampled to 48 kHz,
//! stereo): `cargo run --release --example decode_speed -- FILE ...`.
fn main() {
    for path in std::env::args().skip(1) {
        let started = std::time::Instant::now();
        match axross_amp::engine::decode_file(&path, 48_000) {
            Ok((frames, info)) => {
                let secs = frames as f64 / 48_000.0;
                let took = started.elapsed().as_secs_f64();
                println!(
                    "{:<28} {:>6} {:>6.2} s audio in {:>7.4} s: {:>6.0}x real time",
                    path.rsplit('/').next().unwrap_or(&path),
                    info.codec,
                    secs,
                    took,
                    secs / took
                );
            }
            Err(e) => println!("{path}: {e}"),
        }
    }
}
