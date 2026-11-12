use std::collections::BTreeMap;

pub fn frequencies(text: &str) -> Option<BTreeMap<String, u32>> {
    let mut counts = BTreeMap::<String, u32>::new();
    for word in text.split_whitespace() {
        let count = counts.entry(word.to_owned()).or_default();
        *count = count.checked_add(1)?;
    }
    Some(counts)
}
