use std::collections::{HashMap, HashSet};
use crate::analysis::AnalysisResult;
use crate::ast::*;
use crate::syntax::SyntaxKind;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::{SyntaxNode, NodeOrToken};
use crate::types::*;
use super::{DiagnosticPass, RelatedInfo, WowDiagnostic};

pub(crate) struct AnnotationMetadata;

impl DiagnosticPass for AnnotationMetadata {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
    let root = SyntaxNode::new_root(tree);

    // ── Part 1: Comment-level checks ──────────────────────────────
    // duplicate_constructor, duplicate_doc_alias, duplicate_doc_field
    let mut current_class: Option<String> = None;
    let mut class_constructor_count: HashMap<String, u32> = HashMap::new();
    let mut class_field_names: HashMap<String, HashSet<String>> = HashMap::new();
    let mut seen_aliases: HashSet<String> = HashSet::new();

    for event in root.descendants_with_tokens() {
        let NodeOrToken::Token(tok) = event else { continue };
        if tok.kind() != SyntaxKind::Comment {
            if tok.kind() != SyntaxKind::Whitespace && tok.kind() != SyntaxKind::Newline {
                current_class = None;
            }
            continue;
        }
        let text = tok.text();

        let after = text.strip_prefix("---@class ").or_else(|| text.strip_prefix("---@enum "));
        if let Some(after) = after {
            let name = after.split(|c: char| c.is_whitespace() || c == '<' || c == ':')
                .next().unwrap_or("");
            if !name.is_empty() {
                current_class = Some(name.to_string());
            }
            continue;
        }

        if let Some(rest) = text.strip_prefix("---@constructor") {
            let rest = rest.trim();
            if !rest.is_empty()
                && let Some(ref class_name) = current_class
            {
                let count = class_constructor_count.entry(class_name.clone()).or_insert(0);
                *count += 1;
                if *count > 1 {
                    let r = tok.text_range();
                    super::DUPLICATE_CONSTRUCTOR.emit(
                        diags,
                        format!("duplicate @constructor on class '{}'", class_name),
                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                    );
                }
            }
            continue;
        }

        if let Some(rest) = text.strip_prefix("---@alias ") {
            let rest = rest.strip_prefix("(opaque)").map(|r| r.trim_start()).unwrap_or(rest);
            let name = rest.split(|c: char| c.is_whitespace() || c == '<' || c == ':')
                .next().unwrap_or("");
            if !name.is_empty() && !seen_aliases.insert(name.to_string()) {
                let r = tok.text_range();
                super::DUPLICATE_DOC_ALIAS.emit(
                    diags,
                    format!("duplicate @alias '{}'", name),
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
            continue;
        }

        if let Some(rest) = text.strip_prefix("---@field ") {
            if let Some(ref class_name) = current_class {
                let rest = rest.strip_prefix("private ").or_else(|| rest.strip_prefix("protected "))
                    .or_else(|| rest.strip_prefix("public ")).unwrap_or(rest);
                let raw_name = rest.split_whitespace().next().unwrap_or("");
                if raw_name.starts_with('[') { continue; }
                let field_name = raw_name.trim_end_matches('?');
                if !field_name.is_empty() {
                    let fields = class_field_names.entry(class_name.clone()).or_default();
                    if !fields.insert(field_name.to_string())
                        && let Some((start, end)) = crate::analysis::Analysis::find_field_comment_range(root, class_name, field_name, true)
                    {
                        super::DUPLICATE_DOC_FIELD.emit(
                            diags,
                            format!("duplicate @field '{}'", field_name),
                            start as usize, end as usize,
                        );
                    }
                }
            }
            continue;
        }
    }

    // ── Part 2: Function-level annotation checks ──────────────────
    // duplicate_doc_param, undefined_doc_param, builds_field_not_self,
    // constructor_return, return_self_class_name
    let func_by_start: HashMap<u32, usize> = analysis.ir.functions.iter()
        .enumerate()
        .filter(|(_, f)| f.def_node != DefNode::DUMMY)
        .map(|(i, f)| (f.def_node.start, i))
        .collect();

    for node in root.descendants() {
        if node.kind() != SyntaxKind::FunctionDefinition { continue; }
        let node_start = u32::from(node.text_range().start());
        let Some(&func_idx) = func_by_start.get(&node_start) else { continue };
        let func = &analysis.ir.functions[func_idx];

        let annotations = crate::annotations::extract_annotations(node);

        if !annotations.params.is_empty() {
            let arg_names: HashSet<String> = func.args.iter()
                .filter_map(|&sym_idx| match &analysis.ir.symbols[sym_idx.val()].id {
                    SymbolIdentifier::Name(n) => Some(n.clone()),
                    _ => None,
                })
                .collect();

            let comment_ranges = crate::analysis::Analysis::collect_preceding_annotation_ranges(node);
            let func_start = node_start as usize;
            let func_end = func_start + "function".len();

            let mut seen_params: HashSet<String> = HashSet::new();
            for p in &annotations.params {
                let (s, e) = comment_ranges.iter()
                    .find(|(text, _, _)| text.starts_with("---@param") && text.contains(&p.name))
                    .map(|(_, s, e)| (*s, *e))
                    .unwrap_or((func_start, func_end));
                if !seen_params.insert(p.name.clone()) {
                    super::DUPLICATE_DOC_PARAM.emit(
                        diags,
                        format!("duplicate @param '{}'", p.name),
                        s, e,
                    );
                } else if !arg_names.contains(&p.name) && p.name != "self"
                    && !(p.name == "..." && func.is_vararg)
                {
                    super::UNDEFINED_DOC_PARAM.emit(
                        diags,
                        format!("@param '{}' does not match any parameter in the function signature", p.name),
                        s, e,
                    );
                }
            }
        }

        if func.constructor && !func.return_annotations.is_empty() {
            let r = node.text_range();
            super::CONSTRUCTOR_RETURN.emit(
                diags,
                "@constructor method should not have return annotations (only @return self is allowed)".to_string(),
                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
            );
        }

        let func_index = FunctionIndex(func_idx);
        if analysis.inherited_constructors.contains(&func_index)
            && !func.constructor
            && !func.return_annotations.is_empty()
        {
            let r = node.text_range();
            super::CONSTRUCTOR_RETURN.emit(
                diags,
                "@constructor method should not have return annotations (only @return self is allowed)".to_string(),
                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
            );
        }

        if func.builds_field.is_some()
            && let Some(class_name) = analysis.function_owner_class.get(&func_index)
        {
            let returns_own_class = annotations.returns.iter().any(|rt| {
                matches!(rt, crate::annotations::AnnotationType::Simple(s) if s == class_name)
            });
            if returns_own_class {
                let r = node.text_range();
                super::BUILDS_FIELD_NOT_SELF.emit(
                    diags,
                    format!("@builds-field method returns '{}' instead of 'self'; builder pattern will not track accumulated fields", class_name),
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        }

        if func.builds_field.is_none()
            && let Some(class_name) = analysis.function_owner_class.get(&func_index)
        {
            let returns_own_class = annotations.returns.iter().any(|rt| {
                matches!(rt, crate::annotations::AnnotationType::Simple(s) if s == class_name)
            });
            if returns_own_class {
                let func_node_id = node.id;
                let any_returns_bare_self = FunctionDefinition::cast(node).and_then(|f| f.block()).is_some_and(|block| {
                    block.syntax().descendants().any(|desc| {
                        let Some(ret) = Return::cast(desc) else { return false };
                        let in_nested_fn = ret.syntax().ancestors().any(|anc| {
                            anc.kind() == SyntaxKind::FunctionDefinition && anc.id != func_node_id
                        });
                        if in_nested_fn { return false; }
                        let Some(expr_list) = ret.expression_list() else { return false };
                        let exprs = expr_list.expressions();
                        exprs.first().is_some_and(|expr| {
                            if let Expression::Identifier(ident) = expr {
                                ident.syntax().kind() == SyntaxKind::NameRef
                                    && ident.syntax().text().0 == "self"
                            } else {
                                false
                            }
                        })
                    })
                });
                if any_returns_bare_self {
                    let r = node.text_range();
                    super::RETURN_SELF_CLASS_NAME.emit(
                        diags,
                        format!("Method returns '{}' instead of 'self'; use '@return self' for methods that return the receiver", class_name),
                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                    );
                }
            }
        }
    }

    // ── Part 3: Deprecated call-site checks ──────────────────────
    for expr in analysis.ir.exprs.iter() {
        let Expr::FunctionCall { func: callee, call_range, .. } = expr else { continue };
        let callee = *callee;
        let call_range = *call_range;
        let Some(callee_type) = analysis.resolve_expr_type(callee) else { continue };
        let func_idx = match callee_type {
            ValueType::Function(Some(idx)) => idx,
            _ => continue,
        };
        if !analysis.func(func_idx).deprecated { continue; }
        let name = analysis.function_name(func_idx).unwrap_or_else(|| {
            if let Expr::FieldAccess { table, field, .. } = analysis.expr(callee) {
                let table = *table;
                let field = field.as_str();
                let class_name = analysis.resolve_expr_type(table).and_then(|ty| match ty {
                    ValueType::Table(Some(idx)) => analysis.table(idx).class_name.clone(),
                    _ => None,
                });
                match class_name {
                    Some(cls) => {
                        let sep = if let Expr::FunctionCall { is_method_call: true, .. } = expr { ":" } else { "." };
                        format!("{}{}{}", cls, sep, field)
                    }
                    None => field.to_string(),
                }
            } else {
                "?".to_string()
            }
        });
        // Related info: point to the function definition where @deprecated was declared.
        // Only for local (non-external) functions whose def_node is in the current file.
        let func = analysis.func(func_idx);
        let related = if !func_idx.is_external() && func.def_node.node_id.is_some() {
            vec![RelatedInfo {
                file_path: None,
                start: func.def_node.start as usize,
                end: func.def_node.end as usize,
                message: "Deprecated declaration here".to_string(),
            }]
        } else {
            Vec::new()
        };
        super::DEPRECATED.emit_with_related(
            diags,
            format!("'{}' is deprecated", name),
            call_range.0 as usize, call_range.1 as usize,
            related,
        );
    }
}
}
