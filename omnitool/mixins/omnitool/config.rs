// Java's ConfigMerge.java and ConfigFiles.java: merging what Omnitool knows
// now into the file (never losing what this build doesn't understand), and
// crash-safe JSON files (write to .tmp then move, one .bak per session, a
// broken file is set aside as .broken-<time>). Plus the profile file names.
// What goes into config.json is core.rs. Added to the game as
// `game::omnitool_config`; also a module of this crate, checked against
// tests/fixtures/core-config-merge.json and core-profiles.json.

use super::omnitool_json::{Json, Obj};
use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Java's `ConfigMerge.merge(file, atLoad, now)`.
pub fn merge(file: Option<&Obj>, at_load: Option<&Obj>, now: &Obj) -> Obj {
    let mut out = keep_unknown(file, now);
    if let Some(nm) = now.obj("modules") {
        let fm = file.and_then(|f| f.obj("modules"));
        let lm = at_load.and_then(|l| l.obj("modules"));
        let mut mods = Obj::new();
        for (id, v) in nm.iter() {
            match v {
                Json::Obj(n) => {
                    let m = merge_module(fm.and_then(|f| f.obj(id)), lm.and_then(|l| l.obj(id)), n);
                    mods.insert(id, Json::Obj(m));
                }
                v => mods.insert(id, v.clone()),
            }
        }
        add_missing(&mut mods, fm);
        out.insert("modules", Json::Obj(mods));
    }
    if let Some(nh) = now.obj("hud") {
        out.insert(
            "hud",
            Json::Obj(keep_unknown(file.and_then(|f| f.obj("hud")), nh)),
        );
    }
    out
}

fn merge_module(f: Option<&Obj>, l: Option<&Obj>, n: &Obj) -> Obj {
    let mut out = Obj::new();
    for (k, v) in n.iter() {
        match v {
            Json::Obj(ns) if k == "settings" => {
                let (fs, ls) = (f.and_then(|f| f.obj(k)), l.and_then(|l| l.obj(k)));
                let mut settings = Obj::new();
                for (id, _) in ns.iter() {
                    settings.insert(id, leaf(fs, ls, ns, id));
                }
                add_missing(&mut settings, fs);
                out.insert(k, Json::Obj(settings));
            }
            _ => out.insert(k, leaf(f, l, n, k)),
        }
    }
    add_missing(&mut out, f);
    out
}

/// Unchanged since load (now == atLoad) -> the file's value, else the current one.
fn leaf(f: Option<&Obj>, l: Option<&Obj>, n: &Obj, key: &str) -> Json {
    let nv = n.get(key).cloned().unwrap_or(Json::Null);
    match (f.and_then(|f| f.get(key)), l.and_then(|l| l.get(key))) {
        (Some(fv), Some(lv)) if nv == *lv => fv.clone(),
        _ => nv,
    }
}

/// All keys of `now`, plus those only the file has.
pub fn keep_unknown(file: Option<&Obj>, now: &Obj) -> Obj {
    let mut out = now.clone();
    add_missing(&mut out, file);
    out
}

fn add_missing(out: &mut Obj, from: Option<&Obj>) {
    for (k, v) in from
        .map(|f| f.iter().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        if !out.has(k) {
            out.insert(k, v.clone());
        }
    }
}

thread_local! {
    /// Files already backed up in this session (Java's `BACKED_UP`).
    static BACKED_UP: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());
}

/// For tests: a new session.
#[cfg(test)]
pub fn reset_session() {
    BACKED_UP.with(|b| b.borrow_mut().clear());
}

fn sibling(file: &Path, suffix: &str) -> PathBuf {
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    file.with_file_name(name)
}

pub fn backup_of(file: &Path) -> PathBuf {
    sibling(file, ".bak")
}

/// The file as a JSON object, or why not.
pub fn parse(file: &Path) -> Result<Obj, String> {
    let text = fs::read_to_string(file).map_err(|e| e.to_string())?;
    match Json::parse(&text) {
        Ok(Json::Obj(o)) => Ok(o),
        Ok(_) => Err("not a JSON object (empty or truncated file?)".into()),
        Err(_) if text.trim().is_empty() => {
            Err("not a JSON object (empty or truncated file?)".into())
        }
        Err(e) => Err(e),
    }
}

