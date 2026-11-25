pub mod lexer;
pub mod syntax_kind;

pub use syntax_kind::SyntaxKind;

pub mod tree;
pub mod parser;

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use rustc_hash::FxHashSet;

// Re-export high-level syntax API types
pub use tree::{SyntaxNode, SyntaxToken, TextSize, TextRange, TokenAtOffset, NodeOrToken};

/// Maximum bytes read from one source or configuration file.
pub const MAX_SOURCE_FILE_BYTES: u64 = 8 * 1024 * 1024;

const MAX_SCAN_DEPTH: usize = 64;
const MAX_SCAN_DIRECTORIES: usize = 20_000;
const MAX_SCAN_ENTRIES: usize = 200_000;
const MAX_SCAN_FILES: usize = 50_000;
const MAX_SCAN_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SCAN_DURATION: Duration = Duration::from_secs(10);

/// Why a bounded source-tree walk stopped before exhausting its input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceScanLimit {
    Cancelled,
    ElapsedTime,
    Depth,
    Directories,
    Entries,
    Files,
    Bytes,
}

/// The kind of non-symlink filesystem object presented to a scan visitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourcePathKind {
    Directory,
    RegularFile,
}

/// What a scan visitor wants to do with a filesystem object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourcePathAction {
    Skip,
    Descend,
    Collect,
}

#[derive(PartialEq, Eq, Hash)]
enum DirectoryKey {
    #[cfg(any(unix, windows))]
    Id(file_id::FileId),
    Canonical(PathBuf),
}

/// Resource accounting shared by every root in one workspace scan.
pub struct SourceScanBudget {
    started: Instant,
    directories: usize,
    entries: usize,
    files: usize,
    bytes: u64,
    stopped: Option<SourceScanLimit>,
    visited_directories: FxHashSet<DirectoryKey>,
    max_depth: usize,
    max_directories: usize,
    max_entries: usize,
    max_files: usize,
    max_bytes: u64,
    max_duration: Duration,
}

impl Default for SourceScanBudget {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            directories: 0,
            entries: 0,
            files: 0,
            bytes: 0,
            stopped: None,
            visited_directories: FxHashSet::default(),
            max_depth: MAX_SCAN_DEPTH,
            max_directories: MAX_SCAN_DIRECTORIES,
            max_entries: MAX_SCAN_ENTRIES,
            max_files: MAX_SCAN_FILES,
            max_bytes: MAX_SCAN_BYTES,
            max_duration: MAX_SCAN_DURATION,
        }
    }
}

impl SourceScanBudget {
    fn stop(&mut self, limit: SourceScanLimit) -> SourceScanLimit {
        *self.stopped.get_or_insert(limit)
    }

    fn check<C: Fn() -> bool>(&mut self, should_cancel: &C) -> Option<SourceScanLimit> {
        if let Some(limit) = self.stopped {
            return Some(limit);
        }
        if should_cancel() {
            return Some(self.stop(SourceScanLimit::Cancelled));
        }
        if self.started.elapsed() >= self.max_duration {
            return Some(self.stop(SourceScanLimit::ElapsedTime));
        }
        None
    }

    #[cfg(test)]
    fn with_small_limits(max_files: usize, max_bytes: u64) -> Self {
        Self {
            max_files,
            max_bytes,
            ..Self::default()
        }
    }
}

fn directory_key(path: &Path) -> Option<DirectoryKey> {
    #[cfg(any(unix, windows))]
    if let Ok(id) = file_id::get_file_id(path) {
        return Some(DirectoryKey::Id(id));
    }
    std::fs::canonicalize(path).ok().map(DirectoryKey::Canonical)
}

