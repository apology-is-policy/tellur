use crate::rust_api;
use std::io::Read;
pub use std::fs::File as InputFile;

pub fn file_open(path: &str) -> rust_api::file_open_outcome {
    match InputFile::open(path) {
        Ok(file) => rust_api::file_open_outcome::opened(file),
        Err(error) => rust_api::file_open_outcome::failed(error.to_string()),
    }
}

// Read for &File advances the OS file offset without replacing the File owner.
pub fn file_chunk(mut file: &InputFile) -> rust_api::file_chunk_outcome {
    let mut bytes = vec![0; 4096];
    match file.read(&mut bytes) {
        Ok(0) => rust_api::file_chunk_outcome::end,
        Ok(count) => {
            bytes.truncate(count);
            rust_api::file_chunk_outcome::data(bytes)
        }
        Err(error) => rust_api::file_chunk_outcome::failed(error.to_string()),
    }
}

pub fn file_release(file: InputFile) -> rust_api::file_release_outcome {
    // Rust's File destructor releases the descriptor; it does not report close errors.
    drop(file);
    rust_api::file_release_outcome::released
}
