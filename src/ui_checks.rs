//! Opt-in input regression check. Sends window messages without changing Steam or settings.
use std::cell::RefCell;
thread_local! { static BEFORE: RefCell<String> = const { RefCell::new(String::new()) }; }
thread_local! { static HOME_PASSED: RefCell<bool> = const { RefCell::new(false) }; }

pub fn step(frame: u32, x: f32, y: f32, title: &str) {
    let Some(path) = crate::arg("--ui-check") else {
        return;
    };
    if frame == 10 {
        BEFORE.with(|s| *s.borrow_mut() = title.into());
    }
    #[cfg(windows)]
    unsafe {
        #[link(name = "user32")]
        extern "system" {
            fn FindWindowW(class: *const u16, title: *const u16) -> isize;
            fn PostMessageW(window: isize, message: u32, w: usize, l: isize) -> i32;
        }
        let name: Vec<u16> = crate::config()
            .window_title
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let hwnd = FindWindowW(std::ptr::null(), name.as_ptr());
        let scale = macroquad::window::screen_dpi_scale();
        let point = ((y * scale) as isize) << 16 | (x * scale) as isize;
        match frame {
            12 => {
                PostMessageW(hwnd, 0x0200, 0, point);
                PostMessageW(hwnd, 0x0201, 1, point);
            }
            14 => {
                PostMessageW(hwnd, 0x0202, 0, point);
            }
            18 => {
                PostMessageW(hwnd, 0x0100, 0x24, 0x147 << 16);
            } // Home
            20 => {
                PostMessageW(hwnd, 0x0101, 0x24, 0x147 << 16);
            }
            24 => {
                PostMessageW(hwnd, 0x0102, 'Z' as usize, 0);
            }
            44 => {
                PostMessageW(hwnd, 0x0100, 0x11, 0x1d << 16);
                // Miniquad uses the scan code. A zero virtual key prevents
                // TranslateMessage generating a spurious printable 'a': posted
                // messages do not alter Windows' real keyboard modifier state.
                PostMessageW(hwnd, 0x0100, 0, 0x1e << 16);
            } // Ctrl+A
            46 => {
                PostMessageW(hwnd, 0x0101, 0, 0x1e << 16);
                PostMessageW(hwnd, 0x0101, 0x11, 0x1d << 16);
            }
            50 => {
                for c in "My completely renamed game é".chars() {
                    PostMessageW(hwnd, 0x0102, c as usize, 0);
                }
            }
            _ => {}
        }
    }
    if frame == 40 {
        BEFORE.with(|before| {
            let expected = format!("Z{}", before.borrow());
            HOME_PASSED.with(|passed| *passed.borrow_mut() = title == expected);
        });
    }
    if frame == 70 {
        HOME_PASSED.with(|home| {
            let expected = "My completely renamed game é";
            let result = serde_json::json!({"expected": expected, "actual": title,
                "home_inserts_at_start": *home.borrow(), "select_all_replaces_full_title": title == expected,
                "passed": *home.borrow() && title == expected});
            std::fs::write(path, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
        });
    }
}
