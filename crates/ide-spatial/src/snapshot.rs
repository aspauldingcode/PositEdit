//! F10 workspace snapshots. No credentials or process handles.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PaneSnap {
    pub name: String,
    pub path: Option<String>,
    pub scroll: usize,
    pub cursor: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceSnapshot {
    pub version: u32,
    pub workspace_id: String,
    pub focus: usize,
    pub view: i32,
    pub panes: Vec<PaneSnap>,
    pub lens_history: Vec<String>,
    pub saved_at: u64,
}

#[derive(Clone, Debug, Default)]
pub struct SnapshotStore {
    last: Option<WorkspaceSnapshot>,
    last_path: Option<String>,
}

impl SnapshotStore {
    pub fn opt_out(workspace: &Path) -> bool {
        workspace.join(".posit").join("spatial-opt-out").is_file()
    }

    pub fn capture(
        &mut self,
        workspace_id: impl Into<String>,
        focus: usize,
        view: i32,
        panes: Vec<PaneSnap>,
        lens_history: Vec<String>,
    ) -> WorkspaceSnapshot {
        let saved_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let snap = WorkspaceSnapshot {
            version: 1,
            workspace_id: workspace_id.into(),
            focus,
            view,
            panes,
            lens_history,
            saved_at,
        };
        self.last = Some(snap.clone());
        snap
    }

    pub fn write_atomic(&mut self, workspace: &Path, snap: &WorkspaceSnapshot) -> io::Result<PathBuf> {
        if Self::opt_out(workspace) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "spatial snapshots opted out",
            ));
        }
        let dir = workspace.join(".posit").join("spatial");
        fs::create_dir_all(&dir)?;
        let path = dir.join("workspace.json");
        let tmp = dir.join("workspace.json.tmp");
        let body = serde_json::to_vec_pretty(snap)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        // Strip accidental secrets by rejecting known credential keys in JSON text.
        let text = String::from_utf8_lossy(&body);
        if text.contains("\"password\"")
            || text.contains("\"token\"")
            || text.contains("\"secret\"")
            || text.contains("\"api_key\"")
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "refusing to persist credential-shaped fields",
            ));
        }
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &path)?;
        self.last = Some(snap.clone());
        self.last_path = Some(path.display().to_string());
        Ok(path)
    }

    pub fn load(&mut self, workspace: &Path) -> io::Result<WorkspaceSnapshot> {
        let path = workspace.join(".posit").join("spatial").join("workspace.json");
        let text = fs::read_to_string(&path)?;
        let snap: WorkspaceSnapshot = serde_json::from_str(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if !validate_snapshot(&snap) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "snapshot failed validation",
            ));
        }
        self.last = Some(snap.clone());
        self.last_path = Some(path.display().to_string());
        Ok(snap)
    }

    pub fn last_loaded(&self) -> Option<&WorkspaceSnapshot> {
        self.last.as_ref()
    }

    pub fn last_path(&self, workspace_id: &str) -> String {
        self.last_path
            .clone()
            .unwrap_or_else(|| format!(".posit/spatial/{workspace_id}.json"))
    }
}

/// Pure validation for Bend differential tests.
pub fn validate_snapshot(snap: &WorkspaceSnapshot) -> bool {
    snap.version == 1 && !snap.workspace_id.is_empty() && snap.focus <= snap.panes.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn round_trip_and_opt_out() {
        let root = env::temp_dir().join(format!(
            "posit-spatial-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join(".posit")).unwrap();
        let mut store = SnapshotStore::default();
        let snap = store.capture(
            "ws",
            0,
            0,
            vec![PaneSnap {
                name: "a.rs".into(),
                path: Some("a.rs".into()),
                scroll: 1,
                cursor: 2,
            }],
            vec![],
        );
        assert!(validate_snapshot(&snap));
        let path = store.write_atomic(&root, &snap).unwrap();
        assert!(path.is_file());
        let mut other = SnapshotStore::default();
        let loaded = other.load(&root).unwrap();
        assert_eq!(loaded.panes[0].name, "a.rs");
        fs::write(root.join(".posit").join("spatial-opt-out"), b"").unwrap();
        assert!(store.write_atomic(&root, &snap).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
