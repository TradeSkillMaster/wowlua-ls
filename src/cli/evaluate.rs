//! `evaluate` subcommand: parse + analyze a single file, dumping its syntax
//! tree, type info, and diagnostics. Optional trailing byte offset queries
//! hover/definition/signature at that position.

use std::path::PathBuf;
use std::sync::Arc;

use wowlua_ls::analysis::{Analysis, AnalysisConfig};
use wowlua_ls::pre_globals::PreResolvedGlobals;
use wowlua_ls::{lsp, syntax, types};

use super::CliResult;

fn dump_tree_debug(tree: &syntax::tree::SyntaxTree) {
    dump_node_debug(tree, tree.root(), 0);
}

fn dump_node_debug(tree: &syntax::tree::SyntaxTree, id: syntax::tree::NodeId, indent: usize) {
    let node = tree.node(id);
    let prefix = "  ".repeat(indent);
    println!("{}Node: {:?}, {}..{}", prefix, node.kind, node.start, node.end);
    for child in tree.node_children(id) {
        match child {
            syntax::tree::Child::Node(nid) => dump_node_debug(tree, *nid, indent + 1),
            syntax::tree::Child::Token(tid) => {
                let tok = tree.token(*tid);
                let text = tree.token_text(*tid);
                let child_prefix = "  ".repeat(indent + 1);
                println!("{}{:?}, {}..{}, {:?}", child_prefix, tok.kind, tok.start, tok.end, text);
            }
        }
    }
}

pub fn run(file: PathBuf, with_stubs: bool, rest: &[String]) -> CliResult {
    let s = std::fs::read_to_string(&file)?;
    let numbers = line_numbers::LinePositions::from(s.as_str());

    let syntax_before = std::time::Instant::now();
    let tree = syntax::parser::parse(&s);
    let syntax_dur  = std::time::Instant::now() - syntax_before;
    dump_tree_debug(&tree);
    println!("syntax: {:?}", syntax_dur);

    if tree.errors.is_empty() {
        println!("no syntax errors");
    } else {
        println!("{} syntax error(s):", tree.errors.len());
        for e in &tree.errors {
            let start = numbers.from_offset(e.start as usize);
            let end = numbers.from_offset(e.end as usize);
            println!("  {}:{}-{}:{}: {}", start.0.0 + 1, start.1 + 1, end.0.0 + 1, end.1 + 1, e.message);
        }
    }

    // Optionally load stubs with --with-stubs flag
    let pre_globals = if with_stubs {
        let stubs = lsp::load_precomputed_stubs()
            .expect("Precomputed stubs not found — run `cargo run -- regenerate-stubs` first");
        Arc::new(stubs.pre_globals)
    } else {
        Arc::new(PreResolvedGlobals::empty())
    };

    let variables_before = std::time::Instant::now();
    let mut analysis = Analysis::new_with_tree(&tree, pre_globals, AnalysisConfig::default());
    analysis.resolve_types();
    let variables_dur  = std::time::Instant::now() - variables_before;
    analysis.dump();
    let result = analysis.into_result();
    println!("variables: {:?}", variables_dur);

    let diags = result.run_diagnostics(&tree);
    if diags.is_empty() {
        println!("no semantic diagnostics");
    } else {
        println!("{} semantic diagnostic(s):", diags.len());
        for d in &diags {
            let start = numbers.from_offset(d.start);
            let end = numbers.from_offset(d.end);
            println!("  {}:{}-{}:{}: [{}] {}", start.0.0 + 1, start.1 + 1, end.0.0 + 1, end.1 + 1, d.code, d.message);
        }
    }

    if let Some(offset) = rest.iter().find_map(|a| a.parse::<u32>().ok()) {
        if let Some(hover) = result.hover_at(&tree, offset) {
            println!("hover_at({}): {}", offset, hover.type_str);
            if let Some(doc) = &hover.doc {
                println!("  doc: {}", doc);
            }
        }
        match result.definition_at(&tree, offset) {
            Some(types::DefinitionResult::Local(range)) => {
                println!("definition_at({}): local {:?}", offset, range);
            }
            Some(types::DefinitionResult::External(loc)) => {
                println!("definition_at({}): external {}:{}..{}", offset, loc.path.display(), loc.start, loc.end);
            }
            None => {
                println!("definition_at({}): None", offset);
            }
        }
        if let Some(sig) = result.signature_help_at(&tree, offset) {
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
}
