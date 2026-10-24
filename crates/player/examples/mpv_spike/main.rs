//! Phase 0.5 spike: libmpv direct play from Audiobookshelf.
//!
//! `cargo run -p abs-player --example mpv_spike --release -- [url] [username] [password]`
//! Set `SPIKE_AUDIBLE=1` to hear the output, `SPIKE_HOLD_SECS=n` to keep playing for memory sampling.

mod session;

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use libmpv2::Mpv;
use libmpv2::events::Event;

use session::{Server, Session, locate};

const MULTI_FILE_TITLE: &str = "The Happy Prince and Other Tales";
const LONG_TITLE: &str = "The Happy Prince Marathon";
const EVENT_TIMEOUT: Duration = Duration::from_secs(30);
const SPEEDS: &[f64] = &[0.75, 1.0, 1.5, 2.0, 3.0];
const LEAN_OPTIONS: &[&str] = &[
    "load-scripts",
    "ytdl",
    "osc",
    "input-default-bindings",
    "load-stats-overlay",
    "load-console",
    "load-auto-profiles",
    "load-select",
    "load-positioning",
    "load-commands",
    "media-controls",
];
const MPV_END_FILE_REASON_ERROR: u32 = 4;
const SEEK_POINTS: &[f64] = &[0.10, 0.50, 0.95];

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let base = args
        .next()
        .unwrap_or_else(|| "http://localhost:13379".into());
    let username = args.next().unwrap_or_else(|| "admin".into());
    let password = args.next().unwrap_or_else(|| "admin-pass".into());
    let server = Server::login(&base, &username, &password)?;
    let log_file = std::env::temp_dir().join("mpv_spike.log");
    let mpv = create_player(&server.token, &log_file)?;
    println!("pid {}, mpv log {}", std::process::id(), log_file.display());

    let multi = server.start_session(&server.find_item(MULTI_FILE_TITLE)?)?;
    probe_multi_file(&mpv, &multi)?;
    server.close_session(&multi.id)?;

    let long = server.start_session(&server.find_item(LONG_TITLE)?)?;
    probe_seek_latency(&mpv, &long)?;
    probe_speeds(&mpv)?;
    hold_for_sampling(&mpv)?;
    server.close_session(&long.id)?;
    report_pitch_filter(&log_file)?;
    Ok(())
}

fn create_player(token: &str, log_file: &std::path::Path) -> Result<Mpv> {
    let audible = std::env::var_os("SPIKE_AUDIBLE").is_some();
    let header = format!("Authorization: Bearer {token}");
    let log = log_file.to_string_lossy().into_owned();
    Mpv::with_initializer(|init| {
        init.set_option("video", "no")?;
        init.set_option("ao", if audible { "wasapi" } else { "null" })?;
        init.set_option("idle", "yes")?;
        init.set_option("http-header-fields", header.as_str())?;
        init.set_option("cache", "yes")?;
        init.set_option("demuxer-max-bytes", "8MiB")?;
        init.set_option("demuxer-max-back-bytes", "2MiB")?;
        init.set_option("audio-pitch-correction", "yes")?;
        init.set_option("log-file", log.as_str())?;
        init.set_option("msg-level", "all=v")?;
        if std::env::var_os("SPIKE_FULL").is_none() {
            for option in LEAN_OPTIONS {
                init.set_option(option, "no")?;
            }
        }
        Ok(())
    })
    .map_err(|e| anyhow::anyhow!("mpv init: {e}"))
}

fn probe_multi_file(mpv: &Mpv, session: &Session) -> Result<()> {
    println!("\n== multi-file book: {} tracks", session.tracks.len());
    for (i, track) in session.tracks.iter().enumerate() {
        let mode = if i == 0 { "replace" } else { "append" };
        mpv_cmd(mpv, "loadfile", &[&track.url, mode])?;
    }
    wait_for(mpv, |e| matches!(e, Event::PlaybackRestart))?;
    let total: f64 = session.tracks.iter().map(|t| t.duration).sum();
    for global in [total * 0.05, total * 0.45, total * 0.9] {
        seek_global(mpv, session, global)?;
        let pos: i64 = mpv.get_property("playlist-pos").map_err(mpv_err)?;
        let time: f64 = mpv.get_property("time-pos").map_err(mpv_err)?;
        let landed = session.tracks[pos as usize].start_offset + time;
        println!("seek to {global:8.1}s -> track {pos}, offset {time:7.1}s, global {landed:8.1}s");
    }
    Ok(())
}

