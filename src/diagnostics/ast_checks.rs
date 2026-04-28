use crate::analysis::AnalysisResult;
use crate::ast::*;
use crate::syntax::SyntaxKind;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::{SyntaxNode, NodeOrToken};
use super::WowDiagnostic;

pub(crate) fn run(analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
    let root = SyntaxNode::new_root(tree);
    walk_ast_diagnostics(diags, root, analysis.is_meta);
    crate::diagnostics::doc_field_no_class::run(tree, diags);
    crate::diagnostics::trailing_space::check(diags, tree.source());
}

fn walk_ast_diagnostics(
    diags: &mut Vec<WowDiagnostic>,
    node: SyntaxNode<'_>,
    is_meta: bool,
) {
    match node.kind() {
        SyntaxKind::Block => {
            if let Some(block) = Block::cast(node) {
                check_block_diagnostics(diags, block, is_meta);
            }
            return;
        }
        SyntaxKind::BinaryExpression => {
            if let Some(bin) = BinaryExpression::cast(node) {
                crate::diagnostics::not_precedence::check_node(diags, bin);
            }
        }
        SyntaxKind::FunctionDefinition => {
            if let Some(func) = FunctionDefinition::cast(node) {
                crate::diagnostics::unused_vararg::check_node(diags, func, is_meta);
            }
        }
        SyntaxKind::LocalAssignStatement => {
            if let Some(assign) = LocalAssign::cast(node) {
                check_assignment_balance_local(diags, assign);
            }
        }
        SyntaxKind::AssignStatement => {
            if let Some(assign) = Assign::cast(node) {
                check_assignment_balance_nonlocal(diags, assign);
            }
        }
        SyntaxKind::WhileLoop | SyntaxKind::RepeatUntilLoop => {
            if let Some(block) = node.children().find_map(Block::cast)
                && block_is_empty(&block)
            {
                let r = node.text_range();
                crate::diagnostics::empty_block::check(
                    diags,
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        }
        SyntaxKind::ForCountLoop => {
            if let Some(for_loop) = ForCountLoop::cast(node) {
                check_for_count_loop(diags, for_loop);
            }
        }
        SyntaxKind::ForInLoop => {
            if let Some(for_in) = ForInLoop::cast(node)
                && let Some(block) = for_in.block()
                && block_is_empty(&block)
            {
                let r = for_in.syntax().text_range();
                crate::diagnostics::empty_block::check(
                    diags,
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        }
        SyntaxKind::IfChain => {
            if let Some(if_chain) = IfChain::cast(node) {
                check_if_chain_empty_blocks(diags, if_chain);
            }
        }
        _ => {}
    }
    for child in node.children() {
        walk_ast_diagnostics(diags, child, is_meta);
    }
}

fn check_block_diagnostics(
    diags: &mut Vec<WowDiagnostic>,
    block: Block<'_>,
    is_meta: bool,
) {
    let block_node = block.syntax();
    let statements = block.statements();

    let mut saw_break = false;
    for child in block_node.children_with_tokens() {
        if let NodeOrToken::Token(tok) = &child {
            if tok.kind() == SyntaxKind::BreakKeyword {
                saw_break = true;
            }
        } else if let NodeOrToken::Node(ref n) = child
            && saw_break && Statement::cast(*n).is_some() {
                let r = n.text_range();
                crate::diagnostics::code_after_break::check(
                    diags,
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
                break;
            }
    }

    for (i, stmt) in statements.iter().enumerate() {
        if matches!(stmt, Statement::Return(_)) && i + 1 < statements.len() {
            let next_stmt = &statements[i + 1];
            let r = next_stmt.syntax().text_range();
            crate::diagnostics::unreachable_code::check(
                diags,
                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
            );
        }

        if i + 1 == statements.len()
            && let Statement::Return(ret) = stmt
        {
            let has_values = ret.expression_list()
                .is_some_and(|el| !el.expressions().is_empty());
            let is_fn_top_block = block_node.parent()
                .is_some_and(|p| p.kind() == SyntaxKind::FunctionDefinition);
            if !has_values && is_fn_top_block {
                let r = ret.syntax().text_range();
                crate::diagnostics::redundant_return::check(
                    diags,
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        }
    }

    for child in block_node.children() {
        walk_ast_diagnostics(diags, child, is_meta);
    }
}

fn check_assignment_balance_local(
    diags: &mut Vec<WowDiagnostic>,
    assign: LocalAssign<'_>,
) {
    let Some(name_list) = assign.name_list() else { return };
    let names = name_list.names();
    let expressions = assign
        .expression_list()
        .map(|el| el.expressions())
        .unwrap_or_default();
    let last_is_multi = matches!(
        expressions.last(),
        Some(Expression::FunctionCall(_)) | Some(Expression::VarArgs(_))
    );
    if !last_is_multi && !expressions.is_empty() {
        if expressions.len() > names.len() {
            if let Some(extra) = expressions.get(names.len()) {
                let r = extra.syntax().text_range();
                crate::diagnostics::redundant_value::check(
                    diags,
                    names.len(), expressions.len(),
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        } else if names.len() > expressions.len() {
            let r = assign.syntax().text_range();
            crate::diagnostics::unbalanced_assignments::check(
                diags,
                names.len(), expressions.len(),
                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
            );
        }
    }
}

fn check_assignment_balance_nonlocal(
    diags: &mut Vec<WowDiagnostic>,
    assign: Assign<'_>,
) {
    let Some(var_list) = assign.variable_list() else { return };
    let identifiers = var_list.identifiers();
    let expressions = assign
        .expression_list()
        .map(|el| el.expressions())
        .unwrap_or_default();
    let last_is_multi = matches!(
        expressions.last(),
        Some(Expression::FunctionCall(_)) | Some(Expression::VarArgs(_))
    );
    if !last_is_multi && !expressions.is_empty() {
        if expressions.len() > identifiers.len() {
            if let Some(extra) = expressions.get(identifiers.len()) {
                let r = extra.syntax().text_range();
                crate::diagnostics::redundant_value::check(
                    diags,
                    identifiers.len(), expressions.len(),
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        } else if identifiers.len() > expressions.len() {
            let r = assign.syntax().text_range();
            crate::diagnostics::unbalanced_assignments::check(
                diags,
                identifiers.len(), expressions.len(),
                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
            );
        }
    }
}

fn check_for_count_loop(
    diags: &mut Vec<WowDiagnostic>,
    for_loop: ForCountLoop<'_>,
) {
    if let Some(block) = for_loop.block()
        && block_is_empty(&block)
    {
        let r = for_loop.syntax().text_range();
        crate::diagnostics::empty_block::check(
            diags,
            u32::from(r.start()) as usize, u32::from(r.end()) as usize,
        );
    }

    let Some(expr_list) = for_loop.expression_list() else { return };
    let exprs = expr_list.expressions();
    if exprs.len() < 2 { return; }
    let start_val = expr_literal_number(&exprs[0]);
    let end_val = expr_literal_number(&exprs[1]);
    let step_val = if exprs.len() >= 3 {
        expr_literal_number(&exprs[2])
    } else {
        None
    };
    let (Some(sv), Some(ev)) = (start_val, end_val) else { return };
    let step = step_val.unwrap_or(1.0);
    let should_warn = if step == 0.0 {
        step_val.is_some() && sv != ev
    } else {
        let counting_down = sv > ev;
        let step_positive = step > 0.0;
        (counting_down && step_positive) || (!counting_down && sv != ev && !step_positive)
    };
    if !should_warn { return; }
    let msg = if step_val.is_none() {
        format!("loop from {} to {} will not execute (implicit step is 1; use -1)", sv, ev)
    } else if step == 0.0 {
        format!("loop from {} to {} with step 0 will loop forever", sv, ev)
    } else {
        format!("loop from {} to {} with step {} will not execute", sv, ev, step)
    };
    let br = for_loop.syntax().text_range();
    crate::diagnostics::count_down_loop::check(
        diags,
        u32::from(br.start()) as usize,
        u32::from(br.end()) as usize,
        msg,
    );
}

fn check_if_chain_empty_blocks(
    diags: &mut Vec<WowDiagnostic>,
    if_chain: IfChain<'_>,
) {
    for branch in if_chain.if_branches() {
        if let Some(inner_block) = branch.block()
            && block_is_empty(&inner_block)
        {
            let r = branch.syntax().text_range();
            crate::diagnostics::empty_block::check(
                diags,
                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
            );
        }
    }
    if let Some(else_branch) = if_chain.else_branch()
        && let Some(inner_block) = else_branch.block()
        && block_is_empty(&inner_block)
    {
        let r = else_branch.syntax().text_range();
        crate::diagnostics::empty_block::check(
            diags,
            u32::from(r.start()) as usize, u32::from(r.end()) as usize,
        );
    }
}

fn block_is_empty(block: &Block<'_>) -> bool {
    if !block.statements().is_empty() { return false; }
    for child in block.syntax().children_with_tokens() {
        if let NodeOrToken::Token(tok) = &child
            && (tok.kind() == SyntaxKind::BreakKeyword || tok.kind() == SyntaxKind::Comment)
        {
            return false;
        }
    }
    true
}

fn expr_literal_number(expr: &Expression<'_>) -> Option<f64> {
    match expr {
        Expression::Literal(lit) => {
            lit.get_number().and_then(|s| s.trim().parse::<f64>().ok())
        }
        Expression::UnaryExpression(unary) => {
            if unary.kind() == Operator::Subtract {
                let terms = unary.get_terms();
                if let Some(Expression::Literal(lit)) = terms.first() {
                    return lit.get_number().and_then(|s| s.trim().parse::<f64>().ok()).map(|v| -v);
                }
            }
            None
        }
        _ => None,
    }
}
