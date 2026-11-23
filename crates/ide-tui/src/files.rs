//! Workspace tree and the Yazi chooser used to open or create files.
use std::collections::HashSet;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const SKIP: &[&str] = &[".git", "target", "node_modules", ".direnv", "result"];
const MAX_ENTRIES: usize = 400;
const MAX_DEPTH: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub dir: bool,
}

#[derive(Debug)]
pub enum ChooseError {
    Missing,
    Failed(String),
}

pub fn list_tree(root: &Path, open: &HashSet<PathBuf>) -> Vec<Entry> {
    let mut entries = Vec::new();
    if !root.is_dir() {
        return entries;
    }
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace")
        .to_string();
    entries.push(Entry {
        path: root.to_path_buf(),
        name,
        depth: 0,
        dir: true,
    });
    if open.contains(root) {
        walk(root, 1, open, &mut entries);
    }
    entries
}

fn walk(dir: &Path, depth: usize, open: &HashSet<PathBuf>, entries: &mut Vec<Entry>) {
    if depth > MAX_DEPTH || entries.len() >= MAX_ENTRIES {
        return;
    }
    let mut children = Vec::new();
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for item in read.flatten() {
        let name = item.file_name().to_string_lossy().to_string();
        if SKIP.contains(&name.as_str()) || name.starts_with('.') {
            continue;
        }
        let Ok(kind) = item.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        children.push((name, item.path(), kind.is_dir()));
    }
    children.sort_by(|left, right| {
        right.2.cmp(&left.2).then_with(|| {
            left.0
                .to_ascii_lowercase()
                .cmp(&right.0.to_ascii_lowercase())
        })
    });
    for (name, path, dir) in children {
        if entries.len() >= MAX_ENTRIES {
            return;
        }
        entries.push(Entry {
            path: path.clone(),
            name,
            depth,
            dir,
        });
        if dir && open.contains(&path) {
            walk(&path, depth + 1, open, entries);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirItem {
    pub path: PathBuf,
    pub name: String,
    pub dir: bool,
}

pub fn list_dir(dir: &Path) -> Vec<DirItem> {
    let mut items = Vec::new();
    if let Some(parent) = dir.parent() {
        if parent != dir {
            items.push(DirItem {
                path: parent.to_path_buf(),
                name: "..".to_string(),
                dir: true,
            });
        }
    }
    let Ok(read) = fs::read_dir(dir) else {
        return items;
    };
    let mut children = Vec::new();
    for item in read.flatten() {
        let name = item.file_name().to_string_lossy().to_string();
        if SKIP.contains(&name.as_str()) || name.starts_with('.') {
            continue;
        }
        let Ok(kind) = item.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        children.push(DirItem {
            path: item.path(),
            name,
            dir: kind.is_dir(),
        });
    }
    children.sort_by(|left, right| {
        right.dir.cmp(&left.dir).then_with(|| {
            left.name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase())
        })
    });
    items.extend(children);
    items
}

pub fn choose_files(program: &str, cwd: &Path) -> Result<Vec<PathBuf>, ChooseError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("axiom-yazi-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|error| ChooseError::Failed(error.to_string()))?;
    let chooser = dir.join("chosen");
    let ran = Command::new(program)
        .arg("--chooser-file")
        .arg(&chooser)
        .current_dir(cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    let result = match ran {
        Err(error) if error.kind() == ErrorKind::NotFound => Err(ChooseError::Missing),
        Err(error) => Err(ChooseError::Failed(error.to_string())),
        Ok(status) if !status.success() => Err(ChooseError::Failed(format!(
            "exit {}",
            status.code().unwrap_or(1)
        ))),
        Ok(_) => Ok(read_chooser(&chooser)),
    };
    let _ = fs::remove_dir_all(&dir);
    result
}

fn read_chooser(path: &Path) -> Vec<PathBuf> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect()
}

pub fn read_or_create(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::write(path, "")?;
            Ok(String::new())
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("axiom-files-{label}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn tree_lists_a_file_and_skips_target() {
        let root = temp_dir("tree");
        fs::write(root.join("note.txt"), "hi").unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("target").join("skip.txt"), "no").unwrap();
        let mut open = HashSet::new();
        open.insert(root.clone());
        let entries = list_tree(&root, &open);
        assert!(entries.iter().any(|entry| entry.name == "note.txt"));
        assert!(!entries.iter().any(|entry| entry.name == "skip.txt"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_chooser_program_is_reported() {
        let root = temp_dir("missing");
        let error = choose_files("axiom-no-such-yazi", &root).unwrap_err();
        assert!(matches!(error, ChooseError::Missing));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn chooser_script_returns_paths_and_creates_a_new_file() {
        let root = temp_dir("choose");
        fs::write(root.join("exists.txt"), "hello-from-disk").unwrap();
        let script = root.join("choose.sh");
        fs::write(
            &script,
            "#!/bin/sh\nout=\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = \"--chooser-file\" ]; then out=\"$2\"; shift 2; else shift; fi\ndone\nprintf '%s\\n%s\\n' \"$PWD/exists.txt\" \"$PWD/brand-new.txt\" > \"$out\"\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
        let picked = choose_files(script.to_str().unwrap(), &root).unwrap();
        assert_eq!(picked.len(), 2);
        assert_eq!(read_or_create(&picked[0]).unwrap(), "hello-from-disk");
        assert_eq!(read_or_create(&picked[1]).unwrap(), "");
        assert!(picked[1].exists());
        let _ = fs::remove_dir_all(&root);
    }
}
