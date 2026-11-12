use crate::rust_api::summarize_outcome;

pub fn summarize(text: &str) -> summarize_outcome {
    if text.trim().is_empty() {
        return summarize_outcome::blank;
    }
    let (Ok(characters), Ok(lines)) = (
        u32::try_from(text.chars().count()),
        u32::try_from(text.lines().count()),
    ) else {
        return summarize_outcome::too_large;
    };
    match words::frequencies(text) {
        Some(words) => summarize_outcome::measured(characters, lines, words),
        None => summarize_outcome::too_large,
    }
}
