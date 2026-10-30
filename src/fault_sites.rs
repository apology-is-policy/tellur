//! Source mapping for runtime reports; not executable graph semantics.
use crate::{
    CheckedProgram,
    fault::Site,
    ir::{Action, Instruction},
};
use std::collections::BTreeMap;
impl CheckedProgram {
    pub(crate) fn runtime_name(&self, name: &str) -> String {
        if let Some((application, member)) = crate::collection_support::qualified_member(name) {
            return format!("{}.{member}", self.runtime_name(application));
        }
        if let Some((base, args)) = crate::collection_support::generic(name) {
            return format!(
                "{}<{}>",
                self.runtime_name(base),
                args.iter()
                    .map(|a| self.runtime_name(a))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        self.source_files()
            .iter()
            .find_map(|s| s.names.get(name))
            .cloned()
            .unwrap_or_else(|| crate::manifest::display_type(name))
    }
    pub(crate) fn runtime_sites(&self) -> BTreeMap<usize, Site> {
        fn walk(program: &CheckedProgram, code: &[Instruction], out: &mut BTreeMap<usize, Site>) {
            for instruction in code {
                let p = &instruction.provenance;
                let file = program
                    .source_files()
                    .iter()
                    .rev()
                    .find(|s| s.start_line <= p.span.line);
                let calls = p
                    .call_sites
                    .iter()
                    .take(crate::fault::MAX_EXPANSION_CALLS)
                    .map(|call| {
                        let mut text = call.clone();
                        // Only mangled module symbols change; author identifiers and
                        // semantic labels remain untouched, including method names.
                        for source in program.source_files() {
                            for (global, local) in &source.names {
                                if global.starts_with("_path_m_") {
                                    text = text.replace(global, local);
                                }
                            }
                        }
                        text
                    })
                    .collect();
                out.insert(
                    instruction.id,
                    Site {
                        control: program.runtime_name(&p.region),
                        path: file.map(|s| s.path.to_string_lossy().into_owned()),
                        line: p
                            .span
                            .line
                            .saturating_sub(file.map_or(0, |s| s.start_line - 1)),
                        column: p.span.column,
                        label: p.site.clone(),
                        expansion_calls: calls,
                        omitted_expansion_calls: p
                            .call_sites
                            .len()
                            .saturating_sub(crate::fault::MAX_EXPANSION_CALLS),
                    },
                );
                if let Action::Loop { body, .. } = &instruction.action {
                    walk(program, body, out);
                }
            }
        }
        let mut result = BTreeMap::new();
        for region in self.regions() {
            walk(self, &region.code, &mut result);
        }
        result
    }
}
