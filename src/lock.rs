use std::fs;
use std::path::PathBuf;

const LOCK_DIR: &str = "/tmp/tncli";

fn lock_path(session: &str, service: &str) -> PathBuf {
    PathBuf::from(format!("{LOCK_DIR}/{session}_{service}.lock"))
}

pub fn ensure_dir() {
    let _ = fs::create_dir_all(LOCK_DIR);
}

pub fn acquire(session: &str, service: &str) {
    ensure_dir();
    let _ = fs::write(lock_path(session, service), format!("{}", std::process::id()));
}

pub fn release(session: &str, service: &str) {
    let _ = fs::remove_file(lock_path(session, service));
}

pub fn release_all(session: &str) {
    if let Ok(entries) = fs::read_dir(LOCK_DIR) {
        let prefix = format!("{session}_");
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&prefix) && name.ends_with(".lock") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}
