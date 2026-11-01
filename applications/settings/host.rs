// Mechanisms only: application policy lives in Tellur.
pub use self::files::{file_read, file_replace};
pub use self::http::{
    Connection, Listener, http_accept, http_connection_close, http_listen, http_listener_release,
    http_receive, http_reply_failure, http_send,
};
pub use self::json::{json_decode, json_encode};
use crate::rust_api;

pub fn text_trim(text: &str) -> rust_api::text_trim_outcome {
    rust_api::text_trim_outcome::trimmed(text.trim().to_owned())
}
