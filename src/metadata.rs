use sha1::{Digest, Sha1};
use std::{collections::BTreeMap, fs, path::Path};

// SteamEdit managed IL: AppInfo_0x07564429.Save and AppInfoEntry.FromReader/Write.
const APPINFO_MAGIC: u32 = 0x07564429;
const HEADER_BYTES: usize = 16;
const RECORD_FIXED_BYTES: usize = 60;
const COMPLETED_SUFFIX: &str = " ✓"; // SteamData.get_CompletedSuffix, RVA 0xc2ad.

fn visible_title(title: &str, previous: &str) -> String {
    if previous.ends_with(COMPLETED_SUFFIX) && !title.ends_with(COMPLETED_SUFFIX) {
        format!("{title}{COMPLETED_SUFFIX}")
    } else {
        title.to_string()
    }
}

pub fn hash(b: &[u8]) -> String {
    format!("{:x}", Sha1::digest(b))
}
fn u32at(b: &[u8], p: usize) -> Result<u32, String> {
    b.get(p..p + 4)
        .map(|x| u32::from_le_bytes(x.try_into().unwrap()))
        .ok_or("Truncated metadata".into())
}
fn zstr(b: &[u8], p: &mut usize) -> Result<String, String> {
    let start = *p;
    let n = b
        .get(start..)
        .ok_or("Invalid offset")?
        .iter()
        .position(|v| *v == 0)
        .ok_or("Unterminated text")?;
    *p += n + 1;
    String::from_utf8(b[start..start + n].to_vec()).map_err(|_| "Invalid UTF-8".into())
}
#[derive(Clone, Debug)]
pub struct Prop {
    pub name: String,
    pub val: String,
    pub children: Vec<Prop>,
    pub span: Option<(usize, usize)>,
    pub kind: u8,
}
impl Prop {
    pub fn at(&self, path: &[&str]) -> Option<&Prop> {
        if path.is_empty() {
            return Some(self);
        }
        self.children
            .iter()
            .find(|x| x.name == path[0])?
            .at(&path[1..])
    }
    pub fn text(&self, path: &[&str]) -> String {
        self.at(path).map(|x| x.val.clone()).unwrap_or_default()
    }
}
fn table(b: &[u8], p: &mut usize, keys: &[String], depth: usize) -> Result<Vec<Prop>, String> {
    if depth > 32 {
        return Err("Nested metadata limit".into());
    }
    let mut out = Vec::new();
    loop {
        let kind = *b.get(*p).ok_or("Truncated table")?;
        *p += 1;
        if kind == 8 {
            break;
        }
        let ki = u32at(b, *p)? as usize;
        *p += 4;
        let name = keys.get(ki).ok_or("Bad property key")?.clone();
        let mut n = Prop {
            name,
            val: String::new(),
            children: vec![],
            span: None,
            kind,
        };
        match kind {
            0 => n.children = table(b, p, keys, depth + 1)?,
            1 => {
                let s = *p;
                n.val = zstr(b, p)?;
                n.span = Some((s, *p));
            }
            2 => {
                n.val = (u32at(b, *p)? as i32).to_string();
                *p += 4
            }
            3 => {
                n.val = f32::from_bits(u32at(b, *p)?).to_string();
                *p += 4
            }
            5 => {
                n.val = keys
                    .get(u32at(b, *p)? as usize)
                    .ok_or("Bad wide key")?
                    .clone();
                *p += 4
            }
            6 => return Err("Unsupported color property; no changes made".into()),
            7 => {
                let v = b.get(*p..*p + 8).ok_or("Truncated integer")?;
                n.val = u64::from_le_bytes(v.try_into().unwrap()).to_string();
                *p += 8
            }
            _ => return Err(format!("Unsupported property type {kind}")),
        }
        out.push(n);
    }
    Ok(out)
}
#[derive(Clone)]
pub struct Entry {
    pub id: u32,
    pub start: usize,
    pub end: usize,
    pub root: Prop,
}
pub struct Database {
    pub raw: Vec<u8>,
    pub entries: Vec<Entry>,
    pub table_offset: usize,
}
impl Database {
    pub fn read(path: &Path) -> Result<Self, String> {
        Self::parse(fs::read(path).map_err(|e| e.to_string())?)
    }
    pub fn parse(raw: Vec<u8>) -> Result<Self, String> {
        if u32at(&raw, 0)? != APPINFO_MAGIC {
            return Err(
                "This Steam appinfo version is unsupported. Nothing has been changed.".into(),
            );
        }
        let off =
            u64::from_le_bytes(raw.get(8..16).ok_or("Short header")?.try_into().unwrap()) as usize;
        let count = u32at(&raw, off)? as usize;
        if count > 1_000_000 {
            return Err("Invalid key table".into());
        }
        let mut p = off + 4;
        let mut keys = vec![];
        for _ in 0..count {
            keys.push(zstr(&raw, &mut p)?)
        }
        p = HEADER_BYTES;
        let mut entries = vec![];
        loop {
            let id = u32at(&raw, p)?;
            if id == 0 {
                break;
            }
            let size = u32at(&raw, p + 4)? as usize;
            let start = p;
            let end = p.checked_add(8 + size).ok_or("Bad entry size")?;
            if end > off || size < RECORD_FIXED_BYTES {
                return Err("Invalid entry boundary".into());
            }
            let mut q = p + 8 + RECORD_FIXED_BYTES;
            let children = table(&raw[..end], &mut q, &keys, 0)?;
            if q != end {
                return Err(format!("Unexpected trailing data for AppID {id}"));
            }
            entries.push(Entry {
                id,
                start,
                end,
                root: Prop {
                    name: String::new(),
                    val: String::new(),
                    span: None,
                    kind: 0,
                    children,
                },
            });
            p = end;
        }
        Ok(Self {
            raw,
            entries,
            table_offset: off,
        })
    }
    pub fn titles(&self) -> BTreeMap<u32, String> {
        self.entries
            .iter()
            .map(|e| (e.id, e.root.text(&["appinfo", "common", "name"])))
            .collect()
    }
    pub fn patch(&self, changes: &BTreeMap<u32, String>) -> Result<Vec<u8>, String> {
        let mut out = self.raw[..HEADER_BYTES].to_vec();
        let mut matched = 0;
        for e in &self.entries {
            if let Some(title) = changes.get(&e.id) {
                if title.trim().is_empty() || title.contains('\0') || title.len() > 1024 {
                    return Err("Title must be 1–1024 bytes with no nulls".into());
                }
                let mut root = e.root.clone();
                let mut edits = vec![];
                let old = root.text(&["appinfo", "common", "name"]);
                if old.is_empty() {
                    return Err(format!("AppID {} has no title", e.id));
                }
                let year = super::suffix_year(title);
                fn visit(
                    n: &mut Prop,
                    path: &str,
                    title: &str,
                    year: Option<u16>,
                    edits: &mut Vec<(usize, usize, String)>,
                ) {
                    let here = if path.is_empty() {
                        n.name.clone()
                    } else {
                        format!("{path}/{}", n.name)
                    };
                    let direct =
                        here == "appinfo/common/name" || here == "appinfo/steam_edit/base_name";
                    let localized = here.starts_with("appinfo/common/name_localized/")
                        || here.starts_with("appinfo/steam_edit/base_name_localized/");
                    if direct || localized {
                        if let Some((a, b)) = n.span {
                            let v = if direct {
                                if here == "appinfo/common/name" {
                                    visible_title(title, &n.val)
                                } else {
                                    title
                                        .strip_suffix(COMPLETED_SUFFIX)
                                        .unwrap_or(title)
                                        .to_string()
                                }
                            } else if let Some(y) = year {
                                super::append_year(&n.val, y)
                            } else {
                                n.val.clone()
                            };
                            if v != n.val {
                                n.val = v.clone();
                                edits.push((a, b, v));
                            }
                        }
                    }
                    for c in &mut n.children {
                        visit(c, &here, title, year, edits)
                    }
                }
                for n in &mut root.children {
                    visit(n, "", title, year, &mut edits)
                }
                let before = textual(&e.root);
                let expected = &self.raw[e.start + 8 + 16..e.start + 8 + 36];
                // Steam canonicalizes key casing for its text digest. Older editors
                // can instead hash the original casing; retain the input convention.
                let lowercase_keys = Sha1::digest(before.as_bytes()).as_slice() != expected;
                if lowercase_keys && Sha1::digest(textual_keys(&e.root, true).as_bytes()).as_slice() != expected {
                    return Err(format!(
                        "AppID {} has an unfamiliar text checksum. No file was written.",
                        e.id
                    ));
                }
                let mut payload = self.raw[e.start + 8..e.end].to_vec();
                edits.sort_by_key(|x| x.0);
                for (a, b, v) in edits.into_iter().rev() {
                    let mut bytes = v.into_bytes();
                    bytes.push(0);
                    payload.splice(a - e.start - 8..b - e.start - 8, bytes);
                }
                let text_hash = Sha1::digest(textual_keys(&root, lowercase_keys).as_bytes());
                payload[16..36].copy_from_slice(&text_hash);
                let binary = Sha1::digest(&payload[RECORD_FIXED_BYTES..]);
                payload[40..60].copy_from_slice(&binary);
                out.extend(e.id.to_le_bytes());
                out.extend((payload.len() as u32).to_le_bytes());
                out.extend(payload);
                matched += 1;
            } else {
                out.extend_from_slice(&self.raw[e.start..e.end]);
            }
        }
        if matched != changes.len() {
            return Err("One or more selected AppIDs are absent from metadata".into());
        }
        out.extend(0u32.to_le_bytes());
        let offset = out.len() as u64;
        out[8..16].copy_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&self.raw[self.table_offset..]);
        let verify = Self::parse(out.clone())?;
        for (id, title) in changes {
            let previous = self.titles().get(id).cloned().unwrap_or_default();
            if verify.titles().get(id) != Some(&visible_title(title, &previous)) {
                return Err("Post-write title validation failed".into());
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod recovered_rules_tests {
    use super::*;

    #[test]
    fn patches_steam_lowercase_key_checksum_with_mixed_case_keys() {
        let mut properties = Vec::new();
        properties.push(0);
        properties.extend(0u32.to_le_bytes()); // appinfo
        properties.push(0);
        properties.extend(1u32.to_le_bytes()); // common
        properties.push(1);
        properties.extend(2u32.to_le_bytes()); // name
        properties.extend(b"Game\0");
        properties.push(1);
        properties.extend(3u32.to_le_bytes()); // ReleaseState
        properties.extend(b"released\0");
        properties.extend([8, 8, 8]);
        let canonical = "\"appinfo\"\n{\n\t\"common\"\n\t{\n\t\t\"name\"\t\t\"Game\"\n\t\t\"releasestate\"\t\t\"released\"\n\t}\n}\n";
        let mut payload = vec![0; RECORD_FIXED_BYTES];
        payload[16..36].copy_from_slice(&Sha1::digest(canonical.as_bytes()));
        payload[40..60].copy_from_slice(&Sha1::digest(&properties));
        payload.extend(properties);
        let mut raw = Vec::new();
        raw.extend(APPINFO_MAGIC.to_le_bytes());
        raw.extend(0u32.to_le_bytes());
        raw.extend((HEADER_BYTES as u64 + 8 + payload.len() as u64 + 4).to_le_bytes());
        raw.extend(10u32.to_le_bytes());
        raw.extend((payload.len() as u32).to_le_bytes());
        raw.extend(payload);
        raw.extend(0u32.to_le_bytes());
        raw.extend(4u32.to_le_bytes());
        raw.extend(b"appinfo\0common\0name\0ReleaseState\0");
        let db = Database::parse(raw).unwrap();
        let patched = db.patch(&BTreeMap::from([(10, "Game (2001)".into())])).unwrap();
        let updated = Database::parse(patched).unwrap();
        assert_eq!(updated.titles()[&10], "Game (2001)");
        assert_eq!(updated.entries[0].root.text(&["appinfo", "common", "ReleaseState"]), "released");
        let expected = canonical.replace("Game", "Game (2001)");
        assert_eq!(&updated.raw[40..60], Sha1::digest(expected.as_bytes()).as_slice());
    }

    #[test]
    fn preserves_steamedit_completion_decoration() {
        assert_eq!(visible_title("Game (1998)", "Game ✓"), "Game (1998) ✓");
        assert_eq!(visible_title("Game (1998) ✓", "Game ✓"), "Game (1998) ✓");
        assert_eq!(visible_title("Game (1998)", "Game"), "Game (1998)");
    }
}
fn esc(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"")
}
pub fn textual(root: &Prop) -> String {
    textual_keys(root, false)
}
fn textual_keys(root: &Prop, lowercase_keys: bool) -> String {
    fn write(n: &Prop, d: usize, s: &mut String, lowercase_keys: bool) {
        let tab = "\t".repeat(d);
        s.push_str(&tab);
        s.push('"');
        s.push_str(&esc(&if lowercase_keys { n.name.to_lowercase() } else { n.name.clone() }));
        s.push('"');
        if n.kind == 0 {
            s.push('\n');
            s.push_str(&tab);
            s.push_str("{\n");
            for c in &n.children {
                write(c, d + 1, s, lowercase_keys)
            }
            s.push_str(&tab);
            s.push('}');
        } else {
            s.push_str("\t\t\"");
            s.push_str(&esc(&n.val));
            s.push('"');
        }
        s.push('\n');
    }
    let mut s = String::new();
    for n in &root.children {
        write(n, 0, &mut s, lowercase_keys)
    }
    s
}
