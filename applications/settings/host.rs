// Mechanisms only: application policy lives in Tellur.
pub use self::files::{file_read, file_replace};
pub use self::http::{
    Connection, Listener, http_connection_close, http_listen, http_listener_release,
    http_reply_failure,
};
pub use self::http_accept::http_accept;
pub use self::http_send::{http_connection_finish, http_send};
pub use self::http_receive::http_receive;
pub use self::json::{json_decode, json_encode};
use crate::rust_api;

pub fn text_trim(text: &str) -> rust_api::text_trim_outcome {
    rust_api::text_trim_outcome::trimmed(text.trim().to_owned())
}
