//! Read-only ownership queries through the signed-in Steam client.
//! ABI: ValveSoftware/Proton, steamworks_sdk_152/{isteamclient,isteamapps,isteamuser}.h.
use std::{collections::BTreeSet, path::Path};

#[cfg(all(windows, target_arch = "x86_64"))]
pub fn query(root: &Path, account: &str, ids: &[u32]) -> Result<BTreeSet<u32>, String> {
    use std::{collections::BTreeMap, ffi::c_void, os::windows::ffi::OsStrExt, sync::{Mutex, OnceLock}};
    type Object = *mut c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryExW(path: *const u16, file: Object, flags: u32) -> Object;
        fn GetProcAddress(module: Object, name: *const u8) -> Object;
    }
    // Steam interfaces are not thread safe. Keep DLLs loaded for the process
    // lifetime because the client owns background threads and thread-local data.
    static MODULES: OnceLock<Mutex<BTreeMap<std::path::PathBuf, usize>>> = OnceLock::new();
    let mut modules = MODULES.get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock().map_err(|_| "Steam ownership reader stopped")?;
    let path = root.join("steamclient64.dll").canonicalize()
        .map_err(|_| "Steam client DLL not found")?;
    let module = if let Some(module) = modules.get(&path) {
        *module as Object
    } else {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // Resolve dependencies only beside Steam's DLL and in default DLL directories.
        let module = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), 0x1100) };
        if module.is_null() { return Err("Could not load Steam's ownership interface".into()); }
        modules.insert(path, module as usize);
        module
    };
    // These slots are fixed by the explicitly requested interface versions.
    unsafe fn slot(object: Object, index: usize) -> Object {
        *(*(object as *const *const Object)).add(index)
    }
    struct Session { client: Object, pipe: i32, user: i32 }
    impl Drop for Session {
        fn drop(&mut self) {
            unsafe {
                if self.user != 0 {
                    let release: unsafe extern "C" fn(Object, i32, i32) = std::mem::transmute(slot(self.client, 4));
                    release(self.client, self.pipe, self.user);
                }
                let release: unsafe extern "C" fn(Object, i32) -> bool = std::mem::transmute(slot(self.client, 1));
                release(self.client, self.pipe);
            }
        }
    }
    unsafe {
        let export = GetProcAddress(module, b"CreateInterface\0".as_ptr());
        if export.is_null() { return Err("Steam interface factory unavailable".into()); }
        let factory: unsafe extern "C" fn(*const u8, Object) -> Object = std::mem::transmute(export);
        let client = factory(b"SteamClient020\0".as_ptr(), std::ptr::null_mut());
        if client.is_null() { return Err("SteamClient020 is unavailable".into()); }
        let create: unsafe extern "C" fn(Object) -> i32 = std::mem::transmute(slot(client, 0));
        let pipe = create(client);
        if pipe == 0 { return Err("Open Steam and sign in to refresh ownership".into()); }
        let mut session = Session { client, pipe, user: 0 };
        let connect: unsafe extern "C" fn(Object, i32) -> i32 = std::mem::transmute(slot(client, 2));
        session.user = connect(client, pipe);
        if session.user == 0 { return Err("Steam has no signed-in user".into()); }
        type Interface = unsafe extern "C" fn(Object, i32, i32, *const u8) -> Object;
        let get_user: Interface = std::mem::transmute(slot(client, 5));
        let user = get_user(client, session.user, pipe, b"SteamUser021\0".as_ptr());
        if user.is_null() { return Err("Steam user interface unavailable".into()); }
        let logged_on: unsafe extern "C" fn(Object) -> bool = std::mem::transmute(slot(user, 1));
        if !logged_on(user) { return Err("Sign in to Steam before refreshing ownership".into()); }
        // MSVC returns the CSteamID class through a hidden output pointer,
        // rather than as a plain u64. Check account identity before any queries.
        let get_id: unsafe extern "C" fn(Object, *mut u64) -> Object = std::mem::transmute(slot(user, 2));
        let mut steam_id = 0u64;
        get_id(user, &mut steam_id);
        validate_account(account, steam_id)?;
        let get_apps: Interface = std::mem::transmute(slot(client, 15));
        let apps = get_apps(client, session.user, pipe, b"STEAMAPPS_INTERFACE_VERSION008\0".as_ptr());
        if apps.is_null() { return Err("Steam apps interface unavailable".into()); }
        let subscribed: unsafe extern "C" fn(Object, u32) -> bool = std::mem::transmute(slot(apps, 6));
        Ok(ids.iter().copied().filter(|id| subscribed(apps, *id)).collect())
    }
}

fn validate_account(account: &str, steam_id: u64) -> Result<(), String> {
    if account.parse::<u32>().ok() != Some(steam_id as u32) || steam_id == 0 {
        return Err("Steam is signed in to a different account; ownership was not imported".into());
    }
    Ok(())
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
pub fn query(_: &Path, _: &str, _: &[u32]) -> Result<BTreeSet<u32>, String> {
    Err("Live Steam ownership requires 64-bit Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ownership_must_belong_to_selected_account() {
        assert!(validate_account("123", 76561197960265728 + 123).is_ok());
        assert!(validate_account("456", 76561197960265728 + 123).is_err());
        assert!(validate_account("0", 0).is_err());
        assert!(validate_account("invalid", 76561197960265728 + 123).is_err());
    }
}
