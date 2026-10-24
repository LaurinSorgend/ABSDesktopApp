//! Loads libmpv and optionally plays one URL, for baseline memory measurements.
//!
//! `mpv_idle [url] [bearer-token]`; extra options as `MPV_OPTS=key=value,key=value`.

use libmpv2::Mpv;

fn main() {
    let mut args = std::env::args().skip(1);
    let url = args.next();
    let header = args
        .next()
        .map(|token| format!("Authorization: Bearer {token}"));
    let mpv = Mpv::with_initializer(|init| {
        init.set_option("video", "no")?;
        init.set_option("ao", "null")?;
        if let Some(header) = &header {
            init.set_option("http-header-fields", header.as_str())?;
        }
        for (key, value) in extra_options() {
            init.set_option(&key, value.as_str())?;
        }
        init.set_option("idle", "yes")
    })
    .expect("mpv init");
    if let Some(url) = &url {
        mpv.command("loadfile", &[url, "replace"])
            .expect("loadfile");
    }
    println!("pid {}", std::process::id());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
    while std::time::Instant::now() < deadline {
        let _ = mpv.wait_event(0.1);
    }
}

fn extra_options() -> Vec<(String, String)> {
    let opts = std::env::var("MPV_OPTS").unwrap_or_default();
    opts.split(',')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}
