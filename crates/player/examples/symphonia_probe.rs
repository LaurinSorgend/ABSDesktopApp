//! Measures the symphonia fallback on a long M4B: open time, seek time and memory.
//!
//! `cargo run -p abs-player --example symphonia_probe --release -- <file.m4b> [seek secs] [hold secs]`

use std::fs::File;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .context("usage: symphonia_probe <file> [seek] [hold]")?;
    let seek: f64 = args.next().map_or(Ok(72_000.0), |s| s.parse())?;
    let hold: u64 = args.next().map_or(Ok(0), |s| s.parse())?;
    println!("pid {}", std::process::id());

    let start = Instant::now();
    let source = MediaSourceStream::new(Box::new(File::open(&path)?), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("m4b");
    let probed = symphonia::default::get_probe().format(
        &hint,
        source,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;
    let track = format.default_track().context("no audio track")?.clone();
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &Default::default())?;
    println!("open: {:?}", start.elapsed());

    let start = Instant::now();
    let to = SeekTo::Time {
        time: Time::from(seek),
        track_id: Some(track.id),
    };
    let seeked = format.seek(SeekMode::Accurate, to)?;
    println!(
        "seek to {seek}s: {:?} (ts {})",
        start.elapsed(),
        seeked.actual_ts
    );

    let start = Instant::now();
    let (mut frames, mut errors) = (0, 0);
    while start.elapsed() < Duration::from_secs(2) {
        let packet = format.next_packet()?;
        match decoder.decode(&packet) {
            Ok(audio) => frames += audio.frames(),
            Err(e) if errors == 0 => {
                println!("decode error: {e}");
                errors += 1;
            }
            Err(_) => errors += 1,
        }
    }
    println!("decoded {frames} frames, {errors} decode errors in 2 s");
    std::thread::sleep(Duration::from_secs(hold));
    Ok(())
}
