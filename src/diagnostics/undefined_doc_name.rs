pub(crate) const CODE: &str = "undefined-doc-name";

pub(crate) fn extract_name(message: &str) -> Option<&str> {
    message.strip_prefix("undefined type '").and_then(|s| s.strip_suffix('\''))
}
