//! Connects the `Ui` callbacks to [`App`] methods.

use slint::ComponentHandle;

use crate::app::{Action, App, with_app};
use crate::{AppWindow, Ui};

/// Runs `f` on the app for a callback; the argument is moved in.
fn on<T: 'static>(f: impl Fn(&mut App, T) + Copy + 'static) -> impl Fn(T) + 'static {
    move |value| with_app(move |app| f(app, value))
}

pub fn connect(window: &AppWindow) {
    let ui = window.global::<Ui>();
    connect_navigation(&ui);
    connect_browse(&ui);
    connect_player(&ui);
    connect_downloads(&ui);
    connect_accounts(&ui);
    connect_works(&ui);
}

fn connect_works(ui: &Ui<'_>) {
    ui.on_set_works_merged(on(|app, merged: bool| app.set_works_merged(merged)));
    ui.on_toggle_works_library(on(|app, index: i32| {
        app.toggle_works_library(index.max(0) as usize)
    }));
    ui.on_set_works_sort(on(|app, index: i32| {
        app.set_works_sort(index.max(0) as usize)
    }));
    ui.on_toggle_works_desc(|| with_app(|app| app.toggle_works_desc()));
    ui.on_set_works_filter(on(|app, index: i32| {
        app.set_works_filter(index.max(0) as usize)
    }));
    ui.on_open_works_tile(on(|app, key: slint::SharedString| {
        app.open_works_tile(&key)
    }));
    ui.on_open_work(on(|app, id: i32| app.open_work(i64::from(id))));
    ui.on_play_work(|| with_app(|app| app.play_work()));
    ui.on_set_work_account(on(|app, index: i32| {
        app.set_work_account(index.max(0) as usize)
    }));
    ui.on_open_member(|account, item| with_app(move |app| app.open_account_item(&account, &item)));
    ui.on_split_member(|account, item| with_app(move |app| app.split_member(&account, &item)));
    ui.on_reset_member(|account, item| with_app(move |app| app.reset_member(&account, &item)));
    ui.on_search_work_match(on(|app, text: slint::SharedString| {
        app.search_work_match(&text)
    }));
    ui.on_merge_into_work(|account, item| {
        with_app(move |app| app.merge_into_work(&account, &item))
    });
}

fn connect_navigation(ui: &Ui<'_>) {
    ui.on_navigate(on(|app, page| app.navigate(page)));
    ui.on_back(|| with_app(|app| app.back()));
    ui.on_banner_act(|| with_app(|app| app.banner_act()));
    ui.on_banner_dismiss(|| with_app(|app| app.banner_dismiss()));
    ui.on_edit_theme_file(|| with_app(|app| app.edit_theme_file()));
    ui.on_reload_theme(|| with_app(|app| app.apply_theme()));
    ui.on_import_theme(|| with_app(|app| app.import_theme()));
    ui.on_export_theme(|| with_app(|app| app.export_theme()));
    ui.on_stop_theme_file(|| with_app(|app| app.stop_theme_file()));
    ui.on_remove_theme(|| with_app(|app| app.remove_theme()));
    ui.on_open_link(|url| with_app(move |app| app.open_link(&url)));
    ui.on_sync_now(|| with_app(|app| app.sync_now()));
    ui.on_set_setting(|key, index| {
        with_app(move |app| app.set_setting(&key, index.max(0) as usize))
    });
}

