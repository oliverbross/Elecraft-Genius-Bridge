use serde::Serialize;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static EVIDENCE_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn set_evidence_dir(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref().to_path_buf();
    let _ = create_dir_all(&path);
    EVIDENCE_DIR.set(path).is_ok()
}

pub fn evidence_dir() -> Option<&'static Path> {
    EVIDENCE_DIR.get().map(PathBuf::as_path)
}

pub fn append_evidence_line(file_name: &str, line: impl AsRef<str>) {
    let Some(dir) = evidence_dir() else {
        return;
    };
    let path = dir.join(file_name);
    if let Some(parent) = path.parent() {
        let _ = create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {}", timestamp_ms(), line.as_ref());
    }
}

pub fn append_evidence_json<T: Serialize>(file_name: &str, value: &T) {
    let Ok(line) = serde_json::to_string(value) else {
        return;
    };
    append_evidence_line(file_name, line);
}

fn timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
