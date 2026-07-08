use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Parse a `file://` URI string into an absolute path. Centralizes the subtle
/// conversion (percent-encoding, Windows drive letters, spaces) so every caller
/// shares one implementation — see the round-trip tests below.
fn parse_file_uri(uri: &str) -> Option<PathBuf> {
    url::Url::parse(uri).ok()?.to_file_path().ok()
}

pub fn uri_to_abs_path(uri: &lsp_types::Uri) -> Option<PathBuf> {
    parse_file_uri(uri.as_str())
}

pub fn abs_path_to_uri(path: &Path) -> Option<lsp_types::Uri> {
    // lsp_types::Uri has no direct url::Url constructor, so we round-trip
    // through the serialized string form.
    let url = url::Url::from_file_path(path).ok()?;
    lsp_types::Uri::from_str(url.as_str()).ok()
}

/// Final path segment (file name) of a `file://` URI string, for display in
/// progress messages. Returns `None` when the URI can't be parsed to a path.
pub fn uri_file_name(uri: &str) -> Option<String> {
    parse_file_uri(uri)?
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_space_and_non_ascii() {
        let path = PathBuf::from("/home/user/my proj/café.lua");
        let uri = abs_path_to_uri(&path).expect("encode");
        assert!(uri.as_str().contains("%20"), "space must be percent-encoded: {}", uri.as_str());
        let decoded = uri_to_abs_path(&uri).expect("decode");
        assert_eq!(decoded, path);
    }

    #[test]
    fn round_trip_parens() {
        let path = PathBuf::from("/opt/World of Warcraft_retail_/Interface/AddOns/My (Addon)/a.lua");
        let uri = abs_path_to_uri(&path).expect("encode");
        let decoded = uri_to_abs_path(&uri).expect("decode");
        assert_eq!(decoded, path);
    }

    #[cfg(unix)]
    #[test]
    fn decode_percent_encoded_uri_unix() {
        let uri = lsp_types::Uri::from_str("file:///home/user/my%20proj/file.lua").unwrap();
        let path = uri_to_abs_path(&uri).expect("decode");
        assert_eq!(path, PathBuf::from("/home/user/my proj/file.lua"));
    }

    #[cfg(windows)]
    #[test]
    fn decode_windows_drive_uri() {
        let uri = lsp_types::Uri::from_str(
            "file:///C:/Program%20Files%20(x86)/World%20of%20Warcraft/_retail_/Interface/AddOns/Foo/Foo.lua"
        ).unwrap();
        let path = uri_to_abs_path(&uri).expect("decode");
        assert_eq!(
            path,
            PathBuf::from(r"C:\Program Files (x86)\World of Warcraft\_retail_\Interface\AddOns\Foo\Foo.lua")
        );
    }

    #[test]
    fn file_name_decodes_percent_encoding() {
        let uri = abs_path_to_uri(&PathBuf::from("/home/user/my proj/My File.lua")).unwrap();
        assert_eq!(uri_file_name(uri.as_str()).as_deref(), Some("My File.lua"));
    }

    #[test]
    fn file_name_none_for_non_file_uri() {
        assert_eq!(uri_file_name("untitled:Untitled-1"), None);
    }

    #[cfg(windows)]
    #[test]
    fn round_trip_windows_path() {
        let path = PathBuf::from(r"C:\Users\Foo Bar\My Addons\x.lua");
        let uri = abs_path_to_uri(&path).expect("encode");
        assert!(uri.as_str().starts_with("file:///C:/"), "{}", uri.as_str());
        let decoded = uri_to_abs_path(&uri).expect("decode");
        assert_eq!(decoded, path);
    }
}