fn connect_browse(ui: &Ui<'_>) {
    ui.on_select_library(on(|app, index: i32| {
        app.select_library(index.max(0) as usize)
    }));
    ui.on_grid_columns(|name, columns| {
        with_app(move |app| app.grid_columns(&name, columns.max(1) as usize))
    });
    ui.on_grid_visible(|name, first, last| {
        with_app(move |app| app.grid_visible(&name, first.max(0) as usize, last.max(0) as usize))
    });
    ui.on_open_item(on(|app, id: slint::SharedString| app.open_item(&id)));
    ui.on_set_library_view(on(|app, view| app.set_library_view(view)));
    ui.on_set_sort(on(|app, index: i32| app.set_sort(index.max(0) as usize)));
    ui.on_toggle_desc(|| with_app(|app| app.toggle_desc()));
    ui.on_set_filter(on(|app, index: i32| app.set_filter(index.max(0) as usize)));
    ui.on_open_group(on(|app, id: slint::SharedString| app.open_group(&id)));
    ui.on_open_author(on(|app, name: slint::SharedString| app.open_author(&name)));
    ui.on_peek_item(on(|app, id: slint::SharedString| app.peek_item(&id)));
    ui.on_search(on(|app, text: slint::SharedString| app.search(&text)));
    ui.on_search_server(|| with_app(|app| app.search_server()));
    ui.on_play_item(on(|app, id: slint::SharedString| app.play_item(&id, None)));
    ui.on_play_from(|id, start| with_app(move |app| app.play_item(&id, Some(start as f64))));
    ui.on_set_finished(|id, finished| with_app(move |app| app.set_finished(&id, finished)));
    ui.on_read_item(on(|app, id: slint::SharedString| app.read_item(&id)));
}

fn connect_player(ui: &Ui<'_>) {
    let action = |action: Action| move || with_app(move |app| app.player_action(action));
    ui.on_toggle_play(action(Action::Toggle));
    ui.on_skip_back(action(Action::SkipBack));
    ui.on_skip_forward(action(Action::SkipForward));
    ui.on_previous_chapter(action(Action::PreviousChapter));
    ui.on_next_chapter(action(Action::NextChapter));
    ui.on_seek_fraction(on(|app, f: f32| {
        app.player_action(Action::SeekFraction(f as f64))
    }));
    ui.on_seek_chapter_fraction(on(|app, f: f32| {
        app.player_action(Action::SeekChapterFraction(f as f64))
    }));
    ui.on_seek_to(on(|app, at: f32| {
        app.player_action(Action::SeekTo(at as f64))
    }));
    ui.on_set_speed(on(|app, i: i32| {
        app.player_action(Action::Speed(i.max(0) as usize))
    }));
    ui.on_set_sleep(on(|app, i: i32| {
        app.player_action(Action::Sleep(i.max(0) as usize))
    }));
    ui.on_set_volume(on(|app, percent: f32| app.set_volume(percent as f64)));
    ui.on_toggle_mute(|| with_app(|app| app.toggle_mute()));
    ui.on_add_bookmark(|| with_app(|app| app.add_bookmark()));
    ui.on_remove_bookmark(on(|app, time: f32| app.remove_bookmark(time as f64)));
}

fn connect_downloads(ui: &Ui<'_>) {
    ui.on_download(|account, item| with_app(move |app| app.download(&account, &item)));
    ui.on_pause_download(|account, item| with_app(move |app| app.pause_download(&account, &item)));
    ui.on_resume_download(|account, item| {
        with_app(move |app| app.resume_download(&account, &item))
    });
    ui.on_delete_download(|account, item| {
        with_app(move |app| app.delete_download(&account, &item))
    });
    ui.on_resolve_conflict(on(|app, keep_local: bool| app.resolve_conflict(keep_local)));
    ui.on_choose_download_folder(|| with_app(|app| app.choose_download_folder()));
    ui.on_reset_download_folder(|| with_app(|app| app.reset_download_folder()));
}

fn connect_accounts(ui: &Ui<'_>) {
    ui.on_add_account(|url, username, password| {
        with_app(move |app| app.add_account(url.into(), username.into(), password.into()))
    });
    ui.on_use_account(on(|app, id: slint::SharedString| app.activate(&id)));
    ui.on_remove_account(on(|app, id: slint::SharedString| app.remove_account(&id)));
    ui.on_set_account_enabled(|id, enabled| {
        with_app(move |app| app.set_account_enabled(&id, enabled))
    });
    ui.on_relogin(on(|app, id: slint::SharedString| app.relogin(&id)));
    ui.on_add_another_account(|| with_app(|app| app.show_add_account(true)));
    ui.on_add_server_url(on(|app, url: slint::SharedString| app.add_server_url(&url)));
    ui.on_remove_server_url(on(|app, index: i32| {
        app.remove_server_url(index.max(0) as usize)
    }));
    ui.on_raise_server_url(on(|app, index: i32| {
        app.raise_server_url(index.max(0) as usize)
    }));
}
