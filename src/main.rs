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

/// Convert 1-based line:col to byte offset in source text.
fn line_col_to_offset(text: &str, line: u32, col: u32) -> u32 {
    let mut offset = 0u32;
    for (i, line_text) in text.split('\n').enumerate() {
        if i + 1 == line as usize {
            return offset + (col - 1).min(line_text.len() as u32);
        }
        offset += line_text.len() as u32 + 1;
    }
    text.len() as u32
}

/// Parse "file.lua:LINE:COL" into (filename, line, col). All 1-based.
fn parse_file_location(s: &str) -> Option<(&str, u32, u32)> {
    let mut parts = s.rsplitn(3, ':');
    let col: u32 = parts.next()?.parse().ok()?;
    let line: u32 = parts.next()?.parse().ok()?;
    let file = parts.next()?;
    if file.is_empty() { return None; }
    Some((file, line, col))
}

fn main() -> Result<(), Box<dyn Error + Sync + Send>> {
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 && args[1] == "test-query" {
        // Usage: cargo run -- test-query file.lua:LINE:COL [--with-stubs]
        if args.len() < 3 {
            eprintln!("Usage: wow_ls test-query FILE:LINE:COL [--with-stubs]");
            std::process::exit(1);
        }
        let (filename, line, col) = parse_file_location(&args[2])
            .ok_or("Expected FILE:LINE:COL (1-based)")?;
        let s = std::fs::read_to_string(filename)?;
        let offset = line_col_to_offset(&s, line, col);

        let with_stubs = args.iter().any(|a| a == "--with-stubs");
        let scan_dir = args.iter().position(|a| a == "--scan-dir")
            .and_then(|i| args.get(i + 1))
            .map(|s| std::path::PathBuf::from(s));
        let pre_globals = if with_stubs {
            let stubs_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("stubs/vscode-wow-api/Annotations/Core");
            lsp::scan_stubs_for_test(&stubs_path)
        } else if let Some(dir) = &scan_dir {
            lsp::scan_dir_for_test(dir)
        } else {
            Arc::new(PreResolvedGlobals::empty())
        };

        let mut parser = syntax::syntax::Generator::new(&s);
        let green = parser.process_all();
        let mut variables = Variables::new(green.clone(), pre_globals);
        variables.resolve_types();

        println!("{}:{}:{} (offset {})", filename, line, col, offset);

        if let Some(hover) = variables.hover_at(offset) {
            println!("hover: {}", hover.type_str);
            if let Some(doc) = &hover.doc {
                for line in doc.lines().take(3) {
                    println!("  doc: {}", line);
                }
            }
        } else {
            println!("hover: None");
        }

        match variables.definition_at(offset) {
            Some(crate::variables::DefinitionResult::Local(range)) => {
                let numbers = line_numbers::LinePositions::from(s.as_str());
                let start = numbers.from_offset(u32::from(range.start()) as usize);
                println!("definition: local {}:{}", start.0.0 + 1, start.1 + 1);
            }
            Some(crate::variables::DefinitionResult::External(loc)) => {
                println!("definition: external {}", loc.path.display());
            }
            None => {
                println!("definition: None");
            }
        }

        if let Some(sig) = variables.signature_help_at(offset) {
            for (i, s) in sig.signatures.iter().enumerate() {
                let active = if sig.active_signature == Some(i as u32) { " (active)" } else { "" };
                println!("signature[{}]: {}{}", i, s.label, active);
            }
        }

        if let Some(completions) = variables.completions_at(offset, &s) {
            let preview: Vec<_> = completions.iter().take(10).map(|c| c.label.as_str()).collect();
            println!("completions: {} total [{}{}]", completions.len(), preview.join(", "),
                if completions.len() > 10 { ", ..." } else { "" });
        }

        // Print diagnostics (both syntax and semantic) with suppression applied
        let numbers = line_numbers::LinePositions::from(s.as_str());
        let root = syntax::syntax::SyntaxNode::new_root(green);
        let suppressions = annotations::scan_diagnostic_directives(&root);
        for e in parser.errors() {
            let start = numbers.from_offset(e.start);
            let start_line = start.0.0;
            if !lsp::diagnostics::is_suppressed_pub("syntax", start_line, &suppressions) {
                println!("diagnostic:{}:{}", start_line + 1, e.message);
            }
        }
        for d in variables.diagnostics() {
            let start = numbers.from_offset(d.start);
            let start_line = start.0.0;
            if !lsp::diagnostics::is_suppressed_pub(d.code, start_line, &suppressions) {
                println!("diagnostic:{}:{}", start_line + 1, d.code);
            }
        }

        Ok(())
    } else if args.len() > 1 && args[1] == "evaluate" {
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

        if variables.diagnostics().is_empty() {
            println!("no semantic diagnostics");
        } else {
            println!("{} semantic diagnostic(s):", variables.diagnostics().len());
            for d in variables.diagnostics() {
                let start = numbers.from_offset(d.start);
                let end = numbers.from_offset(d.end);
                println!("  {}:{}-{}:{}: [{}] {}", start.0.0 + 1, start.1 + 1, end.0.0 + 1, end.1 + 1, d.code, d.message);
            }
        }

        if let Some(offset) = args.iter().skip(3).find_map(|a| a.parse::<u32>().ok()) {
            if let Some(hover) = variables.hover_at(offset) {
                println!("hover_at({}): {}", offset, hover.type_str);
                if let Some(doc) = &hover.doc {
                    println!("  doc: {}", doc);
                }
            }
            match variables.definition_at(offset) {
                Some(crate::variables::DefinitionResult::Local(range)) => {
                    println!("definition_at({}): local {:?}", offset, range);
                }
                Some(crate::variables::DefinitionResult::External(loc)) => {
                    println!("definition_at({}): external {}:{}..{}", offset, loc.path.display(), loc.start, loc.end);
                }
                None => {
                    println!("definition_at({}): None", offset);
                }
            }
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
