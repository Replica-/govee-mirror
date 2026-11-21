//! What an SDK error tells a person to do.

/// The error, with what to do where another program holds the port that the
/// lights answer on.
pub fn explain(error: &govee_toolkit::Error) -> String {
    let text = error.to_string();
    if text.contains(":4002") && (text.contains("10013") || text.contains("10048")) {
        format!(
            "{text}. Another program holds port 4002, which the lights answer on: quit Govee \
             Desktop or any other controller, then press Scan."
        )
    } else {
        text
    }
}
