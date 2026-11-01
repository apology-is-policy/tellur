// Strict JSON objects with unique keys and string values.
use crate::rust_api;
use std::collections::BTreeMap;

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
