//! Phase 0.6 spike: OS media controls (SMTC on Windows, MPRIS on Linux) driving libmpv.
//!
//! `cargo run -p abs-desktop --example media_controls --release -- [audio file] [cover file]`
//! Defaults to the first track of the LibriVox fixture. Plays audibly.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use anyhow::{Context, Result};
use libmpv2::Mpv;
#[cfg(windows)]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};

const FIXTURE_DIR: &str = "fixtures/media/audiobooks/Oscar Wilde/The Happy Prince and Other Tales";
const SKIP: Duration = Duration::from_secs(30);

slint::slint! {
    export component ControlsWindow inherits Window {
        title: @tr("Media controls spike");
        preferred-width: 420px;
        preferred-height: 160px;
        in property <string> state;
        in property <string> position;
        callback toggle();
        VerticalLayout {
            padding: 16px;
            spacing: 8px;
            Text { text: root.state; font-size: 16px; }
            Text { text: root.position; }
            TouchArea {
                clicked => { root.toggle(); }
                Rectangle { background: #3b6ea5; height: 36px; border-radius: 4px;
                    Text { text: @tr("Play / pause"); color: white; }
                }
            }
        }
    }
}

struct Spike {
    mpv: Mpv,
    controls: Option<MediaControls>,
    cover_url: String,
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let audio = args.next().map(PathBuf::from).unwrap_or_else(|| {
        Path::new(FIXTURE_DIR).join("thehappyprinceandothertales_01_wilde_64kb.mp3")
    });
    let cover = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(FIXTURE_DIR).join("cover.jpg"));
    let cover_url =
        format!("file://{}", std::fs::canonicalize(&cover)?.display()).replace("\\\\?\\", "");

    let mpv = Mpv::with_initializer(|init| {
        init.set_option("video", "no")?;
        init.set_option("media-controls", "no")?;
        if std::env::var_os("SPIKE_MUTE").is_some() {
            init.set_option("ao", "null")?;
        }
        init.set_option("idle", "yes")
    })
    .map_err(|e| anyhow::anyhow!("mpv init: {e}"))?;
    mpv.command(
        "loadfile",
        &[audio.to_str().context("non-UTF-8 path")?, "replace"],
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    let window = ControlsWindow::new()?;
    let spike = Rc::new(RefCell::new(Spike {
        mpv,
        controls: None,
        cover_url,
    }));
    let toggle_spike = spike.clone();
    window.on_toggle(move || handle_event(&toggle_spike, MediaControlEvent::Toggle));

    let attach_spike = spike.clone();
    let weak = window.as_weak();
    slint::Timer::single_shot(Duration::from_millis(200), move || {
        if let Some(window) = weak.upgrade()
            && let Err(err) = attach_controls(&window, &attach_spike)
        {
            eprintln!("media controls: {err:#}");
        }
    });
    let _ticker = start_ticker(&window, &spike);
    window.run()?;
    Ok(())
}

fn attach_controls(window: &ControlsWindow, spike: &Rc<RefCell<Spike>>) -> Result<()> {
    let config = PlatformConfig {
        display_name: "ABS Desktop",
        dbus_name: "abs_desktop_spike",
        hwnd: native_hwnd(window)?,
    };
    let mut controls = MediaControls::new(config).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let weak = window.as_weak();
    controls
        .attach(move |event| {
            println!("os event: {event:?}");
            let _ = weak.upgrade_in_event_loop(move |_| {
                SPIKE
                    .with(|s| {
                        s.borrow()
                            .as_ref()
                            .map(|spike| handle_event(spike, event.clone()))
                    })
                    .unwrap_or(())
            });
        })
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let cover_url = spike.borrow().cover_url.clone();
    let metadata = MediaMetadata {
        title: Some("The Happy Prince"),
        album: Some("The Happy Prince and Other Tales"),
        artist: Some("Oscar Wilde"),
        cover_url: Some(&cover_url),
        duration: Some(Duration::from_secs(24 * 60)),
    };
    controls
        .set_metadata(metadata)
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    spike.borrow_mut().controls = Some(controls);
    SPIKE.with(|s| *s.borrow_mut() = Some(spike.clone()));
    println!("media controls attached, cover {cover_url}");
    Ok(())
}

thread_local! {
    static SPIKE: RefCell<Option<Rc<RefCell<Spike>>>> = const { RefCell::new(None) };
}

#[cfg(windows)]
fn native_hwnd(window: &ControlsWindow) -> Result<Option<*mut std::ffi::c_void>> {
    let handle = window.window().window_handle();
    match handle
        .window_handle()
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .as_raw()
    {
        RawWindowHandle::Win32(h) => Ok(Some(h.hwnd.get() as *mut std::ffi::c_void)),
        other => anyhow::bail!("unexpected window handle {other:?}"),
    }
}

#[cfg(not(windows))]
fn native_hwnd(_window: &ControlsWindow) -> Result<Option<*mut std::ffi::c_void>> {
    Ok(None)
}

fn handle_event(spike: &Rc<RefCell<Spike>>, event: MediaControlEvent) {
    let spike = spike.borrow();
    let paused: bool = spike.mpv.get_property("pause").unwrap_or(false);
    let result = match event {
        MediaControlEvent::Toggle => spike.mpv.set_property("pause", !paused),
        MediaControlEvent::Play => spike.mpv.set_property("pause", false),
        MediaControlEvent::Pause | MediaControlEvent::Stop => spike.mpv.set_property("pause", true),
        MediaControlEvent::Seek(direction) | MediaControlEvent::SeekBy(direction, _) => {
            seek(&spike.mpv, direction)
        }
        MediaControlEvent::SetPosition(MediaPosition(at)) => spike
            .mpv
            .command("seek", &[&at.as_secs_f64().to_string(), "absolute"]),
        _ => Ok(()),
    };
    if let Err(err) = result {
        eprintln!("mpv: {err}");
    }
}

fn seek(mpv: &Mpv, direction: SeekDirection) -> libmpv2::Result<()> {
    let secs = match direction {
        SeekDirection::Forward => SKIP.as_secs_f64(),
        SeekDirection::Backward => -SKIP.as_secs_f64(),
    };
    mpv.command("seek", &[&secs.to_string(), "relative"])
}

fn start_ticker(window: &ControlsWindow, spike: &Rc<RefCell<Spike>>) -> slint::Timer {
    let (weak, spike) = (window.as_weak(), spike.clone());
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_secs(1),
        move || {
            let Some(window) = weak.upgrade() else { return };
            let mut spike = spike.borrow_mut();
            while spike.mpv.wait_event(0.0).is_some() {}
            let paused: bool = spike.mpv.get_property("pause").unwrap_or(false);
            let position = Duration::from_secs_f64(
                spike
                    .mpv
                    .get_property::<f64>("time-pos")
                    .unwrap_or_default()
                    .max(0.0),
            );
            window.set_state(if paused {
                "Paused".into()
            } else {
                "Playing".into()
            });
            window.set_position(
                format!(
                    "{:02}:{:02}",
                    position.as_secs() / 60,
                    position.as_secs() % 60
                )
                .into(),
            );
            let progress = Some(MediaPosition(position));
            let playback = if paused {
                MediaPlayback::Paused { progress }
            } else {
                MediaPlayback::Playing { progress }
            };
            if let Some(controls) = spike.controls.as_mut() {
                let _ = controls.set_playback(playback);
            }
        },
    );
    timer
}
