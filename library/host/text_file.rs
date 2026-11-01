// Bounded UTF-8 file reads and staged replacement. See library/README.md.
use crate::rust_api;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn file_read(path: &str, limit: u32) -> rust_api::file_read_outcome {
    use rust_api::file_read_outcome::*;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return missing,
        Err(e) => return read_failed(e.to_string()),
    };
    let mut bytes = Vec::new();
    if let Err(e) = file.take(u64::from(limit) + 1).read_to_end(&mut bytes) {
        return read_failed(e.to_string());
    }
    if bytes.len() > limit as usize {
        return too_large;
    }
    match String::from_utf8(bytes) {
        Ok(text) => read(text),
        Err(e) => read_failed(e.to_string()),
    }
}
pub fn file_replace(path: &str, text: &str, limit: u32) -> rust_api::file_replace_outcome {
    use rust_api::file_replace_outcome::*;
    if text.len() > limit as usize {
        return too_large;
    }
    let destination = Path::new(path);
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut pending = None;
    for _ in 0..32 {
        let temporary = parent.join(format!(
            ".tellur-file-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => {
                pending = Some((temporary, file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return write_failed(e.to_string()),
        }
    }
    let Some((temporary, mut file)) = pending else {
        return write_failed("temporary-file names are occupied".into());
    };
    let cleanup = |mut message: String| {
        if let Err(e) = fs::remove_file(&temporary) {
            message.push_str(&format!("; cleanup of {} failed: {e}", temporary.display()));
        }
        message
    };
    let written = file
        .write_all(text.as_bytes())
        .and_then(|_| file.sync_all());
    drop(file);
    if let Err(e) = written {
        return write_failed(cleanup(e.to_string()));
    }
    // Once replacement has been attempted, an error is not a promise of rollback.
    match fs::rename(&temporary, destination) {
        Ok(()) => replaced,
        Err(e) => commit_uncertain(cleanup(e.to_string())),
    }
}