/// Java's `ConfigFiles.read`: None if missing; a broken file is moved to
/// `.broken-<time>` and its `.bak` used (and restored) instead.
pub fn read(file: &Path) -> Option<Obj> {
    if !file.exists() {
        return None;
    }
    match parse(file) {
        Ok(o) => Some(o),
        Err(e) => {
            let moved = set_aside(file, true);
            log(&format!(
                "Could not read {} - kept it as {:?}: {e}",
                file.display(),
                moved
            ));
            let bak = backup_of(file);
            if !bak.exists() {
                return None;
            }
            match parse(&bak) {
                Ok(o) => {
                    if !file.exists() {
                        let _ = fs::copy(&bak, file);
                    }
                    log(&format!(
                        "Restored {} from the backup {}",
                        file.display(),
                        bak.display()
                    ));
                    Some(o)
                }
                Err(e) => {
                    log(&format!("Backup {} is broken too: {e}", bak.display()));
                    None
                }
            }
        }
    }
}

/// Java's `ConfigFiles.write`: pretty JSON to `.tmp` (synced), the old file
/// backed up once per session, then moved over it. Whether it worked.
pub fn write(file: &Path, json: &Obj) -> bool {
    let result = (|| -> std::io::Result<()> {
        if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir)?;
        }
        let tmp = sibling(file, ".tmp");
        let mut f = fs::File::create(&tmp)?;
        f.write_all(Json::Obj(json.clone()).pretty().as_bytes())?;
        f.sync_all()?;
        drop(f);
        let key = std::path::absolute(file).unwrap_or_else(|_| file.to_owned());
        if file.exists() && BACKED_UP.with(|b| b.borrow_mut().insert(key)) {
            backup(file);
        }
        // rename replaces atomically on the same file system
        fs::rename(&tmp, file)
    })();
    if let Err(e) = &result {
        log(&format!("Could not write {}: {e}", file.display()));
    }
    result.is_ok()
}

/// Copies the file to `.bak` if it is valid, else sets a copy aside.
fn backup(file: &Path) {
    if parse(file).is_ok() {
        let _ = fs::copy(file, backup_of(file));
    } else {
        set_aside(file, false);
    }
}

/// Java's `setAside`: keeps the file as `<file>.broken-<yyyyMMdd-HHmmss>`
/// (`-2`, `-3`, ... on a clash), moved or copied. The new path, or None.
pub fn set_aside(file: &Path, moved: bool) -> Option<PathBuf> {
    if !file.exists() {
        return None;
    }
    let stamp = timestamp();
    let mut target = sibling(file, &format!(".broken-{stamp}"));
    let mut i = 2;
    while target.exists() {
        target = sibling(file, &format!(".broken-{stamp}-{i}"));
        i += 1;
    }
    let result = if moved {
        fs::rename(file, &target)
    } else {
        fs::copy(file, &target).map(|_| ())
    };
    match result {
        Ok(()) => Some(target),
        Err(e) => {
            log(&format!("Could not keep a copy of {}: {e}", file.display()));
            None
        }
    }
}

/// `yyyyMMdd-HHmmss`. std has no time zones, so UTC (Java: local time).
pub fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // civil from days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// Log lines go to stderr (Horizon's log), as Java's `LOG.error/warn`.
pub fn log(msg: &str) {
    eprintln!("[Omnitool] {msg}");
}

/// Java's `Profiles.fileName`: trimmed, everything but `A-Za-z0-9_- ` -> `_`.
pub fn profile_file(name: &str) -> String {
    let clean: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_- ".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{clean}.json")
}

