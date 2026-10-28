//! Declared owner anchors are independent of the returned pointee's type/layout.
use crate::{Diagnostic, Result, ir::BorrowSource, manifest, syntax::Region};
use std::collections::BTreeMap;

pub(crate) type Returns = BTreeMap<(String, usize), Vec<BorrowSource>>;

pub(crate) fn declared_with_schemas(
    region: &Region,
    required: bool,
    schemas: &crate::origins::Schemas,
) -> Result<Returns> {
    let error = |message| Diagnostic::new("E_LIFETIME", region.span, message);
    let mut result = Returns::new();
    for outcome in &region.exits {
        for (index, field) in outcome.fields.iter().enumerate() {
            let key = (outcome.name.clone(), index);
            let origin = region.return_origins.get(&key);
            let reference = manifest::reference_inner(&field.ty);
            let Some(origin) = origin else {
                if required && reference.is_some() {
                    return Err(error(format!(
                        "borrowed trait result `{}` requires `from input` naming a borrowed parameter",
                        field.name
                    )));
                }
                continue;
            };
            let Some((mutable, _)) = reference else {
                return Err(error("`from` requires a reference payload".into()));
            };
            let sources = crate::origins::input_sources(origin, region, schemas)
                .map_err(|e| error(e.message))?;
            for source in &sources {
                if source.guard {
                    continue;
                }
                if mutable && !crate::origins::source_mutable(source, region, schemas) {
                    return Err(error(format!(
                        "exclusive result cannot borrow shared parameter origin `{origin}`"
                    )));
                }
            }
            result.insert(key, sources);
        }
    }
    Ok(result)
}
