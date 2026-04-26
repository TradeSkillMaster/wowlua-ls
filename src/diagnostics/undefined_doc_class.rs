pub(crate) const CODE: &str = "undefined-doc-class";

pub(crate) fn extract_name(message: &str) -> Option<&str> {
    message.strip_prefix("undefined class '").and_then(|s| s.strip_suffix('\''))
}
