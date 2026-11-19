use crate::rust_api::accept_reading_outcome;

pub fn accept_reading(value: u32) -> accept_reading_outcome {
    if value == 0 {
        accept_reading_outcome::rejected("sensor offline".into())
    } else {
        accept_reading_outcome::accepted(value)
    }
}
