use std::{env, fs, path::PathBuf};
use tellur::{compile_file, manifest::Manifest};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).map(PathBuf::from).collect();
    let source = &args[0];
    let manifest_path = source.with_file_name("manifest.json");
    let manifest = Manifest::from_json_at(&fs::read_to_string(&manifest_path)?, &manifest_path)?;
    let program = compile_file(source, manifest)?;
    for entry in ["accept", "scan", "observe_scan", "drain", "observe_drain"] {
        for expanded in [false, true] {
            let name = if expanded {
                format!("{entry}-expanded")
            } else {
                entry.into()
            };
            let output = args[1].join(name);
            if expanded {
                program.expanded(entry)?.build_llvm(entry, &output, 3)?;
            } else {
                program.build_llvm(entry, &output, 3)?;
            }
        }
    }
    Ok(())
}
