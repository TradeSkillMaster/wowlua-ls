use crate::analysis::AnalysisResult;
use crate::annotations::Visibility;
use crate::ast::{AstNode, Identifier};
use crate::syntax::{SyntaxNode, TextSize};
use crate::syntax::tree::SyntaxTree;
use crate::types::{SymbolIdentifier, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct AccessCheck;

impl DiagnosticPass for AccessCheck {
    /// Walk all Identifier nodes looking for field accesses to private/protected fields.
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for ident_node in SyntaxNode::new_root(tree).descendants()
            .filter(|n| n.kind().is_identifier())
        {
            let Some(ident) = Identifier::cast(ident_node) else { continue };
            let names = ident.names();
            if names.len() < 2 { continue; }

            let name_tokens = AnalysisResult::collect_name_tokens_recursive(ident_node);
            if name_tokens.len() < 2 { continue; }

            // Resolve the root to a table
            let root_token = &name_tokens[0];
            let root_offset = TextSize::from(u32::from(root_token.text_range().start()));
            let Some(scope_idx) = analysis.scope_at_offset(root_offset) else { continue };
            let Some(root_sym) = analysis.get_symbol(&SymbolIdentifier::Name(root_token.text().to_string()), scope_idx) else { continue };
            let Some(ver) = analysis.sym(root_sym).versions.last() else { continue };
            let Some(ValueType::Table(Some(start_table_idx))) = ver.resolved_type.as_ref() else { continue };
            let mut table_idx = *start_table_idx;

            for i in 1..name_tokens.len() {
                let field_name = name_tokens[i].text().to_string();

                // Skip transparent @accessor names
                if analysis.ir.has_accessor(table_idx, &field_name) { continue; }

                let field_vis = analysis.get_field(table_idx, &field_name).map(|f| f.visibility);

                if let Some(vis) = field_vis
                    && vis != Visibility::Public
                    && analysis.table(table_idx).class_name.is_some()
                {
                    let enclosing_class = analysis.find_enclosing_class(&ident_node);
                    let same_class = enclosing_class.is_some_and(|ec| analysis.same_class(ec, table_idx));
                    let mut is_subclass = enclosing_class.is_some_and(|ec| analysis.is_subclass_of(ec, table_idx));
                    // If the root variable is a defclass-created instance in this file,
                    // allow protected access at file scope. Private still requires colon-method context.
                    if !is_subclass && vis == Visibility::Protected {
                        let root_name = root_token.text().to_string();
                        if let Some(&dc_table) = analysis.defclass_vars.get(&root_name) {
                            is_subclass = analysis.is_subclass_of(dc_table, table_idx);
                        }
                    }
                    let range = name_tokens[i].text_range();
                    let start = u32::from(range.start()) as usize;
                    let end = u32::from(range.end()) as usize;
                    match vis {
                        Visibility::Private if !same_class => {
                            super::ACCESS_PRIVATE.emit(
                                diags,
                                format!("'{}' is private and cannot be accessed here", field_name),
                                start, end,
                            );
                        }
                        Visibility::Protected if !is_subclass => {
                            super::ACCESS_PROTECTED.emit(
                                diags,
                                format!("'{}' is protected and cannot be accessed here", field_name),
                                start, end,
                            );
                        }
                        _ => {}
                    }
                }

                // Walk to next table in the chain
                if i < name_tokens.len() - 1 {
                    let Some(field_expr_id) = analysis.get_field(table_idx, &field_name).map(|f| f.expr) else { break };
                    let Some(ValueType::Table(Some(next_idx))) = analysis.resolve_expr_type(field_expr_id) else { break };
                    table_idx = next_idx;
                }
            }
        }
    }
}
