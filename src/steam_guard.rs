//! Native process enumeration; no shell or console is started.
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

pub fn check() -> Result<bool, String> {
    running_any(&["steam.exe", "steamwebhelper.exe"])
}

#[cfg(windows)]
fn running_any(names: &[&str]) -> Result<bool, String> {
    use std::mem::size_of;
    #[repr(C)]
    struct ProcessEntry {
        size: u32,
        usage: u32,
        pid: u32,
        heap: usize,
        module: u32,
        threads: u32,
        parent: u32,
        priority: i32,
        flags: u32,
        exe: [u16; 260],
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
        fn Process32FirstW(handle: isize, entry: *mut ProcessEntry) -> i32;
        fn Process32NextW(handle: isize, entry: *mut ProcessEntry) -> i32;
        fn CloseHandle(handle: isize) -> i32;
        fn GetLastError() -> u32;
    }
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(2, 0);
        if snapshot == -1 {
            return Err(format!(
                "Could not check running processes: {}",
                std::io::Error::last_os_error()
            ));
        }
        // Always close the snapshot, including error and early-return paths.
        struct Snapshot(isize);
        impl Drop for Snapshot {
            fn drop(&mut self) {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
        let snapshot = Snapshot(snapshot);
        let mut entry: ProcessEntry = std::mem::zeroed();
        entry.size = size_of::<ProcessEntry>() as u32;
        let mut found = Process32FirstW(snapshot.0, &mut entry);
        while found != 0 {
            let end = entry
                .exe
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.exe.len());
            let name = String::from_utf16_lossy(&entry.exe[..end]);
            if names.iter().any(|wanted| name.eq_ignore_ascii_case(wanted)) {
                return Ok(true);
            }
            found = Process32NextW(snapshot.0, &mut entry);
        }
        let error = GetLastError();
        if error != 18 {
            return Err(format!(
                "Process enumeration failed (Windows error {error})."
            ));
        }
        Ok(false)
    }
}
#[cfg(not(windows))]
fn running_any(_: &[&str]) -> Result<bool, String> {
    Err("Steam process checks require Windows.".into())
}

pub struct Monitor {
    updates: Receiver<Result<bool, String>>,
    state: Result<bool, String>,
}
impl Monitor {
    pub fn new() -> Self {
        let (sender, updates) = mpsc::channel();
        std::thread::Builder::new()
            .name("yatse-steam-check".into())
            .spawn(move || loop {
                if sender.send(check()).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
            })
            .expect("Could not start Steam monitor");
        Self {
            updates,
            state: Err("Checking whether Steam is running...".into()),
        }
    }
    pub fn blocked(&mut self) -> Option<String> {
        loop {
            match self.updates.try_recv() {
                Ok(update) => self.state = update,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.state =
                        Err("Steam monitoring stopped. Restart YATSE to continue safely.".into());
                    break;
                }
            }
        }
        match &self.state {
            Ok(false) => None,
            Ok(true) => Some("Steam is running. Choose Steam > Exit, including its system tray icon. YATSE will continue automatically once Steam has fully closed.".into()),
            Err(message) => Some(message.clone()),
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn native_enumeration_finds_own_process_and_rejects_absent_name() {
        let exe = std::env::current_exe().unwrap();
        let name = exe.file_name().unwrap().to_str().unwrap().to_uppercase();
        assert_eq!(running_any(&[&name]), Ok(true));
        assert_eq!(
            running_any(&["yatse-nonexistent-process-619437.exe"]),
            Ok(false)
        );
    }
    #[test]
    fn monitor_blocks_running_unknown_and_failed_checks() {
        let (sender, updates) = mpsc::channel();
        let mut monitor = Monitor {
            updates,
            state: Err("Checking".into()),
        };
        assert!(monitor.blocked().is_some());
        sender.send(Ok(false)).unwrap();
        assert!(monitor.blocked().is_none());
        sender.send(Ok(true)).unwrap();
        assert!(monitor.blocked().is_some());
        sender.send(Err("Access denied".into())).unwrap();
        assert_eq!(monitor.blocked().unwrap(), "Access denied");
        sender.send(Ok(false)).unwrap();
        assert!(monitor.blocked().is_none());
    }
}