/// Java's `Profiles.list`: the `*.json` names in `dir`, sorted (code points).
pub fn profiles(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter_map(|n| n.strip_suffix(".json").map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A new empty folder under the system's temp folder.
    pub fn temp_dir() -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "omnitool-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixture(name: &str) -> Value {
        let path = format!("{}/../tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn obj(v: &Value) -> Option<Obj> {
        match Json::parse(&v.to_string()).unwrap() {
            Json::Obj(o) => Some(o),
            _ => None,
        }
    }

    fn at<'a>(o: &'a Obj, path: &str) -> Option<&'a Json> {
        let mut parts = path.split('.');
        let mut cur = o.get(parts.next()?)?;
        for p in parts {
            cur = cur.as_obj()?.get(p)?;
        }
        Some(cur)
    }

    #[test]
    fn merge_fixture() {
        for c in fixture("core-config-merge.json")["merge"]
            .as_array()
            .unwrap()
        {
            let (file, at_load) = (obj(&c["file"]), obj(&c["atLoad"]));
            let out = merge(file.as_ref(), at_load.as_ref(), &obj(&c["now"]).unwrap());
            if let Some(e) = c.get("expected") {
                assert_eq!(
                    Json::Obj(out.clone()),
                    Json::Obj(obj(e).unwrap()),
                    "{}",
                    c["name"]
                );
            }
            for (path, want) in c
                .get("expect_contains")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                let want = Json::parse(&want.to_string()).unwrap();
                assert_eq!(at(&out, path), Some(&want), "{}: {path}", c["name"]);
            }
            if let Some(h) = c.get("expect_hud") {
                assert_eq!(
                    out.get("hud"),
                    Some(&Json::Obj(obj(h).unwrap())),
                    "{}",
                    c["name"]
                );
            }
        }
    }

    fn v(n: i64) -> Obj {
        let mut o = Obj::new();
        o.insert("v", Json::Int(n));
        o
    }

    fn broken(dir: &Path) -> usize {
        fs::read_dir(dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains(".broken-")
            })
            .count()
    }

    #[test]
    fn atomic_write_and_one_backup_per_session() {
        let dir = temp_dir();
        let file = dir.join("config.json");
        reset_session();
        assert!(write(&file, &v(1)));
        assert!(!backup_of(&file).exists());
        reset_session();
        assert!(write(&file, &v(2)));
        assert!(write(&file, &v(3)));
        assert_eq!(parse(&backup_of(&file)).unwrap(), v(1));
        assert_eq!(parse(&file).unwrap(), v(3));
        assert!(!dir.join("config.json.tmp").exists());
    }

    #[test]
    fn broken_file_kept_and_backup_restored() {
        let dir = temp_dir();
        let file = dir.join("config.json");
        fs::write(backup_of(&file), "{\"v\":7}").unwrap();
        fs::write(&file, "{\"v\":8, \"modules\": {").unwrap();
        assert_eq!(read(&file), Some(v(7)));
        assert_eq!(broken(&dir), 1);
        assert_eq!(parse(&file).unwrap(), v(7));
    }

    #[test]
    fn broken_file_without_backup() {
        let dir = temp_dir();
        let file = dir.join("waypoints.json");
        fs::write(&file, "").unwrap();
        assert_eq!(read(&file), None);
        assert_eq!(broken(&dir), 1);
        assert!(write(&file, &Obj::new()));
        assert_eq!(broken(&dir), 1);
    }

    #[test]
    fn broken_file_not_overwritten_without_copy() {
        let dir = temp_dir();
        let file = dir.join("overlay.json");
        fs::write(&file, "not json").unwrap();
        let mut a = Obj::new();
        a.insert("a", Json::Int(1));
        assert!(write(&file, &a));
        assert_eq!(broken(&dir), 1);
        assert!(!backup_of(&file).exists());
        // a clash gets -2, -3, ...
        let dir = temp_dir();
        let file = dir.join("x.json");
        fs::write(&file, "x").unwrap();
        let first = set_aside(&file, false)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let second = set_aside(&file, false)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        // unless the second ticked over in between
        if second[..second.len().min(first.len())] == first[..] {
            assert_eq!(second, format!("{first}-2"));
        }
    }

    #[test]
    fn profile_files() {
        let fx = fixture("core-profiles.json");
        for c in fx["file_name"].as_array().unwrap() {
            assert_eq!(
                profile_file(c["name"].as_str().unwrap()),
                c["file"].as_str().unwrap()
            );
        }
        let dir = temp_dir();
        for f in fx["list_order"]["files"].as_array().unwrap() {
            fs::write(dir.join(f.as_str().unwrap()), "{}").unwrap();
        }
        let want: Vec<String> =
            serde_json::from_value(fx["list_order"]["expected"].clone()).unwrap();
        // a case-insensitive file system keeps only one of b.json / B.json
        let got = profiles(&dir);
        if got.len() == want.len() {
            assert_eq!(got, want);
        }
        assert!(profiles(&dir.join("missing")).is_empty());
    }

    #[test]
    fn timestamp_format() {
        let t = timestamp();
        assert_eq!(t.len(), 15);
        assert_eq!(&t[8..9], "-");
        assert!(t.starts_with("20"));
    }
}
