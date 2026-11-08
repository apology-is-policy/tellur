//! Explicit interpretation of a completed control at the process boundary.
//! This metadata never participates in ordinary outcome assignability.
use crate::{CheckedProgram, Diagnostic, Result, Span};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryConfiguration {
    #[serde(default, deserialize_with = "unique_map")]
    pub exit_codes: BTreeMap<String, u8>,
    /// `"cancel"`: the first SIGINT or SIGTERM cancels the entry's tasks; a second
    /// terminates the process with status 128 + signal number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signals: Option<SignalPolicy>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignalPolicy {
    Cancel,
}

// Duplicate contract metadata keys must not silently select a different binding.
pub(crate) fn unique_map<'de, D, T>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Map<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Map<T> {
        type Value = BTreeMap<String, T>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an object with unique names")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut a: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut values = BTreeMap::new();
            while let Some((key, value)) = a.next_entry::<String, T>()? {
                if values.insert(key.clone(), value).is_some() {
                    return Err(serde::de::Error::custom(format!("duplicate key `{key}`")));
                }
            }
            Ok(values)
        }
    }
    deserializer.deserialize_map(Map(std::marker::PhantomData))
}

fn error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_EXIT_CODE", Span::default(), message)
}

impl CheckedProgram {
    /// Resolve defaults on the final contract, then explicit per-entry overrides.
    /// Every declared alternative needs a status, even if currently unreachable.
    pub fn entry_exit_codes(&self, entry: &str) -> Result<BTreeMap<String, u8>> {
        let region = self
            .region(entry)
            .ok_or_else(|| error(format!("unknown executable entry `{entry}`")))?;
        if region.asynchronous {
            return Err(error(
                "async controls require a synchronous entry using run",
            ));
        }
        if region.is_constructor() {
            return Err(error(
                "constructors produce values and cannot be executable entries",
            ));
        }
        if region.boundary.is_some() || region.contingency {
            return Err(error(
                "boundaries are entered using supervise, not as executable controls",
            ));
        }
        if region.parameters.iter().any(|p| p.ty.starts_with('&')) {
            return Err(error(format!(
                "executable entry `{entry}` cannot require borrowed arguments"
            )));
        }
        let mut codes = self
            .outcome_exit_codes
            .get(entry)
            .cloned()
            .unwrap_or_default();
        if let Some(config) = self.manifest.launch.get(entry) {
            for (name, code) in &config.exit_codes {
                if !region.exits.iter().any(|o| &o.name == name) {
                    return Err(error(format!(
                        "launch mapping for `{entry}` names unknown outcome `{name}`"
                    )));
                }
                codes.insert(name.clone(), *code);
            }
        }
        let missing = region
            .exits
            .iter()
            .filter(|o| !codes.contains_key(&o.name))
            .map(|o| o.name.as_str())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(error(format!(
                "executable entry `{entry}` needs process statuses for: {}; annotate the final named contract with @exit N or supply launch.{entry}.exit_codes in the manifest",
                missing.join(", ")
            )));
        }
        Ok(codes)
    }

    /// Embedders can supply the same explicit policy as a manifest launch entry.
    pub fn with_entry_exit_codes(
        &self,
        entry: &str,
        exit_codes: BTreeMap<String, u8>,
    ) -> Result<Self> {
        let mut program = self.clone();
        let signals = program
            .manifest
            .launch
            .get(entry)
            .and_then(|config| config.signals);
        program.manifest.launch.insert(
            entry.into(),
            EntryConfiguration {
                exit_codes,
                signals,
            },
        );
        program.entry_exit_codes(entry)?;
        Ok(program)
    }

    /// Whether the entry opted into cooperative cancellation on signals.
    pub fn entry_cancels_on_signal(&self, entry: &str) -> bool {
        self.manifest
            .launch
            .get(entry)
            .is_some_and(|config| config.signals == Some(SignalPolicy::Cancel))
    }

    pub(crate) fn executable_entries(
        &self,
        default_entry: &str,
    ) -> Result<BTreeMap<String, BTreeMap<String, u8>>> {
        self.entry_exit_codes(default_entry)?;
        // An explicit mapping is an assertion that this control is launchable.
        // Check even non-default entries so typos cannot silently remove them.
        for name in self.manifest.launch.keys() {
            self.entry_exit_codes(name)?;
        }
        Ok(self
            .regions()
            .filter_map(|r| {
                self.entry_exit_codes(&r.name)
                    .ok()
                    .map(|codes| (r.name.clone(), codes))
            })
            .collect())
    }
}
