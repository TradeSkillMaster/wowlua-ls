use std::collections::HashMap;
use crate::ast::{AstNode, Block, Statement, Expression, FunctionCall, TableConstructor, FieldKind};
use crate::syntax::SyntaxNode;
use super::annotation_scanning::{
    CallbackRegistryDecl, StringArrayConstDecl, GeneratesEventsSpec, ExternalGlobal, ExternalGlobalKind,
    detect_addon_ns_var, canonicalize_member_path, scope_addon_ns_path, event_name_from_expr,
    collect_statements_recursive,
};

/// Build the map of `@generates-events` method leaf name → spec from all globals.
/// Keyed by the leaf method name so any receiver inheriting the method matches at
/// the call site. Shared by the callback-registry scan and `DefclassContext`.
pub(crate) fn build_generates_events_methods(
    all_globals: &[ExternalGlobal],
) -> HashMap<String, GeneratesEventsSpec> {
    build_generates_events_methods_iter(all_globals.iter())
}

/// Iterator form of [`build_generates_events_methods`], so callers holding globals
/// across several collections (stubs + a per-file map) can build the map without
/// allocating a combined slice.
pub(crate) fn build_generates_events_methods_iter<'a>(
    globals: impl Iterator<Item = &'a ExternalGlobal>,
) -> HashMap<String, GeneratesEventsSpec> {
    let mut map: HashMap<String, GeneratesEventsSpec> = HashMap::new();
    for g in globals.filter(|g| g.generates_events.is_some()) {
        let leaf = match &g.kind {
            ExternalGlobalKind::Method(_, method_name, _) => method_name.clone(),
            _ => g.name.split('.').next_back().unwrap_or(&g.name).to_string(),
        };
        map.insert(leaf, g.generates_events.clone().unwrap());
    }
    map
}

/// Extract event names from a table constructor used as an event list. Positional
/// string literals contribute their value; field references (`SomeEvents.OnFoo`)
/// and named fields (`OnFoo = "OnFoo"`) contribute their leaf/key name (the
/// value==name convention). Returns `(names, complete)`; `complete` is false when
/// any entry couldn't be statically resolved (a computed/non-literal positional).
fn extract_event_names(tc: &TableConstructor<'_>) -> (Vec<String>, bool) {
    let mut names = Vec::new();
    let mut complete = true;
    for field in tc.fields() {
        match field.kind() {
            Some(FieldKind::Positional(value)) => {
                match event_name_from_expr(&value) {
                    Some(n) => names.push(n),
                    None => complete = false,
                }
            }
            Some(FieldKind::Named { name, .. }) => names.push(name),
            // Bracket-keyed entry (`[expr] = v`) — can't resolve to a name.
            None => complete = false,
        }
    }
    (names, complete)
}

/// Scan a file for callback registries and string-array constants used to declare
/// their events. A registry is `Receiver:Method(arg)` where `Method` carries
/// `@generates-events` (per `events_methods`); a string-array constant is
/// `path = { "a", "b", ... }`. Both keys are canonicalized (addon-namespace alias
/// rewritten) so the producer site, a cross-file `events_ref`, and the consumer
/// `:RegisterCallback("…")` site agree.
pub(crate) fn scan_callback_registries(
    root: SyntaxNode<'_>,
    events_methods: &HashMap<String, GeneratesEventsSpec>,
    addon_scope: Option<&str>,
) -> (Vec<CallbackRegistryDecl>, Vec<StringArrayConstDecl>) {
    let mut registries = Vec::new();
    let mut constants = Vec::new();
    if events_methods.is_empty() {
        return (registries, constants);
    }
    let Some(block) = Block::cast(root) else { return (registries, constants); };
    let ns = detect_addon_ns_var(root);
    let ns_ref = ns.as_deref();
    // Canonicalize a name chain and scope addon-namespace paths by addon identity.
    // Paths rooted at `self` are dropped: a method receiver is not a stable cross-file
    // key (every mixin method has a `self`), so different classes' `self:...` registries
    // would otherwise collide under one key and produce false positives.
    let canon = |names: &[String]| -> Option<String> {
        if names.first().map(String::as_str) == Some("self") {
            return None;
        }
        canonicalize_member_path(names, ns_ref).map(|p| scope_addon_ns_path(p, addon_scope))
    };

    // Registry calls: walk every call (init code may live inside a function body).
    for node in root.descendants() {
        let Some(call) = FunctionCall::cast(node) else { continue };
        let Some(ident) = call.identifier() else { continue };
        if !ident.is_call_to_self() { continue; }
        let chain = ident.names();
        if chain.len() < 2 { continue; }
        let method = &chain[chain.len() - 1];
        let Some(spec) = events_methods.get(method) else { continue };
        let Some(receiver_path) = canon(&chain[..chain.len() - 1]) else { continue };

        let arg = call
            .arguments()
            .map(|a| a.expressions())
            .and_then(|exprs| exprs.into_iter().nth(spec.events_param.saturating_sub(1)));
        let (inline_events, events_ref, complete) = match arg {
            Some(Expression::TableConstructor(tc)) => {
                let (names, complete) = extract_event_names(&tc);
                (names, None, complete)
            }
            // A reference to an events table (`addonTable.Constants.Events`).
            Some(Expression::Identifier(id)) => {
                (Vec::new(), canon(&id.names()), true)
            }
            // Anything else (computed/missing) — can't determine the event set.
            _ => (Vec::new(), None, false),
        };
        registries.push(CallbackRegistryDecl { receiver_path, inline_events, events_ref, complete });
    }

    // String-array constants: `path = { "a", "b", ... }` (recurse into control flow).
    let mut stmts = Vec::new();
    collect_statements_recursive(&block, &mut stmts);
    for stmt in &stmts {
        let Statement::Assign(assign) = stmt else { continue };
        let (Some(var_list), Some(expr_list)) = (assign.variable_list(), assign.expression_list()) else { continue };
        let idents = var_list.identifiers();
        let exprs = expr_list.expressions();
        if idents.len() != 1 || exprs.len() != 1 { continue; }
        let Expression::TableConstructor(tc) = &exprs[0] else { continue };
        let (values, complete) = extract_event_names(tc);
        if values.is_empty() { continue; }
        let Some(path) = canon(&idents[0].names()) else { continue };
        constants.push(StringArrayConstDecl { path, values, complete });
    }

    (registries, constants)
}
