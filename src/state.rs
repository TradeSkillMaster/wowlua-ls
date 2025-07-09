use std::{collections::{HashMap, HashSet}, sync::{Mutex}};

use lsp_types::Uri;
use rowan::GreenNode;
use crate::diagnostics;
use crate::syntax::SyntaxNodePtr;

pub struct File {
    root: GreenNode,
    uri: Uri,
    diagnostics: Vec<diagnostics::Diagnostic>,
}

static COUNTER: Mutex<u128> = Mutex::new(0);

#[derive(Hash, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identifier(u128);

impl Identifier {
    fn new() -> Identifier {
        let mut counter = COUNTER.lock().unwrap();
        *counter += 1;
        Identifier(*counter)
    }
}

pub struct State {
    files: Vec<File>,
    identifiers: HashMap<Identifier, (SyntaxNodePtr, HashSet<Identifier>)>,
}

impl State {
    pub fn add_file(&mut self, f: File) {
        self.files.push(f);
    }
}
