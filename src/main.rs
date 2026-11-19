#![cfg_attr(windows, windows_subsystem = "windows")]

mod disk_worker;
mod steam_guard;
mod steam_ownership;
mod library;
mod metadata;

mod tests;
mod ui_checks;
mod text_input;
use text_input::input;
use library::*;
use macroquad::prelude::*;
use std::{collections::BTreeMap, path::PathBuf, sync::mpsc};
static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub fn suffix_year(s: &str) -> Option<u16> {
    let s = s.trim_end().trim_end_matches('✓').trim_end();
    let b = s.as_bytes();
    if b.len() >= 6 && b[b.len() - 6] == b'(' && b[b.len() - 1] == b')' {
        let y = std::str::from_utf8(&b[b.len() - 5..b.len() - 1])
            .ok()?
            .parse()
            .ok()?;
        if (1970..=2100).contains(&y) {
            return Some(y);
        }
    }
    None
}
pub fn title_year(s: &str) -> Option<u16> {
    let b = s.as_bytes();
    b.windows(4).enumerate().find_map(|(i, w)| {
        if (i > 0 && b[i - 1].is_ascii_digit()) || (i + 4 < b.len() && b[i + 4].is_ascii_digit()) {
            return None;
        }
        let y = std::str::from_utf8(w).ok()?.parse::<u16>().ok()?;
        if (1970..=2100).contains(&y) {
            Some(y)
        } else {
            None
        }
    })
}
pub fn append_year(s: &str, y: u16) -> String {
    if title_year(s).is_some() || has_multi_year(s) {
        s.into()
    } else {
        let trimmed = s.trim_end();
        if let Some(base) = trimmed.strip_suffix(" ✓") {
            format!("{} ({y}) ✓", base.trim_end())
        } else {
            format!("{trimmed} ({y})")
        }
    }
}
fn arg(key: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter()
        .position(|s| s == key)
        .and_then(|p| a.get(p + 1).cloned())
}
fn config() -> Conf {
    Conf {
        window_title: "Yet AnoTher Steam Editor (YATSE) — Release year editor".into(),
        window_width: 1440,
        window_height: 940,
        high_dpi: true,
        window_resizable: true,
        ..Default::default()
    }
}
const INK: Color = Color::new(0.12, 0.18, 0.22, 1.);
const MUTED: Color = Color::new(0.43, 0.49, 0.53, 1.);
const LINE: Color = Color::new(0.88, 0.90, 0.91, 1.);
const TEAL: Color = Color::new(0.05, 0.40, 0.36, 1.);
thread_local! {static FONT:std::cell::RefCell<Option<Font>>=const{std::cell::RefCell::new(None)};}
fn txt(s: &str, x: f32, y: f32, size: f32, c: Color) {
    FONT.with(|f| {
        draw_text_ex(
            s,
            x,
            y,
            TextParams {
                font: f.borrow().as_ref(),
                font_size: size as u16,
                color: c,
                ..Default::default()
            },
        )
    });
}
fn metrics(s: &str, size: u16) -> TextDimensions {
    FONT.with(|f| measure_text(s, f.borrow().as_ref(), size, 1.))
}
fn fit(s: &str, w: f32, size: f32) -> String {
    if metrics(s, size as u16).width <= w {
        return s.into();
    }
    let mut s = s.to_string();
    while metrics(&(s.clone() + "…"), size as u16).width > w && !s.is_empty() {
        s.pop();
    }
    s + "…"
}
fn panel(x: f32, y: f32, w: f32, h: f32, c: Color) {
    draw_rectangle(x, y, w, h, c);
}
fn button(s: &str, r: Rect, primary: bool, enabled: bool) -> bool {
    let hover = r.contains(Vec2::from(mouse_position()));
    let bg = if !enabled {
        Color::from_hex(0xe9edec)
    } else if primary {
        if hover {
            Color::from_hex(0x075b50)
        } else {
            TEAL
        }
    } else if hover {
        Color::from_hex(0xe7efec)
    } else {
        WHITE
    };
    panel(r.x, r.y, r.w, r.h, bg);
    if !primary {
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 1., LINE)
    }
    let size = 17.;
    let m = metrics(s, size as u16);
    txt(
        s,
        r.x + (r.w - m.width) / 2.,
        r.y + (r.h + size * 0.65) / 2.,
        size,
        if primary && enabled {
            WHITE
        } else if enabled {
            INK
        } else {
            MUTED
        },
    );
    enabled && hover && is_mouse_button_pressed(MouseButton::Left)
}
fn badge(s: &str, x: f32, y: f32) {
    let color = match s {
        "Ready" => TEAL,
        "Review" => Color::from_hex(0xa76518),
        _ => MUTED,
    };
    let w = metrics(s, 15).width + 18.;
    panel(
        x,
        y - 17.,
        w,
        25.,
        if s == "Review" {
            Color::from_hex(0xfff1d9)
        } else {
            Color::from_hex(0xeaf2ee)
        },
    );
    txt(s, x + 9., y + 1., 15., color)
}
fn checkbox(x: f32, y: f32, on: bool, enabled: bool) -> bool {
    panel(x, y, 17., 17., if on { TEAL } else { WHITE });
    draw_rectangle_lines(x, y, 17., 17., 1., if enabled { MUTED } else { LINE });
    if on {
        draw_line(x + 4., y + 9., x + 7., y + 12., 2., WHITE);
        draw_line(x + 7., y + 12., x + 14., y + 4., 2., WHITE)
    }
    enabled
        && Rect::new(x - 5., y - 5., 27., 27.).contains(Vec2::from(mouse_position()))
        && is_mouse_button_pressed(MouseButton::Left)
}
fn wrap(s: &str, x: f32, mut y: f32, w: f32, size: f32, c: Color) -> f32 {
    let mut line = String::new();
    for word in s.split_whitespace() {
        let next = if line.is_empty() {
            word.into()
        } else {
            format!("{line} {word}")
        };
        if metrics(&next, size as u16).width > w && !line.is_empty() {
            txt(&line, x, y, size, c);
            y += size + 6.;
            line = word.into()
        } else {
            line = next
        }
    }
    if !line.is_empty() {
        txt(&line, x, y, size, c);
        y += size + 6.;
    }
    y
}

