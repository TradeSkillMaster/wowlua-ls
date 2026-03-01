//Copyright (C) 2025-  plusmouse and other contributors
//
//This program is free software: you can redistribute it and/or modify
//it under the terms of the GNU General Public License as published by
//the Free Software Foundation, either version 3 of the License, or
//(at your option) any later version.
//
//This program is distributed in the hope that it will be useful,
//but WITHOUT ANY WARRANTY; without even the implied warranty of
//MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//GNU General Public License for more details.
//
//You should have received a copy of the GNU General Public License
//along with this program.  If not, see <https://www.gnu.org/licenses/>.

#![allow(clippy::print_stderr)]

use std::error::Error;
use std::env;
use std::sync::Arc;

use crate::variables::{Variables, PreResolvedGlobals};

mod syntax;
mod lsp;
mod state;
mod diagnostics;
mod variables;
mod ast;
mod annotations;

fn main() -> Result<(), Box<dyn Error + Sync + Send>> {
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 && args[1] == "evaluate" {
        let filename = if args.len() > 2 { &args[2] } else { "tests/type-scans2.lua" };
        //let s = std::fs::read_to_string("../wow-ui-source/full.lua")?;
        let s = std::fs::read_to_string(filename)?;
        let mut a = syntax::syntax::Generator::new(&s);
        let numbers = line_numbers::LinePositions::from(s.as_str());

        let syntax_before = std::time::Instant::now();
        let res = a.process_all();
        let root = syntax::syntax::SyntaxNode::new_root(res.clone());
        let syntax_dur  = std::time::Instant::now() - syntax_before;
        syntax::debug::print_tree(&res);
        println!("syntax: {:?}", syntax_dur);

        if a.errors().is_empty() {
            println!("no syntax errors");
        } else {
            println!("{} syntax error(s):", a.errors().len());
            for e in a.errors() {
                let start = numbers.from_offset(e.start);
                let end = numbers.from_offset(e.end);
                println!("  {}:{}-{}:{}: {}", start.0.0 + 1, start.1 + 1, end.0.0 + 1, end.1 + 1, e.message);
            }
        }

        // Optionally load stubs with --with-stubs flag
        let with_stubs = args.iter().any(|a| a == "--with-stubs");
        let pre_globals = if with_stubs {
            let stubs_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("stubs/vscode-wow-api/Annotations/Core");
            lsp::scan_stubs_for_test(&stubs_path)
        } else {
            Arc::new(PreResolvedGlobals::empty())
        };

        let variables_before = std::time::Instant::now();
        let mut variables = Variables::new(res, pre_globals);
        variables.resolve_types();
        let variables_dur  = std::time::Instant::now() - variables_before;
        variables.dump();
        println!("variables: {:?}", variables_dur);

        if let Some(offset) = args.iter().skip(3).find_map(|a| a.parse::<u32>().ok()) {
            if let Some(hover) = variables.hover_at(offset) {
                println!("hover_at({}): {}", offset, hover.type_str);
                if let Some(doc) = &hover.doc {
                    println!("  doc: {}", doc);
                }
            }
            println!("definition_at({}) => {:?}", offset, variables.definition_at(offset));
            if let Some(sig) = variables.signature_help_at(offset) {
                println!("signature_help_at({}):", offset);
                for (i, s) in sig.signatures.iter().enumerate() {
                    let active = if sig.active_signature == Some(i as u32) { " <-- active" } else { "" };
                    println!("  [{}] {}{}", i, s.label, active);
                    if let Some(doc) = &s.doc {
                        println!("      doc: {}", doc.lines().next().unwrap_or(""));
                    }
                    for (j, p) in s.params.iter().enumerate() {
                        let active_p = if j as u32 == sig.active_parameter { " <-- active param" } else { "" };
                        println!("      param {}: {}{}", j, p, active_p);
                    }
                }
            }
        }

        Ok(())
    } else {
        lsp::start_ls()
    }
}
