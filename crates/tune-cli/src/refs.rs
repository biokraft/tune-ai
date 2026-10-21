//! Reference tracks that live outside git: named in `tune.local.toml`
//! (`[refs]`) or dropped into a `refs/` folder next to it.

use std::path::{Path, PathBuf};

/// Where a reference was declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefSource {
    Config,
    RefsDir,
}

/// One named reference track.
#[derive(Clone, Debug, PartialEq)]
pub struct RefEntry {
    pub name: String,
    /// The path as written in the config (or the file name in `refs/`).
    pub configured: String,
    /// The path after `~` and root expansion.
    pub path: PathBuf,
    pub exists: bool,
    pub from: RefSource,
}

/// The references visible from a directory.
#[derive(Clone, Debug, PartialEq)]
pub struct Refs {
    /// Nearest ancestor holding `tune.local.toml` or `refs/`.
    pub root: Option<PathBuf>,
    /// The `tune.local.toml` in the root, when there is one.
    pub config: Option<PathBuf>,
    pub entries: Vec<RefEntry>,
}

const CONFIG: &str = "tune.local.toml";
const AUDIO_EXTS: [&str; 5] = ["wav", "mp3", "flac", "m4a", "aac"];

/// Finds the references visible from `cwd`: the nearest ancestor (inclusive)
/// holding `tune.local.toml` or `refs/` is the root. `home` expands `~/`.
pub fn find_refs(cwd: &Path, home: Option<&Path>) -> Result<Refs, String> {
    let Some(root) = cwd
        .ancestors()
        .find(|d| d.join(CONFIG).is_file() || d.join("refs").is_dir())
    else {
        return Ok(Refs {
            root: None,
            config: None,
            entries: Vec::new(),
        });
    };
    let config_path = root.join(CONFIG);
    let config = config_path.is_file().then_some(config_path);
    let mut entries = Vec::new();
    if let Some(path) = &config {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{CONFIG}: {e}"))?;
        let table: toml::Table = text.parse().map_err(|e| format!("{CONFIG}: {e}"))?;
        if let Some(refs) = table.get("refs") {
            let refs = refs
                .as_table()
                .ok_or_else(|| format!("{CONFIG}: refs must be a table"))?;
            for (name, value) in refs {
                let configured = value
                    .as_str()
                    .ok_or_else(|| format!("{CONFIG}: refs.{name} must be a path string"))?;
                let path = expand(configured, root, home);
                entries.push(RefEntry {
                    name: name.clone(),
                    configured: configured.to_string(),
                    exists: path.exists(),
                    path,
                    from: RefSource::Config,
                });
            }
        }
    }
    let mut found: Vec<(String, String, PathBuf)> = Vec::new();
    if let Ok(dir) = std::fs::read_dir(root.join("refs")) {
        for item in dir.flatten() {
            let path = item.path();
            let ext_ok = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str()));
            let (Some(stem), Some(file)) = (
                path.file_stem().and_then(|s| s.to_str()),
                path.file_name().and_then(|s| s.to_str()),
            ) else {
                continue;
            };
            if ext_ok && path.is_file() {
                found.push((stem.to_string(), file.to_string(), path));
            }
        }
    }
    found.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    for (name, file, path) in found {
        if entries.iter().any(|e| e.name == name) {
            continue;
        }
        entries.push(RefEntry {
            name,
            configured: format!("refs/{file}"),
            path,
            exists: true,
            from: RefSource::RefsDir,
        });
    }
    Ok(Refs {
        root: Some(root.to_path_buf()),
        config,
        entries,
    })
}

fn expand(configured: &str, root: &Path, home: Option<&Path>) -> PathBuf {
    if let Some(h) = home {
        if configured == "~" {
            return h.to_path_buf();
        }
        if let Some(rest) = configured.strip_prefix("~/") {
            return h.join(rest);
        }
    }
    let p = Path::new(configured);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}