#[cfg(windows)]
fn background_window() {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn FindWindowW(class: *const u16, title: *const u16) -> isize;
        fn SetWindowPos(
            h: isize,
            after: isize,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
        fn ShowWindow(h: isize, cmd: i32) -> i32;
    }
    let title: Vec<u16> = "Yet AnoTher Steam Editor (YATSE) — Release year editor"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let h = FindWindowW(std::ptr::null(), title.as_ptr());
        if h != 0 {
            ShowWindow(h, 4);
            SetWindowPos(h, 1, 0, 0, 0, 0, 0x0013);
        }
    }
}
#[cfg(not(windows))]
fn background_window() {}

// Share portable settings between debug and release builds in this checkout.
// A standalone executable keeps its settings beside the executable.
fn settings_dir() -> PathBuf {
    if let Some(dir) = arg("--data-dir") {
        return PathBuf::from(dir);
    }
    let exe = std::env::current_exe().expect("executable path");
    let parent = exe.parent().expect("executable directory");
    parent.ancestors()
        .find(|dir| dir.join("Cargo.toml").is_file() && dir.join("src/main.rs").is_file())
        .unwrap_or(parent)
        .join("settings")
}

fn main() {
    STARTED.set(std::time::Instant::now()).ok();
    // GUI startup enters the event loop before any filesystem or registry work.
    let cli = arg("--self-test").is_some() || std::env::args().any(|a| a == "--inspect");
    if !cli {
        macroquad::Window::from_config(config(), ui_main());
        return;
    }
    let data = settings_dir();
    let root = arg("--steam-path")
        .map(PathBuf::from)
        .unwrap_or_else(detect);
    let account = arg("--account").unwrap_or_default();
    if let Some(out) = arg("--self-test") {
        let result = tests::run(&root, &data);
        let p = PathBuf::from(out);
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(p, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
        macroquad::Window::from_config(config(), ui_main());
        return;
    }
    if std::env::args().any(|a| a == "--inspect") {
        match discover(&root, &account, &data) {
            Ok(l) => {
                println!("{}\n{} games", l.coverage, l.rows.len());
                for r in &l.rows {
                    println!(
                        "{} | {} | {:?} | {} | {}",
                        r.id,
                        r.old,
                        r.year,
                        r.source,
                        r.status()
                    )
                }
                let db = metadata::Database::read(&root.join("appcache/appinfo.vdf")).unwrap();
                for r in l.rows.iter().filter(|r| !r.existing).take(3) {
                    let mut c = BTreeMap::new();
                    c.insert(r.id, append_year(&r.old, 2001));
                    println!(
                        "Patch validation {}: {:?}",
                        r.id,
                        db.patch(&c).map(|b| b.len())
                    );
                }
                let changes = preview(&l);
                println!("Read-only batch validation ({} selected titles): {:?}", changes.len(), db.patch(&changes).map(|b| b.len()));
            }
            Err(e) => println!("{e}"),
        }
        return;
    }
    macroquad::Window::from_config(config(), ui_main());
}
async fn load_library(root: PathBuf, account: String, dir: PathBuf) -> Result<Library, String> {
    let worker = disk_worker::DiskWorker::new();
    let (tx, rx) = mpsc::channel();
    let result = worker.submit(move || discover_progress(&root, &account, &dir, |percent, message| {
        let _ = tx.send((percent, message));
    }));
    let mut progress = (0u8, "Starting library scan");
    loop {
        for update in rx.try_iter() { progress = update; }
        clear_background(Color::from_hex(0xf5f7f6));
        txt("Yet AnoTher Steam Editor (YATSE)", 40., 65., 28., INK);
        txt(progress.1, 40., 125., 21., INK);
        let width = (screen_width() - 80.).max(100.);
        draw_rectangle(40., 155., width, 12., Color::from_hex(0xdce5e1));
        draw_rectangle(40., 155., width * progress.0 as f32 / 100., 12., TEAL);
        draw_circle(45. + (get_time() as f32 * 0.4).fract() * (width - 10.), 190., 4., TEAL);
        txt(&format!("{}%", progress.0), 40., 230., 18., MUTED);
        next_frame().await;
        match result.try_recv() {
            Ok(library) => return library,
            Err(mpsc::TryRecvError::Empty) => {},
            Err(mpsc::TryRecvError::Disconnected) => return Err("Library worker stopped.".into()),
        }
    }
}

async fn wait_for_steam(monitor: &mut steam_guard::Monitor) {
    while let Some(message) = monitor.blocked() {
        steam_blocked_screen(&message);
        next_frame().await;
    }
}
fn steam_blocked_screen(message: &str) {
    clear_background(Color::from_hex(0xf5f7f6));
    txt("Yet AnoTher Steam Editor (YATSE)", 40., 65., 28., INK);
    txt("Close Steam to continue", 40., 130., 26., INK);
    wrap(message, 40., 180., (screen_width()-80.).min(800.), 20., MUTED);
    txt("Checking automatically. Your edits are kept while this screen is shown.", 40., 310., 18., MUTED);
    let width = (screen_width()-80.).min(800.);
    draw_rectangle(40., 350., width, 8., LINE);
    draw_rectangle(40.+(get_time() as f32*0.5).fract()*(width-80.),350.,80.,8.,TEAL);
}

fn startup_frame(message: &str) {
    clear_background(Color::from_hex(0xf5f7f6));
    txt("Yet AnoTher Steam Editor (YATSE)", 40., 65., 28., INK);
    txt(message, 40., 125., 21., INK);
    let width = (screen_width() - 80.).max(100.);
    draw_rectangle(40., 155., width, 12., Color::from_hex(0xdce5e1));
    let segment = width * 0.2;
    draw_rectangle(40. + (get_time() as f32 * 0.5).fract() * (width-segment), 155., segment, 12., TEAL);
}

async fn ui_main() {
    // The Windows backend interprets Conf dimensions as physical pixels.
    // Request logical dimensions so DPI affects sharpness without shrinking layout.
    request_new_screen_size(1440., 940.);
    // Present a real frame before detection, font loading, or database scanning.
    startup_frame("Starting YATSE");
    next_frame().await;
    if let Some(path) = arg("--startup-check") {
        let result = serde_json::json!({
            "first_frame_ms": STARTED.get().unwrap().elapsed().as_millis(),
            "before_steam_detection_and_font_loading": true,
            "dpi_scale": screen_dpi_scale(),
            "logical_width": screen_width(),
        });
        std::fs::write(path, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    }
    let worker = disk_worker::DiskWorker::new();
    let startup = worker.submit(|| {
        let data = settings_dir();
        let root = arg("--steam-path").map(PathBuf::from).unwrap_or_else(detect);
        let account = arg("--account").unwrap_or_default();
        let font = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf").ok();
        (data, root, account, font)
    });
    let (data, root, account, font) = loop {
        startup_frame("Locating Steam and preparing fonts");
        next_frame().await;
        match startup.try_recv() {
            Ok(result) => break result,
            Err(mpsc::TryRecvError::Empty) => {},
            Err(mpsc::TryRecvError::Disconnected) => panic!("Startup worker stopped"),
        }
    };
    if let Some(bytes) = font {
        if let Ok(font) = load_ttf_font_from_bytes(&bytes) {
            FONT.with(|f| *f.borrow_mut() = Some(font));
        }
    }
    let shot = arg("--screenshot");
    let test_out = arg("--self-test");
    if shot.is_some() || test_out.is_some() {
        background_window();
    }
    let mut steam_monitor = steam_guard::Monitor::new();
    let mut lib = load_library(root.clone(), account.clone(), data.clone()).await.unwrap_or_else(|e| Library {
        path: root.clone(),
        account: account.clone(),
        rows: vec![],
        fingerprint: String::new(),
        coverage: e,
    });
    // Capture signed-in ownership before asking Steam to close for safe editing.
    let steam_was_running = steam_guard::check().unwrap_or(true);
    wait_for_steam(&mut steam_monitor).await;
    if steam_was_running {
        if let Ok(refreshed) = load_library(root.clone(), account.clone(), data.clone()).await {
            lib = refreshed;
        }
    }
    let mut status = String::new();
    let mut search = String::new();
    let mut tab = 0;
    let mut offset = 0usize;
    let mut current = lib.rows.iter().position(|r| !r.existing).unwrap_or(0);
    let mut active = 0;
    let mut year_edit = String::new();
    let mut title_edit = String::new();
    let mut detail_id = 0u32;
    let mut root_edit = root.to_string_lossy().to_string();
    let mut account_edit = lib.account.clone();
    let mut api_key = String::new();
    let mut ownership: Option<mpsc::Receiver<Result<BTreeMap<u32, String>, String>>> = None;
    let mut backups = false;
    let mut settings = false;
    let disk = disk_worker::DiskWorker::new();
    let mut disk_job: Option<mpsc::Receiver<Result<(String, Option<Library>), String>>> = None;
    let mut settings_jobs: Vec<mpsc::Receiver<Result<(), String>>> = Vec::new();
    let mut confirm = false;
    let mut save_notice: Option<(bool, String)> = None;
    let mut network: Option<mpsc::Receiver<(u32, Result<Option<u16>, String>)>> = None;
    let mut pending: usize = 0;
    let mut attempted = std::collections::BTreeSet::new();
    let mut frames = 0;
    let mut rendered_rows = 0usize;
    loop {
        if let Some(message) = steam_monitor.blocked() {
            steam_blocked_screen(&message);
            next_frame().await;
            continue;
        }
        clear_background(Color::from_hex(0xf5f7f6));
        let w = screen_width();
        let h = screen_height();
        settings_jobs.retain(|rx| match rx.try_recv() {
            Ok(Ok(())) => false,
            Ok(Err(e)) => { status = format!("Could not save settings: {e}"); false },
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => { status = "Settings worker stopped.".into(); false },
        });
        if let Some(rx) = &disk_job {
            let result = match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("Disk worker stopped unexpectedly. Check the backup before retrying.".into())),
            };
            if let Some(result) = result {
                match result {
                    Ok((message, refreshed)) => {
                        if let Some(updated) = refreshed { lib = updated; detail_id = 0; }
                        status = message.clone();
                        save_notice = Some((true, message));
                    }
                    Err(e) => { status = e.clone(); save_notice = Some((false, e)); }
                }
                disk_job = None;
            }
        }
        if disk_job.is_some() {
            txt("Yet AnoTher Steam Editor (YATSE)", 40., 60., 28., INK);
            txt(&status, 40., 120., 21., INK);
            txt("Please keep YATSE open until the operation finishes.", 40., 160., 18., MUTED);
            let angle = get_time() as f32 * 4.;
            draw_circle(55. + angle.cos() * 12., 210. + angle.sin() * 12., 5., TEAL);
            next_frame().await;
            continue;
        }
        let left = 224.;
        let side = 342.;
        let content = w - left;
        let table_w = (content - side - 52.).max(420.);
        let dx = w - side - 20.;
        if let Some(rx) = &ownership {
            if let Ok(result) = rx.try_recv() {
                ownership = None;
                match result {
                    Ok(games) => {
                        let snapshot = lib.clone();
                        let dir = data.clone();
                        status = "Saving ownership data...".into();
                        disk_job = Some(disk.submit(move || {
                            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                            write_json(&dir.join(format!("owned-{}.json", snapshot.account)), &games)?;
                            let refreshed = discover(&snapshot.path, &snapshot.account, &dir)?;
                            Ok((format!("Steam returned {} owned AppIDs. Local and installed records merged.", games.len()), Some(refreshed)))
                        }));
                    }
                    Err(e) => status = e,
                }
            }
        }
        if let Some(rx) = &network {
            while let Ok((id, result)) = rx.try_recv() {
                pending = pending.saturating_sub(1);
                if let Some(r) = lib.rows.iter_mut().find(|r| r.id == id) {
                    match result {
                        Ok(Some(y)) => {
                            if r.approved {
                                continue;
                            }
                            r.year = Some(y);
                            r.proposed = append_year(&r.old, y);
                            r.approved = true;
                            r.selected = !r.excluded;
                            r.source = "Steam Store API • release date".into();
                            r.note="Store dates can describe a port, remaster or later launch. Review the original year.".into();
                        }
                        Ok(None) => r.note =
                            "Store did not provide a usable release date. Enter a verified year."
                                .into(),
                        Err(e) => r.note = format!("Store lookup: {e}"),
                    }
                }
            }
            if pending == 0 {
                network = None;
                status = "Years added. You can change any year before saving.".into();
            }
        }
        panel(0., 0., left, h, Color::from_hex(0xeff3f0));
        draw_line(left, 0., left, h, 1., LINE);
        panel(24., 28., 32., 32., TEAL);
        txt("Y", 33., 51., 25., WHITE);
        txt("YATSE", 67., 50., 23., INK);
        txt("A little order for your library", 24., 83., 15., MUTED);
        txt("WORKSPACE", 24., 134., 13., MUTED);
        for (i, label) in ["Game titles", "Backups & safety", "Library settings"]
            .iter()
            .enumerate()
        {
            let y = 151. + i as f32 * 46.;
            let on =
                (!settings && !backups && i == 0) || (backups && i == 1) || (settings && i == 2);
            if on {
                panel(12., y, 200., 38., Color::from_hex(0xdce9e1))
            }
            if button(label, Rect::new(18., y + 2., 188., 34.), false, true) {
                settings = i == 2;
                backups = i == 1;
            }
        }
        txt("YOUR LIBRARY", 24., 343., 13., MUTED);
        txt(
            &format!("{} games found", lib.rows.len()),
            24.,
            377.,
            21.,
            INK,
        );
        txt(
            &format!(
                "Account {}",
                if lib.account.is_empty() {
                    "not selected"
                } else {
                    &lib.account
                }
            ),
            24.,
            404.,
            15.,
            MUTED,
        );
        let _ = wrap(
            "Start with Add years to all. Select a game to adjust its year or title.",
            24.,
            443.,
            173.,
            16.,
            MUTED,
        );
        panel(16., h - 155., 192., 127., Color::from_hex(0xe2ebe5));
        txt("Safe by design", 29., h - 128., 18., TEAL);
        wrap(
            "Preview first. Every write gets a verified backup.",
            29.,
            h - 101.,
            162.,
            16.,
            INK,
        );
        let x = left + 28.;
        txt("LIBRARY TOOLS  /  GAME TITLES", x, 37., 13., MUTED);
        txt("Edit your Steam game titles", x, 79., 32., INK);
        txt(
            "Select a game, edit its title or year, then save the results.",
            x,
            108.,
            17.,
            MUTED,
        );
        if backups {
            panel(x, 140., content - 56., h - 275., WHITE);
            txt("Your safety net", x + 24., 185., 28., INK);
            wrap("Every apply stores the complete original metadata, checksums and an AppID change list. Rollback restores the latest snapshot only when no later metadata changes would be lost.",x+24.,234.,content-110.,19.,MUTED);
            if button(
                "Export dry run",
                Rect::new(x + 24., 590., 214., 42.),
                false,
                !preview(&lib).is_empty(),
            ) {
                let snapshot = lib.clone();
                let dir = data.clone();
                status = "Exporting dry run?".into();
                disk_job = Some(disk.submit(move || {
                    dry_run(&snapshot, &dir)?;
                    Ok(("Dry run exported. No Steam metadata changed.".into(), None))
                }));
            }
            let history = data.join("backups");
            let count = std::fs::read_dir(&history).map(|r| r.count()).unwrap_or(0);
            txt(
                &format!("{} backup snapshots", count),
                x + 24.,
                345.,
                24.,
                TEAL,
            );
            let latest = std::fs::read(data.join("latest-backup.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<Backup>(&b).ok());
            if let Some(b) = latest {
                txt(
                    &format!("Latest apply: {} titles", b.changes.len()),
                    x + 24.,
                    397.,
                    20.,
                    INK,
                );
                wrap(
                    &format!("Before: {}   After: {}", b.before, b.after),
                    x + 24.,
                    438.,
                    content - 110.,
                    16.,
                    MUTED,
                );
            } else {
                txt(
                    "No changes have been applied yet.",
                    x + 24.,
                    397.,
                    19.,
                    MUTED,
                );
            }
            if button(
                "Open backup folder",
                Rect::new(x + 24., 515., 214., 42.),
                false,
                true,
            ) {
                settings_jobs.push(disk.submit(move || {
                    std::fs::create_dir_all(&history).map_err(|e| e.to_string())?;
                    std::process::Command::new("explorer").arg(&history).spawn().map_err(|e| e.to_string())?;
                    Ok(())
                }));
            }
            if button(
                "Restore latest backup",
                Rect::new(x + 258., 515., 228., 42.),
                true,
                count > 0,
            ) {
                let snapshot = lib.clone();
                let dir = data.clone();
                status = "Restoring verified backup...".into();
                disk_job = Some(disk.submit(move || {
                    let message = rollback(&dir, true)?;
                    let refreshed = discover(&snapshot.path, &snapshot.account, &dir).ok();
                    Ok((message, refreshed))
                }));
            }
        } else if settings {
            panel(x, 140., content - 56., h - 260., WHITE);
            txt("Connect your local library", x + 24., 180., 26., INK);
            txt("Steam folder", x + 24., 222., 17., INK);
            input(
                4,
                &mut active,
                &mut root_edit,
                Rect::new(x + 24., 238., content - 108., 40.),
                "Steam folder",
            );
            txt("Account folder ID", x + 24., 316., 17., INK);
            input(
                5,
                &mut active,
                &mut account_edit,
                Rect::new(x + 24., 333., 360., 40.),
                "Select an account ID",
            );
            let choices = accounts(&PathBuf::from(&root_edit));
            txt(
                &format!("Local accounts: {}", choices.join(", ")),
                x + 24.,
                407.,
                16.,
                MUTED,
            );
            wrap("Open Steam and sign in before launching Yatse to refresh ownership automatically. Close Steam when prompted to edit safely. Verified ownership is saved for offline use; Web API verification below is optional.",x+24.,451.,content-120.,18.,MUTED);
            if button(
                "Load this library",
                Rect::new(x + 24., 540., 190., 43.),
                true,
                true,
            ) {
                match load_library(PathBuf::from(&root_edit), account_edit.clone(), data.clone()).await {
                    Ok(l) => {
                        lib = l;
                        current = 0;
                        detail_id = 0;
                        settings = false;
                        status = "Library loaded.".into()
                    }
                    Err(e) => status = e,
                }
            }
            txt(
                "Optional: verify purchases missing from the local cache",
                x + 24.,
                627.,
                20.,
                INK,
            );
            input(
                6,
                &mut active,
                &mut api_key,
                Rect::new(x + 24., 650., 440., 39.),
                "Steam Web API key • kept in memory only",
            );
            if button(
                "Verify owned library",
                Rect::new(x + 482., 650., 223., 39.),
                true,
                ownership.is_none() && !lib.account.is_empty(),
            ) {
                let key = std::mem::take(&mut api_key);
                let acct = lib.account.clone();
                let (tx, rx) = mpsc::channel();
                ownership = Some(rx);
                std::thread::spawn(move || {
                    let _ = tx.send(fetch_owned(&acct, &key));
                });
                status = "Checking Steam ownership for this account…".into();
            }
            if button(
                "Get a Steam API key",
                Rect::new(x + 24., 704., 215., 34.),
                false,
                true,
            ) {
                let _ = std::process::Command::new("rundll32")
                    .args([
                        "url.dll,FileProtocolHandler",
                        "https://steamcommunity.com/dev/apikey",
                    ])
                    .spawn();
            }
        } else {
            let ready = lib.rows.iter().filter(|r| r.ready()).count();
            panel(x, 137., content - 56., 96., WHITE);
            if button(
                if pending > 0 {
                    "Looking up…"
                } else {
                    "1. Add year to all"
                },
                Rect::new(x, 143., 255., 42.),
                false,
                pending == 0 && !settings && !backups && !confirm,
            ) {
                for r in &mut lib.rows {
                    if !r.existing && !r.excluded && !r.approved && r.year.is_some() {
                        r.approved = true;
                        r.selected = true;
                    }
                }
                let rows = lib.rows.clone();
                let dir = data.clone();
                settings_jobs.push(disk.submit(move || save(&dir, &rows)));
                detail_id = 0;
                let ids: Vec<u32> = lib
                    .rows
                    .iter()
                    .filter(|r| {
                        !r.existing && !r.excluded && !r.approved && r.year.is_none() && !attempted.contains(&r.id)
                    })
                    .map(|r| r.id)
                    .collect();
                if ids.is_empty() {
                    status = "Years added. Edit any year you want to change, then Save the results."
                        .into()
                } else {
                    pending = ids.len();
                    attempted.extend(ids.iter().copied());
                    let (tx, rx) = mpsc::channel();
                    network = Some(rx);
                    std::thread::spawn(move || {
                        for id in ids {
                            let _ = tx.send((id, store_year(id)));
                            std::thread::sleep(std::time::Duration::from_millis(1400));
                        }
                    });
                    status = "Adding years across your library. Missing or uncertain dates will be flagged for editing.".into();
                }
            }
            txt("2. Manually edit below", x + 280., 169., 20., INK);
            txt(
                "Add years in bulk, or edit a complete title below.",
                x + 280.,
                201.,
                16.,
                MUTED,
            );
            panel(x, 249., table_w, h - 374., WHITE);
            input(
                1,
                &mut active,
                &mut search,
                Rect::new(x + 16., 264., table_w - 32., 38.),
                "Search games or AppID…",
            );
            let tabs = ["All games", "Ready", "Review", "Has year", "Excluded"];
            for (i, t) in tabs.iter().enumerate() {
                let tx = x + 15. + i as f32 * (table_w - 30.) / 5.;
                let tw = (table_w - 30.) / 5. - 3.;
                if button(t, Rect::new(tx, 315., tw, 31.), tab == i, true) {
                    tab = i;
                    offset = 0;
                }
            }
            let query = search.to_lowercase();
            let filtered: Vec<usize> = lib
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| {
                    (r.old.to_lowercase().contains(&query) || r.id.to_string().contains(&query))
                        && match tab {
                            1 => r.ready(),
                            2 => r.status() == "Review",
                            3 => r.existing,
                            4 => r.excluded,
                            _ => true,
                        }
                })
                .map(|(i, _)| i)
                .collect();
            let (_, scroll) = mouse_wheel();
            if Rect::new(x, 350., table_w, h - 475.).contains(Vec2::from(mouse_position())) {
                if scroll < 0. {
                    offset = offset.saturating_add(3)
                } else if scroll > 0. {
                    offset = offset.saturating_sub(3)
                }
            }
            let count = ((h - 501.) / 66.).max(1.) as usize;
            offset = offset.min(filtered.len().saturating_sub(count));
            let all = lib.rows.iter().filter(|r| r.ready()).all(|r| r.selected);
            if checkbox(x + 18., 365., all, ready > 0) {
                for r in &mut lib.rows {
                    if r.ready() {
                        r.selected = !all
                    }
                }
            }
            txt("CURRENT TITLE / APPID", x + 49., 378., 12., MUTED);
            txt(
                "PROPOSED TITLE / SOURCE",
                x + table_w * 0.49,
                378.,
                12.,
                MUTED,
            );
            draw_line(x, 393., x + table_w, 393., 1., LINE);
            rendered_rows = 0;
            for (n, &idx) in filtered.iter().skip(offset).take(count).enumerate() {
                rendered_rows += 1;
                let y = 394. + n as f32 * 66.;
                let r = &mut lib.rows[idx];
                if current == idx {
                    panel(x, y, table_w, 66., Color::from_hex(0xf0f6f2));
                    panel(x, y, 3., 66., TEAL)
                }
                draw_line(x, y + 66., x + table_w, y + 66., 1., LINE);
                if checkbox(x + 18., y + 23., r.selected, r.ready()) {
                    r.selected = !r.selected;
                }
                let a = x + 49.;
                let col = x + table_w * 0.49;
                let title_w = table_w * 0.43 - 45.;
                txt(&fit(&r.old, title_w, 17.), a, y + 26., 17., INK);
                txt(
                    &format!(
                        "{}  •  {}",
                        r.id,
                        if r.installed {
                            "Installed"
                        } else {
                            "Owned library"
                        }
                    ),
                    a,
                    y + 47.,
                    13.,
                    MUTED,
                );
                txt(
                    &fit(&r.proposed, table_w * 0.49 - 26., 16.),
                    col,
                    y + 26.,
                    16.,
                    if r.ready() { TEAL } else { INK },
                );
                txt(
                    &fit(
                        &format!(
                            "{}  ·  {}",
                            r.year.map(|x| x.to_string()).unwrap_or("—".into()),
                            r.source
                        ),
                        table_w * 0.49 - 26.,
                        13.,
                    ),
                    col,
                    y + 47.,
                    13.,
                    MUTED,
                );
                if Rect::new(a, y, table_w - 49., 66.).contains(Vec2::from(mouse_position()))
                    && is_mouse_button_pressed(MouseButton::Left)
                {
                    current = idx;
                }
            }
            txt(
                &format!(
                    "{}–{} of {}",
                    if filtered.is_empty() { 0 } else { offset + 1 },
                    (offset + count).min(filtered.len()),
                    filtered.len()
                ),
                x + 17.,
                h - 143.,
                14.,
                MUTED,
            );
            if button(
                "‹",
                Rect::new(x + table_w - 77., h - 166., 28., 29.),
                false,
                offset > 0,
            ) {
                offset = offset.saturating_sub(count)
            }
            if button(
                "›",
                Rect::new(x + table_w - 43., h - 166., 28., 29.),
                false,
                offset + count < filtered.len(),
            ) {
                offset += count
            }
            panel(dx, 249., side, h - 374., WHITE);
            txt("2. MANUALLY EDIT", dx + 19., 279., 13., MUTED);
            if let Some(r) = lib.rows.get_mut(current) {
                if detail_id != r.id {
                    detail_id = r.id;
                    year_edit = r.year.map(|y| y.to_string()).unwrap_or_default();
                    title_edit = r.proposed.clone();
                    active = 0;
                }
                badge(r.status(), dx + 19., 312.);
                txt(
                    &format!("AppID {}", r.id),
                    dx + side - 120.,
                    313.,
                    14.,
                    MUTED,
                );
                txt(&fit(&r.old, side - 38., 22.), dx + 19., 359., 22., INK);
                let by = 370.;
                txt("RELEASE YEAR", dx + 19., by + 19., 12., MUTED);
                input(
                    2,
                    &mut active,
                    &mut year_edit,
                    Rect::new(dx + 19., by + 30., side - 38., 38.),
                    "YYYY",
                );
                txt("FULL TITLE", dx + 19., by + 94., 12., MUTED);
                if button("Clear", Rect::new(dx + side - 80., by + 73., 60., 27.), false, true) {
                    title_edit.clear();
                    active = 3;
                }
                input(
                    3,
                    &mut active,
                    &mut title_edit,
                    Rect::new(dx + 19., by + 106., side - 38., 40.),
                    "Enter any game title",
                );
                let note = if r.note.is_empty() {
                    "Save any full title, or use the year button to add a release year."
                } else {
                    &r.note
                };
                let ny = wrap(
                    &fit(note, (side - 38.) * 2.3, 16.),
                    dx + 19.,
                    by + 180.,
                    side - 38.,
                    16.,
                    MUTED,
                );
                let sy = wrap(
                    &format!("Source: {}", r.source),
                    dx + 19.,
                    ny + 12.,
                    side - 38.,
                    14.,
                    MUTED,
                );
                let _button_y = sy + 10.;
                if button("Save title", Rect::new(dx + 19., h - 263., side - 38., 39.), true, !r.excluded) {
                    match review_title(r, &title_edit) {
                        Ok(()) => {
                            title_edit = r.proposed.clone();
                            let rows = lib.rows.clone();
                            let dir = data.clone();
                            settings_jobs.push(disk.submit(move || save(&dir, &rows)));
                            status = "Title prepared. Save the results to update Steam.".into();
                        }
                        Err(e) => status = e,
                    }
                }
                let r = lib.rows.get_mut(current).unwrap();
                if button(
                    "Use this year",
                    Rect::new(dx + 19., h - 216., side - 38., 39.),
                    true,
                    !r.excluded,
                ) {
                    if let Ok(y) = year_edit.parse::<u16>() {
                        if (1970..=2100).contains(&y) {
                            let proposed = if title_edit == r.old || title_edit == r.proposed {
                                let clean =
                                    r.old.trim_end().trim_end_matches('\u{2713}').trim_end();
                                let base = if suffix_year(&r.old).is_some() {
                                    clean.get(..clean.len() - 6).unwrap_or(clean).trim_end()
                                } else {
                                    clean
                                };
                                format!(
                                    "{base} ({y}){}",
                                    if r.old.ends_with(" \u{2713}") {
                                        " \u{2713}"
                                    } else {
                                        ""
                                    }
                                )
                            } else {
                                title_edit.trim().into()
                            };
                            if suffix_year(&proposed) != Some(y) {
                                status = "Proposed title must end in the confirmed (YYYY).".into()
                            } else {
                                r.year = Some(y);
                                r.proposed = proposed;
                                r.existing = false;
                                r.approved = true;
                                r.selected = true;
                                r.source = "Manual • reviewed year".into();
                                title_edit = r.proposed.clone();
                                let rows = lib.rows.clone();
                let dir = data.clone();
                settings_jobs.push(disk.submit(move || save(&dir, &rows)));
                                status =
                                    "Review saved. This title is selected for the batch.".into();
                            }
                        } else {
                            status = "Enter a year between 1970 and 2100.".into()
                        }
                    } else {
                        status = "Enter a four-digit release year.".into()
                    }
                }
                if let Some(r) = lib.rows.get_mut(current) {
                    if button(
                        if r.excluded {
                            "Restore to review"
                        } else {
                            "Exclude this game"
                        },
                        Rect::new(dx + 19., h - 167., side - 38., 34.),
                        false,
                        true,
                    ) {
                        r.excluded = !r.excluded;
                        r.selected = false;
                        let rows = lib.rows.clone();
                let dir = data.clone();
                settings_jobs.push(disk.submit(move || save(&dir, &rows)));
                    }
                }
            } else {
                wrap(
                    "No games to show. Check your Steam folder and account in Library settings.",
                    dx + 19.,
                    332.,
                    side - 38.,
                    18.,
                    MUTED,
                );
            }
        }
        panel(left, h - 117., w - left, 117., WHITE);
        draw_line(left, h - 117., w, h - 117., 1., LINE);
        let changes = preview(&lib);
        txt(
            &format!("{} titles ready to save", changes.len()),
            x,
            h - 86.,
            20.,
            INK,
        );
        txt(
            "Backup created automatically",
            x,
            h - 62.,
            14.,
            MUTED,
        );
        let by = h - 104.;
        if button(
            "3. Save the results",
            Rect::new(w - 255., by, 226., 36.),
            true,
            !changes.is_empty() && pending == 0 && !settings && !backups && save_notice.is_none(),
        ) {
            confirm = true;
        }
        txt(&fit(&status, content - 320., 14.), x, h - 25., 14., MUTED);
        if confirm {
            panel(0., 0., w, h, Color::new(0., 0., 0., 0.25));
            let cw = 570.;
            let cx = (w - cw) / 2.;
            let cy = (h - 285.) / 2.;
            panel(cx, cy, cw, 285., WHITE);
            txt("Save these titles to Steam?", cx + 28., cy + 44., 27., INK);
            wrap(&format!("{} AppIDs will be updated. Steam must be fully closed. A verified metadata backup will be made first; launch settings and unrelated properties are preserved.",changes.len()),cx+28.,cy+87.,cw-56.,19.,MUTED);
            if button(
                "Cancel",
                Rect::new(cx + 28., cy + 214., 153., 43.),
                false,
                true,
            ) {
                confirm = false
            }
            if button(
                "Save the results",
                Rect::new(cx + 201., cy + 214., cw - 229., 43.),
                true,
                true,
            ) {
                let snapshot = lib.clone();
                let dir = data.clone();
                status = "Saving titles and verifying backup...".into();
                disk_job = Some(disk.submit(move || {
                    let message = apply(&snapshot, &dir, true)?;
                    let refreshed = discover(&snapshot.path, &snapshot.account, &dir).ok();
                    Ok((message, refreshed))
                }));
                confirm = false
            }
        }
        if let Some((success, message)) = &save_notice {
            panel(0., 0., w, h, Color::new(0., 0., 0., 0.35));
            let cw = 640.;
            let cx = (w - cw) / 2.;
            let cy = (h - 310.) / 2.;
            panel(cx, cy, cw, 310., WHITE);
            txt(if *success { "Operation complete" } else { "Operation failed" }, cx + 28., cy + 45., 27., INK);
            wrap(message, cx + 28., cy + 90., cw - 56., 19., MUTED);
            if button("OK", Rect::new(cx + cw - 181., cy + 239., 153., 43.), true, true) {
                save_notice = None;
            }
        }
        frames += 1;
        ui_checks::step(frames, dx + 280., 490., &title_edit);

        if (shot.is_some() || test_out.is_some()) && frames == 1 {
            background_window()
        }
        if frames >= 120 {
            if let Some(path) = &test_out {
                let mut result: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                if let Some(steps) = result["steps"].as_array_mut() {
                    for step in steps {
                        if step["step"] == "hud" {
                            step["passed"] = serde_json::json!(rendered_rows > 0);
                            step["detail"]=serde_json::json!(format!("Rendered {rendered_rows} actual library rows with title, proposed title, year, source and review inspector over {frames} frames."));
                        }
                    }
                }
                std::fs::write(path, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
                return;
            }
            if let Some(path) = &shot {
                let p = PathBuf::from(path);
                if let Some(parent) = p.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                get_screen_data().export_png(path);
                return;
            }
        }
        next_frame().await;
    }
}
