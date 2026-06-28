use crate::analysis::AnalysisResult;
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use super::{DiagnosticPass, WowDiagnostic};

pub struct ClassShadowsBuiltin;

impl DiagnosticPass for ClassShadowsBuiltin {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);
        let scan = crate::annotations::scan_all_annotations(root);

        for class in &scan.classes {
            if !analysis.ir.ext.stub_class_names.contains(&class.name) { continue; }
            // Only flag a *record* declaration — one that defines its own explicit
            // `@field` contract — reusing a built-in name. This is the case that
            // silently clobbers the built-in type's meaning and drove the motivating
            // false `missing-fields`. A stub-name reuse with no explicit `@field`s is
            // almost always a deliberate, benign augmentation/typing idiom (a
            // `CreateFromMixins` mixin, a namespace table whose members are inferred
            // from its constructor, or a `CreateFrame` result typed as the builtin),
            // which merges additively and is not worth the noise.
            if class.declared_field_names.is_empty() { continue; }
            let Some((start, end)) = class.def_range else { continue };
            super::CLASS_SHADOWS_BUILTIN.emit(
                diags,
                format!("class '{}' shadows a built-in WoW API class", class.name),
                start as usize, end as usize,
            );
        }
    }
}
