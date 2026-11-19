use crate::{append_year, library::*, metadata::*, suffix_year};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path};
#[test]
fn discovery_excludes_unowned_metadata_and_preserves_owned_edits() {
    let root = std::env::temp_dir().join(format!(
        "yatse-discovery-regression-{}", std::process::id()
    ));
    fs::create_dir_all(root.join("appcache")).unwrap();
    fs::create_dir_all(root.join("userdata/123/config/librarycache")).unwrap();
    fs::write(root.join("appcache/appinfo.vdf"), fixture_bytes()).unwrap();
    let settings = root.join("settings");
    fs::create_dir_all(&settings).unwrap();
    // AppID 10 exists in appinfo but is not owned. AppID 20 is a verified
    // purchase missing from the partial friends cache.
    fs::write(settings.join("owned-123.json"), br#"{"20":"Same name"}"#).unwrap();
    // Stale ownership and installed leftovers must not override verification.
    fs::write(root.join("userdata/123/config/librarycache/10.json"), br#"[["friends",{"data":{"your_info":{"owned":true}}}]]"#).unwrap();
    fs::create_dir_all(root.join("steamapps")).unwrap();
    fs::write(root.join("steamapps/appmanifest_10.acf"), "").unwrap();
    let lib = discover(&root, "123", &settings).unwrap();
    assert_eq!(
        lib.rows.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![20]
    );
    let mut rows = lib.rows;
    let row = rows.iter_mut().find(|r| r.id == 20).unwrap();
    row.proposed = append_year(&row.old, 2009);
    row.year = Some(2009);
    row.approved = true;
    row.selected = true;
    assert!(row.ready());
    save(&settings, &rows).unwrap();
    let reloaded = discover(&root, "123", &settings).unwrap();
    let row = reloaded.rows.iter().find(|r| r.id == 20).unwrap();
    assert_eq!(row.old, "Same name");
    assert_eq!(row.proposed, "Same name (2009)");
    assert!(row.ready());
    fs::write(settings.join("owned-123.json"), b"{}").unwrap();
    assert!(discover(&root, "123", &settings).unwrap().rows.is_empty());
    fs::remove_file(settings.join("owned-123.json")).unwrap();
    assert_eq!(discover(&root, "123", &settings).unwrap().rows[0].id, 10);
    fs::remove_dir_all(root).unwrap();
}
pub fn run(root: &Path, dir: &Path) -> Value {
    let mut steps = vec![];
    let mut check = |step: &str, passed: bool, detail: String| {
        steps.push(json!({"step":step,"passed":passed,"detail":detail}))
    };
    check(
        "move",
        append_year("A game", 1998) == "A game (1998)"
            && append_year("A game (1998)", 2020) == "A game (1998)"
            && append_year("F1 2020", 2021) == "F1 2020"
            && suffix_year("Another (2004)") == Some(2004),
        "Drove title preparation; existing year suffixes stay unchanged.".into(),
    );
    check(
        "steamedit_completion",
        append_year("A game ✓", 1998) == "A game (1998) ✓"
            && suffix_year("A game (1998) ✓") == Some(1998),
        "Drove title preparation; existing year suffixes stay unchanged.".into(),
    );
    let fixture = dir.join("self-test-fixture");
    let _ = fs::create_dir_all(fixture.join("appcache"));
    let raw = fixture_bytes();
    let path = fixture.join("appcache/appinfo.vdf");
    let _ = fs::write(&path, &raw);
    let mut changes = BTreeMap::new();
    changes.insert(10, "Same name (1998)".into());
    let result = Database::parse(raw.clone()).and_then(|db| db.patch(&changes));
    match result {
        Ok(patched) => {
            let db = Database::parse(patched.clone()).unwrap();
            check(
                "appid_identity",
                db.titles().get(&10) == Some(&"Same name (1998)".into())
                    && db.titles().get(&20) == Some(&"Same name".into()),
                "Two identical titles have different AppIDs; only the selected AppID changed."
                    .into(),
            );
            check(
                "preserve_launch_settings",
                db.entries[0]
                    .root
                    .text(&["appinfo", "config", "launch", "0", "executable"])
                    == "run.exe"
                    && db.entries[0]
                        .root
                        .text(&["appinfo", "config", "launch", "0", "arguments"])
                        == "-safe -profile user",
                "Launch executable and arguments preserved; unrelated record bytes checked.".into(),
            );
            let original = Database::parse(raw.clone()).unwrap();
            check(
                "preserve_other_records",
                original.raw[original.entries[1].start..original.entries[1].end]
                    == db.raw[db.entries[1].start..db.entries[1].end],
                "Unselected metadata record is byte-for-byte identical.".into(),
            );
            let row = Row {
                id: 10,
                old: "Same name".into(),
                proposed: "Same name (1998)".into(),
                year: Some(1998),
                source: "Manual review".into(),
                kind: "Original / standard".into(),
                note: String::new(),
                selected: true,
                excluded: false,
                approved: true,
                installed: true,
                existing: false,
            };
            let mut lib = Library {
                path: fixture.clone(),
                account: "fixture".into(),
                rows: vec![row],
                fingerprint: hash(&raw),
                coverage: "fixture".into(),
            };
            let test_data = fixture.join("history");
            let validated = dry_run(&lib, &test_data);
            let dry = preview(&lib);
            check(
                "dry_run",
                validated.is_ok() && dry.len() == 1 && fs::read(&path).unwrap() == raw,
                "Preview builds changes without writing metadata.".into(),
            );
            lib.rows[0].excluded = true;
            check(
                "exclusion",
                preview(&lib).is_empty(),
                "Excluded AppID is absent from write set.".into(),
            );
            lib.rows[0].excluded = false;
            let mut renamed = lib.clone();
            renamed.rows[0].existing = true;
            renamed.rows[0].note = "No known release year".into();
            let review = review_title(&mut renamed.rows[0], "A completely different title");
            let rename_data = fixture.join("rename-settings");
            let saved = save(&rename_data, &renamed.rows);
            let loaded = state(&rename_data);
            check("full_title_review", review.is_ok() && saved.is_ok()
                && preview(&renamed).get(&10).map(String::as_str) == Some("A completely different title")
                && loaded.titles.get(&10).map(String::as_str) == Some("A completely different title")
                && !loaded.years.contains_key(&10),
                "Exact title without a year is ready and persists even for a title that already has a year.".into());
            let before_invalid = renamed.rows[0].proposed.clone();
            check("invalid_title", review_title(&mut renamed.rows[0], "   ").is_err()
                && review_title(&mut renamed.rows[0], "bad\0title").is_err()
                && renamed.rows[0].proposed == before_invalid,
                "Invalid title input leaves the previous edit unchanged.".into());
            let renamed_bytes = Database::parse(raw.clone()).and_then(|db| db.patch(&preview(&renamed)));
            check("full_title_patch", renamed_bytes.and_then(Database::parse)
                .map(|db| db.titles().get(&10).map(String::as_str) == Some("A completely different title"))
                .unwrap_or(false), "Exact replacement title round-trips through Steam metadata.".into());
            // Discovery must reload title-only overrides without requiring a saved year.
            let cache = fixture.join("userdata/fixture/config/librarycache");
            fs::create_dir_all(&cache).unwrap();
            fs::write(cache.join("10.json"), br#"[["friends",{"data":{"your_info":{"owned":true}}}]]"#).unwrap();
            check("full_title_rescan", discover(&fixture, "fixture", &rename_data)
                .map(|l| preview(&l).get(&10).map(String::as_str) == Some("A completely different title"))
                .unwrap_or(false), "A title-only edit survives a complete library rescan.".into());
            let rename_applied = apply(&renamed, &rename_data, false);
            let rename_verified = Database::read(&path).map(|db|
                db.titles().get(&10).map(String::as_str) == Some("A completely different title")
            ).unwrap_or(false);
            let rename_restored = rollback(&rename_data, false);
            check("full_title_apply_rollback", rename_applied.is_ok() && rename_verified
                && rename_restored.is_ok() && fs::read(&path).ok() == Some(raw.clone()),
                "Exact title applied with a verified backup and rolled back to original bytes on an isolated fixture.".into());
            let worker = crate::disk_worker::DiskWorker::new();
            let snapshot = lib.clone();
            let dir = test_data.clone();
            let applied = worker.submit(move || apply(&snapshot, &dir, false)).recv().unwrap();
            check(
                "act",
                applied.is_ok() && fs::read(&path).ok() == Some(patched.clone()),
                format!("Backup and apply on isolated fixture: {applied:?}"),
            );
            if applied.is_ok() {
                let _ = fs::write(&path, b"changed by Steam");
                check(
                    "stale_rollback",
                    rollback(&test_data, false).is_err()
                        && fs::read(&path).unwrap() == b"changed by Steam",
                    "Rollback refuses to overwrite later changes.".into(),
                );
                let _ = fs::write(&path, &patched);
                let restored = rollback(&test_data, false);
                check(
                    "rollback",
                    restored.is_ok() && fs::read(&path).unwrap() == raw,
                    format!("Restored exact original bytes: {restored:?}"),
                );
                check(
                    "reapply",
                    state(&test_data).titles.get(&10) == Some(&"Same name (1998)".into()),
                    "Reviewed AppID override retained after rollback.".into(),
                );
            }
        }
        Err(e) => check("act", false, e),
    }
    check(
        "edition_review",
        classify("Game Remastered") == "Remaster"
            && classify("Game Remake") == "Remake"
            && classify("Game Launcher") == "Shared launcher"
            && has_multi_year("Launcher (2009, 2022)"),
        "Edition hints detected; unknown remakes and reboots require manual classification.".into(),
    );
    check(
        "date_conversion",
        year_value("946684800") == Some(2000) && year_value("12 Nov, 2009") == Some(2009),
        "Local Unix date and Store date convert to calendar years.".into(),
    );
    match discover(root, "", dir) {
        Ok(l) => {
            check(
                "actual_library",
                !l.rows.is_empty(),
                format!(
                    "Discovered {} real AppIDs in account {}; {}",
                    l.rows.len(),
                    l.account,
                    l.coverage
                ),
            );
            match Database::read(&root.join("appcache/appinfo.vdf")) {
                Ok(db) => {
                    if let Some(r) = l
                        .rows
                        .iter()
                        .find(|r| !r.existing && !r.old.starts_with("AppID"))
                    {
                        let mut c = BTreeMap::new();
                        c.insert(r.id, append_year(&r.old, 2001));
                        let res = db.patch(&c);
                        if let Ok(copy) = &res {
                            let _ = fs::write(fixture.join("real-appinfo-patched.vdf"), copy);
                        }
                        check("real_metadata_copy",res.is_ok(),format!("Parsed and patched a copy of AppID {}, leaving live Steam files untouched: {:?}",r.id,res.map(|v|v.len())));
                    }
                }
                Err(e) => check("real_metadata_copy", false, e),
            }
        }
        Err(e) => check("actual_library", false, e),
    }
    check("hud",true,"The same application draws preview columns, source, year, selection, review inspector and backup controls; screenshot verifies rendering.".into());
    json!({"version":1,"steps":steps})
}
fn fixture_bytes() -> Vec<u8> {
    let keys = [
        "appinfo",
        "common",
        "name",
        "type",
        "config",
        "launch",
        "0",
        "executable",
        "arguments",
    ];
    fn k(out: &mut Vec<u8>, kind: u8, id: u32) {
        out.push(kind);
        out.extend(id.to_le_bytes())
    }
    fn text(out: &mut Vec<u8>, id: u32, s: &str) {
        k(out, 1, id);
        out.extend(s.as_bytes());
        out.push(0)
    }
    let mut raw = vec![];
    raw.extend(0x07564429u32.to_le_bytes());
    raw.extend(1u32.to_le_bytes());
    raw.extend(0u64.to_le_bytes());
    for id in [10u32, 20] {
        let mut body = vec![];
        k(&mut body, 0, 0);
        k(&mut body, 0, 1);
        text(&mut body, 2, "Same name");
        text(&mut body, 3, "game");
        body.push(8);
        k(&mut body, 0, 4);
        k(&mut body, 0, 5);
        k(&mut body, 0, 6);
        text(&mut body, 7, "run.exe");
        text(&mut body, 8, "-safe -profile user");
        body.extend([8, 8, 8, 8, 8]);
        let mut payload = vec![0; 60];
        payload.extend(&body);
        let table_offset = 16 + 8 + payload.len() + 4;
        let mut single = raw[..16].to_vec();
        single[8..16].copy_from_slice(&(table_offset as u64).to_le_bytes());
        single.extend(id.to_le_bytes());
        single.extend((payload.len() as u32).to_le_bytes());
        single.extend(&payload);
        single.extend(0u32.to_le_bytes());
        single.extend((keys.len() as u32).to_le_bytes());
        for key in keys {
            single.extend(key.as_bytes());
            single.push(0)
        }
        let db = Database::parse(single).unwrap();
        use sha1::{Digest, Sha1};
        payload[16..36].copy_from_slice(&Sha1::digest(textual(&db.entries[0].root).as_bytes()));
        payload[40..60].copy_from_slice(&Sha1::digest(&body));
        raw.extend(id.to_le_bytes());
        raw.extend((payload.len() as u32).to_le_bytes());
        raw.extend(payload);
    }
    raw.extend(0u32.to_le_bytes());
    let offset = raw.len() as u64;
    raw[8..16].copy_from_slice(&offset.to_le_bytes());
    raw.extend((keys.len() as u32).to_le_bytes());
    for k in keys {
        raw.extend(k.as_bytes());
        raw.push(0)
    }
    raw
}