/// Walk a source tree without following descendant symlinks, collecting only
/// regular files selected by `visit`. The caller can share one budget across
/// roots and cancel between filesystem entries. The explicitly supplied root
/// may itself be a symlink; descendants may not be.
pub fn collect_source_files<F, C>(
    root: &Path,
    out: &mut Vec<PathBuf>,
    budget: &mut SourceScanBudget,
    should_cancel: &C,
    mut visit: F,
) -> Option<SourceScanLimit>
where
    F: FnMut(&Path, SourcePathKind) -> SourcePathAction,
    C: Fn() -> bool,
{
    struct Pending {
        path: PathBuf,
        depth: usize,
        is_root: bool,
    }

    let mut pending = vec![Pending { path: root.to_path_buf(), depth: 0, is_root: true }];
    while let Some(item) = pending.pop() {
        if let Some(limit) = budget.check(should_cancel) {
            return Some(limit);
        }

        // Following the root preserves explicitly selected symlinked workspaces.
        // Every descendant uses lstat semantics and symlinks are skipped below.
        let metadata = if item.is_root {
            std::fs::metadata(&item.path)
        } else {
            std::fs::symlink_metadata(&item.path)
        };
        let Ok(metadata) = metadata else { continue };
        if !item.is_root && metadata.file_type().is_symlink() {
            continue;
        }

        if metadata.is_dir() {
            if item.depth > budget.max_depth {
                return Some(budget.stop(SourceScanLimit::Depth));
            }
            if budget.directories >= budget.max_directories {
                return Some(budget.stop(SourceScanLimit::Directories));
            }
            budget.directories += 1;

            let Some(key) = directory_key(&item.path) else { continue };
            if budget.visited_directories.contains(&key)
                || visit(&item.path, SourcePathKind::Directory) != SourcePathAction::Descend
            {
                continue;
            }
            budget.visited_directories.insert(key);

            let Ok(entries) = std::fs::read_dir(&item.path) else { continue };
            let mut children = Vec::new();
            for entry in entries {
                if let Some(limit) = budget.check(should_cancel) {
                    return Some(limit);
                }
                if budget.entries >= budget.max_entries {
                    return Some(budget.stop(SourceScanLimit::Entries));
                }
                budget.entries += 1;
                if let Ok(entry) = entry {
                    children.push(entry.path());
                }
            }
            // The stack is LIFO; reverse sorting preserves the old deterministic
            // ascending depth-first scan order.
            children.sort_unstable_by(|a, b| b.cmp(a));
            pending.extend(children.into_iter().map(|path| Pending {
                path,
                depth: item.depth + 1,
                is_root: false,
            }));
        } else if metadata.is_file()
            && visit(&item.path, SourcePathKind::RegularFile) == SourcePathAction::Collect
        {
            if budget.files >= budget.max_files {
                return Some(budget.stop(SourceScanLimit::Files));
            }
            if metadata.len() > MAX_SOURCE_FILE_BYTES {
                continue;
            }
            if budget.bytes.saturating_add(metadata.len()) > budget.max_bytes {
                return Some(budget.stop(SourceScanLimit::Bytes));
            }
            budget.files += 1;
            budget.bytes += metadata.len();
            out.push(item.path);
        }
    }
    budget.stopped
}

/// Read a source file to a `String`, stripping a leading UTF-8 BOM.
///
/// Files saved with a BOM (`EF BB BF`, common for non-ASCII locale files on
/// Windows) carry U+FEFF as their first character. Keeping it would push every
/// byte offset on line 0 right by one UTF-16 unit, so server-computed
/// diagnostic and navigation positions on that line would land one column off
/// from where a BOM-stripping editor renders the text. Editors strip the BOM
/// before sending contents over LSP, so stripping it here on the disk-read path
/// keeps the two paths' offsets aligned. Use this for every source file that
/// feeds byte-offset→LSP-position mapping (Lua and `.toc` alike).
///
/// The lexer independently skips a stray leading BOM as trivia, so text that
/// reaches it unstripped still parses — this reader just keeps offsets honest.
pub fn read_source_file(path: &Path) -> io::Result<String> {
    read_source_file_with_limit(path, MAX_SOURCE_FILE_BYTES)
}

