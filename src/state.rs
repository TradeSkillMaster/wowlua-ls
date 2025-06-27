use std::collections::{HashMap, HashSet};

use lsp_types::Uri;
use rowan::GreenNode;
use crate::diagnostics;

pub struct File {
    root: GreenNode,
    uri: Uri,
    diagnostics: Vec<diagnostics::Diagnostic>,
}

#[derive(Hash)]
pub struct Identifier(String);

pub struct State {
    files: Vec<File>,
    identifiers: HashMap<Identifier, HashSet<Identifier>>,
}

impl State {
    pub fn add_file(&mut self, f: File) {
        self.files.push(f);
    }
}