impl Refs {
    /// The entry a reference name points to, when its file exists.
    pub fn resolve(&self, name: &str) -> Result<&RefEntry, String> {
        let Some(e) = self.entries.iter().find(|e| e.name == name) else {
            let known: Vec<&str> = self.entries.iter().map(|e| e.name.as_str()).collect();
            let known = if known.is_empty() {
                "none".to_string()
            } else {
                known.join(", ")
            };
            return Err(format!(
                "no reference named `{name}`; known: {known} (add it under [refs] in {CONFIG})"
            ));
        };
        if !e.exists {
            return Err(format!(
                "reference `{name}` points to `{}`, which does not exist",
                e.configured
            ));
        }
        Ok(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    fn names(r: &Refs) -> Vec<&str> {
        r.entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn config_then_refs_dir() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "tune.local.toml", "[refs]\na = \"a.mp3\"\n");
        write(d.path(), "refs/b.mp3", "");
        write(d.path(), "refs/a.wav", "");
        write(d.path(), "refs/notes.txt", "");
        let r = find_refs(d.path(), None).unwrap();
        assert_eq!(names(&r), ["a", "b"]);
        assert_eq!(r.entries[0].from, RefSource::Config);
        assert_eq!(r.entries[1].from, RefSource::RefsDir);
        assert_eq!(r.root.as_deref(), Some(d.path()));
        assert_eq!(r.config, Some(d.path().join("tune.local.toml")));
    }

    #[test]
    fn config_keeps_file_order() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "tune.local.toml",
            "[refs]\nz = \"z.mp3\"\na = \"a.mp3\"\n",
        );
        let r = find_refs(d.path(), None).unwrap();
        assert_eq!(names(&r), ["z", "a"]);
    }

    #[test]
    fn tilde_expands_from_home() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "tune.local.toml", "[refs]\nx = \"~/x.mp3\"\n");
        let r = find_refs(d.path(), Some(Path::new("/h"))).unwrap();
        assert_eq!(r.entries[0].path, Path::new("/h/x.mp3"));
        assert_eq!(r.entries[0].configured, "~/x.mp3");
    }

    #[test]
    fn relative_resolves_against_root() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "tune.local.toml", "[refs]\nx = \"music/x.mp3\"\n");
        write(d.path(), "music/x.mp3", "");
        let r = find_refs(d.path(), None).unwrap();
        assert_eq!(r.entries[0].path, d.path().join("music/x.mp3"));
        assert!(r.entries[0].exists);
        assert_eq!(r.resolve("x").unwrap().path, d.path().join("music/x.mp3"));
    }

    #[test]
    fn root_found_from_subdirectory() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "tune.local.toml", "[refs]\nx = \"x.mp3\"\n");
        let sub = d.path().join("songs/x");
        fs::create_dir_all(&sub).unwrap();
        let r = find_refs(&sub, None).unwrap();
        assert_eq!(r.config, Some(d.path().join("tune.local.toml")));
        assert_eq!(names(&r), ["x"]);
    }

    #[test]
    fn missing_marked() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "tune.local.toml",
            "[refs]\ngone = \"~/nope.mp3\"\n",
        );
        let r = find_refs(d.path(), Some(d.path())).unwrap();
        assert!(!r.entries[0].exists);
        assert_eq!(
            r.resolve("gone").unwrap_err(),
            "reference `gone` points to `~/nope.mp3`, which does not exist"
        );
    }

    #[test]
    fn unknown_name_lists_known() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "tune.local.toml",
            "[refs]\na = \"a.mp3\"\nb = \"b.mp3\"\n",
        );
        let r = find_refs(d.path(), None).unwrap();
        assert_eq!(
            r.resolve("c").unwrap_err(),
            "no reference named `c`; known: a, b (add it under [refs] in tune.local.toml)"
        );
        let empty = tempfile::tempdir().unwrap();
        let r = find_refs(empty.path(), None).unwrap();
        assert!(r.root.is_none() || r.entries.is_empty());
        assert!(r.resolve("c").unwrap_err().contains("known: none"));
    }

    #[test]
    fn non_string_value_fails() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "tune.local.toml", "[refs]\na = 3\n");
        assert_eq!(
            find_refs(d.path(), None).unwrap_err(),
            "tune.local.toml: refs.a must be a path string"
        );
    }

    #[test]
    fn bad_toml_fails() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "tune.local.toml", "[refs\n");
        let e = find_refs(d.path(), None).unwrap_err();
        assert!(e.starts_with("tune.local.toml: "), "{e}");
    }

    #[test]
    fn other_tables_ignored() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "tune.local.toml",
            "[other]\nx = 1\n[refs]\na = \"a.mp3\"\n",
        );
        assert_eq!(names(&find_refs(d.path(), None).unwrap()), ["a"]);
    }
}