fn read_source_file_with_limit(path: &Path, max_bytes: u64) -> io::Result<String> {
    let path_metadata = std::fs::symlink_metadata(path)?;
    if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "source path is not a regular file"));
    }
    if path_metadata.len() > max_bytes {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "source file exceeds size limit"));
    }

    let file = std::fs::File::open(path)?;
    let opened_metadata = file.metadata()?;
    if !opened_metadata.is_file() || opened_metadata.len() > max_bytes {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "opened source exceeds size limit"));
    }
    let mut text = String::with_capacity(opened_metadata.len() as usize);
    let mut limited = file.take(max_bytes + 1);
    limited.read_to_string(&mut text)?;
    if text.len() as u64 > max_bytes {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "source file exceeds size limit"));
    }
    strip_leading_bom(&mut text);
    Ok(text)
}

/// Remove a single leading UTF-8 BOM (U+FEFF) from `text`, in place. No-op when
/// absent. Split out from [`read_source_file`] so the offset-sensitive stripping
/// is unit-testable without touching the filesystem.
fn strip_leading_bom(text: &mut String) {
    if text.starts_with('\u{FEFF}') {
        text.drain(..'\u{FEFF}'.len_utf8());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_only_a_leading_bom() {
        // Leading BOM is removed, leaving the rest byte-for-byte.
        let mut with = "\u{FEFF}local L = 5".to_string();
        strip_leading_bom(&mut with);
        assert_eq!(with, "local L = 5");

        // No BOM → untouched.
        let mut without = "local L = 5".to_string();
        strip_leading_bom(&mut without);
        assert_eq!(without, "local L = 5");

        // A non-leading U+FEFF is left in place (only the first is a BOM).
        let mut mid = "a\u{FEFF}b".to_string();
        strip_leading_bom(&mut mid);
        assert_eq!(mid, "a\u{FEFF}b");

        // Only one BOM is stripped.
        let mut double = "\u{FEFF}\u{FEFF}x".to_string();
        strip_leading_bom(&mut double);
        assert_eq!(double, "\u{FEFF}x");
    }

    #[test]
    fn bounded_reader_rejects_oversized_files() {
        let path = std::env::temp_dir().join(format!("wowlua_syntax_reader_{}", std::process::id()));
        std::fs::write(&path, "four").unwrap();
        let result = read_source_file_with_limit(&path, 3);
        let _ = std::fs::remove_file(path);
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[cfg(unix)]
    #[test]
    fn source_walk_skips_descendant_symlinks() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("wowlua_syntax_walk_{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("wowlua_syntax_outside_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("real.lua"), "return 1").unwrap();
        std::fs::write(outside.join("outside.lua"), "return 2").unwrap();
        symlink(&outside, root.join("linked-dir")).unwrap();
        symlink(outside.join("outside.lua"), root.join("linked.lua")).unwrap();

        let mut files = Vec::new();
        let result = collect_source_files(
            &root,
            &mut files,
            &mut SourceScanBudget::default(),
            &|| false,
            |path, kind| match kind {
                SourcePathKind::Directory => SourcePathAction::Descend,
                SourcePathKind::RegularFile if path.extension().is_some_and(|ext| ext == "lua") => SourcePathAction::Collect,
                SourcePathKind::RegularFile => SourcePathAction::Skip,
            },
        );

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        assert_eq!(result, None);
        assert_eq!(files, vec![root.join("real.lua")]);
    }

    #[test]
    fn source_walk_enforces_file_budget() {
        let root = std::env::temp_dir().join(format!("wowlua_syntax_budget_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.lua"), "a").unwrap();
        std::fs::write(root.join("b.lua"), "b").unwrap();

        let mut files = Vec::new();
        let result = collect_source_files(
            &root,
            &mut files,
            &mut SourceScanBudget::with_small_limits(1, 1024),
            &|| false,
            |_, kind| match kind {
                SourcePathKind::Directory => SourcePathAction::Descend,
                SourcePathKind::RegularFile => SourcePathAction::Collect,
            },
        );

        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(result, Some(SourceScanLimit::Files));
        assert_eq!(files.len(), 1);
    }
}