fn seek_global(mpv: &Mpv, session: &Session, global: f64) -> Result<()> {
    let (index, offset) = locate(&session.tracks, global);
    let current: i64 = mpv.get_property("playlist-pos").map_err(mpv_err)?;
    if current != index as i64 {
        mpv.set_property("playlist-pos", index as i64)
            .map_err(mpv_err)?;
        wait_for(mpv, |e| matches!(e, Event::PlaybackRestart))?;
    }
    mpv_cmd(mpv, "seek", &[&offset.to_string(), "absolute"])?;
    wait_for(mpv, |e| matches!(e, Event::PlaybackRestart))
}

fn probe_seek_latency(mpv: &Mpv, session: &Session) -> Result<()> {
    let track = session.tracks.first().context("long book has no tracks")?;
    println!("\n== 40 h M4B over HTTP, duration {:.0}s", track.duration);
    let start = Instant::now();
    mpv_cmd(mpv, "loadfile", &[&track.url, "replace"])?;
    wait_for(mpv, |e| matches!(e, Event::PlaybackRestart))?;
    println!("open to first audio: {:?}", start.elapsed());
    for fraction in SEEK_POINTS {
        let target = track.duration * fraction;
        let start = Instant::now();
        mpv_cmd(mpv, "seek", &[&target.to_string(), "absolute"])?;
        wait_for(mpv, |e| matches!(e, Event::PlaybackRestart))?;
        let time: f64 = mpv.get_property("time-pos").map_err(mpv_err)?;
        println!(
            "seek {:>3.0}% -> {time:9.1}s in {:?}",
            fraction * 100.0,
            start.elapsed()
        );
    }
    Ok(())
}

fn probe_speeds(mpv: &Mpv) -> Result<()> {
    println!("\n== speed");
    for speed in SPEEDS {
        mpv.set_property("speed", *speed).map_err(mpv_err)?;
        let before: f64 = mpv.get_property("time-pos").map_err(mpv_err)?;
        pump_events(mpv, Duration::from_secs(2));
        let after: f64 = mpv.get_property("time-pos").map_err(mpv_err)?;
        println!(
            "speed {speed:4.2}: {:.2}s of audio in 2 s wall time",
            after - before
        );
    }
    mpv.set_property("speed", 1.0).map_err(mpv_err)
}

fn hold_for_sampling(mpv: &Mpv) -> Result<()> {
    let secs: u64 = std::env::var("SPIKE_HOLD_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if secs == 0 {
        return Ok(());
    }
    println!("\n== playing for {secs}s for memory sampling");
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        pump_events(mpv, Duration::from_secs(5));
        let cache: i64 = mpv.get_property("cache-buffering-state").unwrap_or(-1);
        println!(
            "time-pos {:.0}s, cache-buffering-state {cache}, demuxer-cache-duration {:.0}s",
            mpv.get_property::<f64>("time-pos").unwrap_or_default(),
            mpv.get_property::<f64>("demuxer-cache-duration")
                .unwrap_or_default()
        );
    }
    Ok(())
}

fn report_pitch_filter(log_file: &std::path::Path) -> Result<()> {
    let log = std::fs::read_to_string(log_file)?;
    let lines: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("scaletempo"))
        .take(3)
        .collect();
    println!("\n== pitch correction filter lines in mpv log:");
    for line in lines {
        println!("  {line}");
    }
    Ok(())
}

fn wait_for(mpv: &Mpv, wanted: impl Fn(&Event) -> bool) -> Result<()> {
    let deadline = Instant::now() + EVENT_TIMEOUT;
    while Instant::now() < deadline {
        match mpv.wait_event(0.1) {
            Some(Ok(Event::EndFile(MPV_END_FILE_REASON_ERROR))) => {
                anyhow::bail!("playback ended with an error")
            }
            Some(Ok(event)) if wanted(&event) => return Ok(()),
            Some(Err(err)) => anyhow::bail!("mpv event error: {err}"),
            _ => {}
        }
    }
    anyhow::bail!("timed out waiting for mpv event")
}

fn pump_events(mpv: &Mpv, duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        let _ = mpv.wait_event(0.1);
    }
}

fn mpv_cmd(mpv: &Mpv, name: &str, args: &[&str]) -> Result<()> {
    mpv.command(name, args).map_err(mpv_err)
}

fn mpv_err(err: libmpv2::Error) -> anyhow::Error {
    anyhow::anyhow!("mpv: {err}")
}
