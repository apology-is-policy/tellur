// Mechanisms only: validation, defaults and failure policy live in settings.tellur.
use crate::rust_api;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn text_trim(text: &str) -> rust_api::text_trim_outcome {
    rust_api::text_trim_outcome::trimmed(text.trim().to_owned())
}
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
// Reject duplicate keys as an encoding error, instead of silently keeping one value.
struct Object(BTreeMap<String, String>);
impl<'de> serde::Deserialize<'de> for Object {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Object;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object with unique keys and string values")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Object, A::Error> {
                let mut values = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, String>()? {
                    if values.insert(key.clone(), value).is_some() {
                        return Err(serde::de::Error::custom(format!("duplicate key: {key}")));
                    }
                }
                Ok(Object(values))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}
pub fn json_decode(text: String) -> rust_api::json_decode_outcome {
    match serde_json::from_str::<Object>(&text) {
        Ok(Object(values)) => rust_api::json_decode_outcome::decoded(values),
        Err(e) => rust_api::json_decode_outcome::invalid(e.to_string()),
    }
}
pub fn json_encode(values: &BTreeMap<String, String>) -> rust_api::json_encode_outcome {
    match serde_json::to_string_pretty(values) {
        Ok(text) => rust_api::json_encode_outcome::encoded(text),
        Err(e) => rust_api::json_encode_outcome::encode_failed(e.to_string()),
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
            ".tellur-settings-{}-{}",
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
