use std::path::{Path, PathBuf};

pub fn manifest_path(source: &Path) -> PathBuf {
    std::iter::once(source.with_extension("manifest.json"))
        .chain(
            source
                .ancestors()
                .skip(1)
                .map(|dir| dir.join("manifest.json")),
        )
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("missing manifest for {}", source.display()))
}
