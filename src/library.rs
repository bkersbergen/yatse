use crate::{
    append_year,
    metadata::{hash, Database},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Row {
    pub id: u32,
    pub old: String,
    pub proposed: String,
    pub year: Option<u16>,
    pub source: String,
    pub kind: String,
    pub note: String,
    pub selected: bool,
    pub excluded: bool,
    pub approved: bool,
    pub installed: bool,
    pub existing: bool,
}
impl Row {
    pub fn ready(&self) -> bool {
        !self.excluded
            && (self.approved || (!self.existing && self.year.is_some() && self.note.is_empty()))
            && !self.proposed.trim().is_empty()
            && !self.proposed.chars().any(char::is_control)
            && self.proposed != self.old
    }
    pub fn status(&self) -> &str {
        if self.excluded {
            "Excluded"
        } else if self.ready() {
            "Ready"
        } else if self.existing {
            "Has year"
        } else if self.ready() {
            "Ready"
        } else {
            "Review"
        }
    }
}
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Saved {
    pub titles: BTreeMap<u32, String>,
    pub excluded: BTreeSet<u32>,
    pub approved: BTreeSet<u32>,
    pub years: BTreeMap<u32, u16>,
    pub kinds: BTreeMap<u32, String>,
}
#[derive(Clone)]
pub struct Library {
    pub path: PathBuf,
    pub account: String,
    pub rows: Vec<Row>,
    pub fingerprint: String,
    pub coverage: String,
}
pub fn fetch_owned(account: &str, key: &str) -> Result<BTreeMap<u32, String>, String> {
    if key.len() != 32 || !key.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Enter your 32-character Steam Web API key.".into());
    }
    let id = account
        .parse::<u64>()
        .map_err(|_| "Choose your account first")?
        + 76561197960265728;
    let v: serde_json::Value = ureq::get(
        "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/",
    )
    .query("key", key)
    .query("steamid", &id.to_string())
    .query("include_appinfo", "true")
    .query("include_played_free_games", "true")
    .query("skip_unvetted_apps", "false")
    .timeout(std::time::Duration::from_secs(20))
    .call()
    .map_err(|_| {
        "Steam ownership request failed. Check the key, connection and profile game privacy."
    })?
    .into_json()
    .map_err(|_| "Steam returned invalid ownership data")?;
    let games = v["response"]["games"]
        .as_array()
        .ok_or("Steam did not expose this account's library. Check game details privacy.")?;
    let mut out = BTreeMap::new();
    for g in games {
        if let Some(id) = g["appid"].as_u64() {
            out.insert(id as u32, g["name"].as_str().unwrap_or("").into());
        }
    }
    Ok(out)
}
pub fn state(dir: &Path) -> Saved {
    fs::read(dir.join("overrides.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
pub fn save(dir: &Path, rows: &[Row]) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut s = state(dir);
    for r in rows {
        if r.excluded {
            s.excluded.insert(r.id);
        } else {
            s.excluded.remove(&r.id);
        }
        if r.approved {
            s.approved.insert(r.id);
            s.years.remove(&r.id);
            s.years.extend(r.year.map(|y| (r.id, y)));
            s.titles.insert(r.id, r.proposed.clone());
            s.kinds.insert(r.id, r.kind.clone());
        } else if !r.existing {
            s.approved.remove(&r.id);
            s.years.remove(&r.id);
            s.titles.remove(&r.id);
            s.kinds.remove(&r.id);
        }
    }
    write_json(&dir.join("overrides.json"), &s)
}
pub fn write_json(path: &Path, v: &impl Serialize) -> Result<(), String> {
    fs::write(
        path,
        serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
pub fn detect() -> PathBuf {
    if let Some(p) = std::env::var_os("STEAM_PATH") {
        return p.into();
    }
    let o = std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"])
        .output();
    if let Ok(o) = o {
        let s = String::from_utf8_lossy(&o.stdout);
        if let Some(l) = s.lines().find(|x| x.contains("REG_SZ")) {
            return l.split("REG_SZ").nth(1).unwrap_or("").trim().into();
        }
    }
    PathBuf::from(r"C:\Program Files (x86)\Steam")
}
fn dirs(p: &Path) -> Vec<PathBuf> {
    fs::read_dir(p)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect()
}
pub fn accounts(root: &Path) -> Vec<String> {
    dirs(&root.join("userdata"))
        .iter()
        .filter(|p| p.join("config/librarycache").is_dir())
        .filter_map(|p| p.file_name()?.to_str().map(str::to_owned))
        .collect()
}
fn quoted(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut inside = false;
    let mut escape = false;
    for c in s.chars() {
        if escape {
            cur.push(c);
            escape = false;
        } else if inside && c == '\\' {
            escape = true;
        } else if c == '"' {
            if inside {
                out.push(cur.clone());
                cur.clear()
            }
            inside = !inside
        } else if inside {
            cur.push(c)
        }
    }
    out
}
pub fn discover(root: &Path, account: &str, dir: &Path) -> Result<Library, String> {
    discover_progress(root, account, dir, |_, _| {})
}
pub fn discover_progress(root: &Path, account: &str, dir: &Path, progress: impl Fn(u8, &'static str)) -> Result<Library, String> {
    progress(5, "Finding Steam accounts");
    let aa = accounts(root);
    let account = if !account.trim().is_empty() {
        account.to_string()
    } else if aa.len() == 1 {
        aa[0].clone()
    } else {
        return Err(format!(
            "Choose a Steam account ID. Found: {}",
            aa.join(", ")
        ));
    };
    if !aa.contains(&account) {
        return Err("That account has no local library cache".into());
    }
    progress(15, "Scanning installed games");
    let mut installed = BTreeSet::new();
    let mut libraries = vec![root.to_path_buf()];
    if let Ok(s) = fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
        let q = quoted(&s);
        for pair in q.windows(2) {
            if pair[0] == "path" {
                libraries.push(PathBuf::from(&pair[1]));
            }
        }
    }
    for p in libraries {
        for f in dirs(&p.join("steamapps")) {
            if let Some(s) = f.file_name().and_then(|s| s.to_str()) {
                if let Some(id) = s
                    .strip_prefix("appmanifest_")
                    .and_then(|x| x.strip_suffix(".acf"))
                    .and_then(|x| x.parse::<u32>().ok())
                {
                    installed.insert(id);
                }
            }
        }
    }
    progress(35, "Reading library ownership");
    let mut owned = BTreeSet::new();
    for f in dirs(
        &root
            .join("userdata")
            .join(&account)
            .join("config/librarycache"),
    ) {
        if f.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let (Some(id), Ok(b)) = (
            f.file_stem()
                .and_then(|x| x.to_str())
                .and_then(|x| x.parse::<u32>().ok()),
            fs::read(&f),
        ) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b) {
                if v.as_array()
                    .map(|a| {
                        a.iter().any(|p| {
                            p[0] == "friends" && p[1]["data"]["your_info"]["owned"] == true
                        })
                    })
                    .unwrap_or(false)
                {
                    owned.insert(id);
                }
            }
        }
    }
    let online: BTreeMap<u32, String> = fs::read(dir.join(format!("owned-{account}.json")))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    owned.extend(online.keys().copied());
    let ids: BTreeSet<u32> = owned.union(&installed).copied().collect();
    progress(60, "Reading Steam title database");
    let db = Database::read(&root.join("appcache/appinfo.vdf"))?;
    progress(85, "Preparing titles and saved edits");
    let saved = state(dir);
    let mut rows = vec![];
    for id in ids {
        let entry = db.entries.iter().find(|e| e.id == id);
        let name = entry
            .map(|e| e.root.text(&["appinfo", "common", "name"]))
            .unwrap_or_default();
        let typ = entry
            .map(|e| e.root.text(&["appinfo", "common", "type"]))
            .unwrap_or_default();
        if !typ.is_empty() && !typ.eq_ignore_ascii_case("game") {
            continue;
        }
        let old = if name.is_empty() {
            online
                .get(&id)
                .cloned()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("AppID {id} â€” metadata missing"))
        } else {
            name
        };
        let kind = classify(&old);
        let existing = crate::title_year(&old).is_some() || has_multi_year(&old);
        let mut year = None;
        let mut source = "No date available".to_string();
        let mut note = if entry.is_none() {
            "Refresh Steam's library metadata, then rescan.".into()
        } else {
            "Release year needs a source.".into()
        };
        if let Some(e) = entry {
            for (key, label, original) in [
                ("original_release_date", "Local  -  original release", true),
                ("steam_release_date", "Local  -  Steam release", false),
                ("release_date", "Local  -  release date", false),
            ] {
                let val = e.root.text(&["appinfo", "common", key]);
                if let Some(y) = year_value(&val) {
                    year = Some(y);
                    source = label.into();
                    note = if original && kind == "Original / standard" {
                        String::new()
                    } else {
                        "Confirm the original release year; this may be a later edition or PC launch.".into()
                    };
                    break;
                }
            }
        }
        if existing {
            year = crate::title_year(&old);
            source = "Existing title  -  preserved".into();
            note = if has_multi_year(&old) {
                "Several years in a shared title. Kept unchanged.".into()
            } else {
                String::new()
            };
        }
        let mut r = Row {
            id,
            proposed: year.map(|y| append_year(&old, y)).unwrap_or(old.clone()),
            old,
            year,
            source,
            kind,
            note,
            selected: false,
            excluded: saved.excluded.contains(&id),
            approved: false,
            installed: installed.contains(&id),
            existing,
        };
        if saved.approved.contains(&id) {
            if let Some(title) = saved.titles.get(&id) {
                r.proposed = title.clone();
                r.year = saved.years.get(&id).copied();
                r.approved = true;
                r.source = "Saved reviewed title".into();
                r.kind = saved.kinds.get(&id).cloned().unwrap_or(r.kind);
            }
        }
        r.selected = r.ready();
        rows.push(r);
    }
    rows.sort_by_key(|r| r.old.to_lowercase());
    progress(100, "Library ready");
    Ok(Library{path:root.into(),account,rows,fingerprint:hash(&db.raw),coverage:format!("{} cached owned AppIDs + {} installed AppIDs. Uncached purchases may be absent; sign in and let Steam refresh your library.",owned.len(),installed.len())})
}
pub fn classify(s: &str) -> String {
    let s = s.to_lowercase();
    if s.contains("launcher") || s.contains("mw2 2022") || s == "call of dutyÂ®" {
        "Shared launcher"
    } else if s.contains("remaster")
        || s.contains("definitive edition")
        || s.contains("enhanced edition")
    {
        "Remaster"
    } else if s.contains("remake") {
        "Remake"
    } else {
        "Unclassified"
    }
    .into()
}
pub fn has_multi_year(s: &str) -> bool {
    s.as_bytes()
        .windows(4)
        .filter(|w| w.iter().all(u8::is_ascii_digit))
        .filter(|w| {
            std::str::from_utf8(w)
                .ok()
                .and_then(|s| s.parse::<u16>().ok())
                .map(|y| (1970..=2100).contains(&y))
                .unwrap_or(false)
        })
        .count()
        > 1
}
pub fn year_value(s: &str) -> Option<u16> {
    if let Ok(n) = s.parse::<i64>() {
        if (1970..=2100).contains(&n) {
            return Some(n as u16);
        }
        if n > 0 {
            let days = n / 86400;
            let z = days + 719468;
            let era = z / 146097;
            let doe = z - era * 146097;
            let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
            let mut y = yoe + era * 400;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            if mp >= 10 {
                y += 1
            }
            if (1970..=2100).contains(&y) {
                return Some(y as u16);
            }
        }
    }
    s.as_bytes()
        .windows(4)
        .filter_map(|w| std::str::from_utf8(w).ok()?.parse::<u16>().ok())
        .find(|y| (1970..=2100).contains(y))
}
pub fn store_year(id: u32) -> Result<Option<u16>, String> {
    let url = format!("https://store.steampowered.com/api/appdetails?appids={id}&l=english");
    let response: serde_json::Value = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(12))
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    let item = &response[id.to_string()];
    if item["success"] != true {
        return Err("Store has no public details for this AppID".into());
    }
    if item["data"]["release_date"]["coming_soon"] == true {
        return Err("Upcoming release â€” no original year confirmed".into());
    }
    Ok(year_value(
        item["data"]["release_date"]["date"].as_str().unwrap_or(""),
    ))
}
#[derive(Serialize, Deserialize)]
pub struct Backup {
    pub original: String,
    pub before: String,
    pub after: String,
    pub snapshot: String,
    pub changes: BTreeMap<u32, String>,
    pub created: u64,
}
pub fn steam_running() -> bool {
    // Failure to enumerate processes must never permit a metadata write.
    crate::steam_guard::check().unwrap_or(true)
}
pub fn preview(lib: &Library) -> BTreeMap<u32, String> {
    lib.rows
        .iter()
        .filter(|r| r.selected && r.ready())
        .map(|r| (r.id, r.proposed.clone()))
        .collect()
}
pub fn apply(lib: &Library, dir: &Path, check_steam: bool) -> Result<String, String> {
    apply_titles(lib, dir, check_steam, preview(lib), true)
}
pub fn apply_titles(
    lib: &Library,
    dir: &Path,
    check_steam: bool,
    changes: BTreeMap<u32, String>,
    save_reviews: bool,
) -> Result<String, String> {
    if check_steam && steam_running() {
        return Err("Close Steam completely before applying. Your preview is saved.".into());
    }
    if changes.is_empty() {
        return Err("Select at least one reviewed change first.".into());
    }
    let path = lib.path.join("appcache/appinfo.vdf");
    let db = Database::read(&path)?;
    if hash(&db.raw) != lib.fingerprint {
        return Err("Steam metadata changed since scanning. Rescan before applying.".into());
    }
    let modified = db.patch(&changes)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let folder = dir.join("backups").join(now.to_string());
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let snap = folder.join("appinfo.vdf");
    fs::write(&snap, &db.raw).map_err(|e| e.to_string())?;
    if hash(&fs::read(&snap).map_err(|e| e.to_string())?) != lib.fingerprint {
        return Err("Backup verification failed".into());
    }
    let b = Backup {
        original: path.to_string_lossy().into(),
        before: lib.fingerprint.clone(),
        after: hash(&modified),
        snapshot: snap.to_string_lossy().into(),
        changes,
        created: now,
    };
    let manifest = folder.join("backup.json");
    write_json(&manifest, &b)?;
    if save_reviews {
        save(dir, &lib.rows)?;
    }
    let temp = path.with_extension("years.tmp");
    fs::write(&temp, &modified).map_err(|e| e.to_string())?;
    if hash(&fs::read(&path).map_err(|e| e.to_string())?) != b.before {
        return Err("Metadata changed during backup. Nothing applied.".into());
    }
    if check_steam && steam_running() {
        let _ = fs::remove_file(&temp);
        return Err("Steam started during this operation. Close Steam and try again. Nothing applied.".into());
    }
    write_json(&dir.join("latest-backup.json"), &b)?;
    replace(&path, &temp)?;
    Ok(format!(
        "Applied {} titles. Verified backup saved. Restart Steam to see them.",
        b.changes.len()
    ))
}
pub fn rollback(dir: &Path, check_steam: bool) -> Result<String, String> {
    if check_steam && steam_running() {
        return Err("Close Steam completely before rollback.".into());
    }
    let b: Backup = serde_json::from_slice(
        &fs::read(dir.join("latest-backup.json")).map_err(|_| "No backup exists yet")?,
    )
    .map_err(|e| e.to_string())?;
    let path = PathBuf::from(&b.original);
    let current = fs::read(&path).map_err(|e| e.to_string())?;
    if hash(&current) != b.after {
        return Err("Rollback paused: metadata has changed since applying. Restore only after reviewing the backup; unrelated updates will not be overwritten.".into());
    }
    let snap = fs::read(&b.snapshot).map_err(|e| e.to_string())?;
    if hash(&snap) != b.before {
        return Err("Backup checksum mismatch".into());
    }
    let temp = path.with_extension("years.tmp");
    fs::write(&temp, snap).map_err(|e| e.to_string())?;
    if hash(&fs::read(&path).map_err(|e| e.to_string())?) != b.after {
        return Err("Metadata changed during restore. Nothing restored.".into());
    }
    if check_steam && steam_running() {
        let _ = fs::remove_file(&temp);
        return Err("Steam started during this operation. Close Steam and try again. Nothing applied.".into());
    }
    replace(&path, &temp)?;
    Ok("Original metadata restored. Saved overrides remain available for reapply.".into())
}
fn replace(path: &Path, temp: &Path) -> Result<(), String> {
    fs::rename(temp, path).map_err(|e| e.to_string())
}

pub fn dry_run(lib: &Library, dir: &Path) -> Result<(), String> {
    let db = Database::read(&lib.path.join("appcache/appinfo.vdf"))?;
    if hash(&db.raw) != lib.fingerprint {
        return Err("Metadata changed. Rescan before dry run.".into());
    }
    let changes = preview(lib);
    db.patch(&changes)?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    write_json(
        &dir.join("dry-run.json"),
        &lib.rows
            .iter()
            .filter(|r| r.selected && r.ready())
            .collect::<Vec<_>>(),
    )
}

pub fn review_title(row: &mut Row, title: &str) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() || title.chars().any(char::is_control) {
        return Err("Enter a nonempty title without control characters.".into());
    }
    row.proposed = title.into();
    row.year = crate::suffix_year(title);
    row.approved = true;
    row.selected = true;
    row.source = "Manual title".into();
    Ok(())
}
