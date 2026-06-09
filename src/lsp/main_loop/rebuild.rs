use super::*;

/// Compare two globals on the fields that affect analysis results (excludes
/// positional fields like doc, source_path, def_start, def_end which only affect
/// hover/go-to-definition display, not type resolution or diagnostics).
// IMPORTANT: Update this function when adding semantic fields to ExternalGlobal.
pub(super) fn global_semantic_eq(x: &ExternalGlobal, y: &ExternalGlobal) -> bool {
    x.name == y.name
        && x.kind == y.kind
        && x.params == y.params
        && x.returns == y.returns
        && x.overloads == y.overloads
        && x.deprecated == y.deprecated
        && x.nodiscard == y.nodiscard
        && x.constructor == y.constructor
        && x.visibility == y.visibility
        && x.generics == y.generics
        && x.defclass == y.defclass
        && x.defclass_parent == y.defclass_parent
        && x.builds_field == y.builds_field
        && x.built_name == y.built_name
        && x.built_extends == y.built_extends
        && x.string_value == y.string_value
        && x.number_value == y.number_value
        && x.requires == y.requires
}

pub(super) fn globals_match(a: &[ExternalGlobal], b: &[ExternalGlobal]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| global_semantic_eq(x, y))
}

/// Compare two class declarations on the fields that affect analysis results,
/// ignoring positional fields (def_range, def_path, field_ranges, field_paths)
/// and display-only fields (see, declared_field_names, field_literals).
// IMPORTANT: Update this function when adding semantic fields to ClassDecl.
// bare_inferred_field_names: always empty in per-file classes; tracked via self_fields_match.
pub(super) fn class_semantic_eq(x: &ClassDecl, y: &ClassDecl) -> bool {
    x.name == y.name
        && x.type_params == y.type_params
        && x.type_param_constraints == y.type_param_constraints
        && x.parents == y.parents
        && x.fields == y.fields
        && x.accessors == y.accessors
        && x.overloads == y.overloads
        && x.generics == y.generics
        && x.constructor_methods == y.constructor_methods
        && x.constraint_type_arg_subs == y.constraint_type_arg_subs
        && x.field_built_names == y.field_built_names
        && x.is_enum == y.is_enum
        && x.is_key_enum == y.is_key_enum
        && x.correlated_groups == y.correlated_groups
}

pub(super) fn classes_match(a: &[ClassDecl], b: &[ClassDecl]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| class_semantic_eq(x, y))
}

/// Compare two alias declarations ignoring positional fields (def_range, def_path).
// IMPORTANT: Update this function when adding semantic fields to AliasDecl.
pub(super) fn alias_semantic_eq(x: &AliasDecl, y: &AliasDecl) -> bool {
    x.name == y.name
        && x.type_params == y.type_params
        && x.typ == y.typ
        && x.is_opaque == y.is_opaque
}

pub(super) fn aliases_match(a: &[AliasDecl], b: &[AliasDecl]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| alias_semantic_eq(x, y))
}

/// Collect declaration names that differ between the old and new slice, keyed by
/// `name` (not positional index, since edits can reorder/insert/remove entries).
/// A name is "changed" if it is added, removed, or any of its same-named entries
/// differ semantically. Over-approximation is safe — these names seed the
/// reverse-dependency closure that decides which files to re-analyze.
pub(super) fn diff_changed_names<T, F>(old: &[T], new: &[T], name_of: impl Fn(&T) -> &str, eq: F) -> HashSet<String>
where
    F: Fn(&T, &T) -> bool,
{
    use std::collections::HashMap;
    let group = |items: &[T]| -> HashMap<String, Vec<usize>> {
        let mut m: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, it) in items.iter().enumerate() {
            m.entry(name_of(it).to_string()).or_default().push(i);
        }
        m
    };
    let old_groups = group(old);
    let new_groups = group(new);
    let mut changed = HashSet::new();
    // Collect the union of keys once to avoid visiting names present in both
    // groups twice (the old chain approach relied on a `changed.contains`
    // guard to skip the duplicate).
    let all_names: HashSet<&String> = old_groups.keys().chain(new_groups.keys()).collect();
    for name in all_names {
        let o = old_groups.get(name);
        let n = new_groups.get(name);
        let differs = match (o, n) {
            (Some(oi), Some(ni)) => {
                oi.len() != ni.len()
                    || oi.iter().zip(ni.iter()).any(|(&a, &b)| !eq(&old[a], &new[b]))
            }
            _ => true, // present on only one side: added or removed
        };
        if differs {
            changed.insert(name.clone());
        }
    }
    changed
}

pub(super) fn globals_changed_names(old: &[ExternalGlobal], new: &[ExternalGlobal]) -> HashSet<String> {
    diff_changed_names(old, new, |g| g.name.as_str(), global_semantic_eq)
}

pub(super) fn classes_changed_names(old: &[ClassDecl], new: &[ClassDecl]) -> HashSet<String> {
    diff_changed_names(old, new, |c| c.name.as_str(), class_semantic_eq)
}

pub(super) fn aliases_changed_names(old: &[AliasDecl], new: &[AliasDecl]) -> HashSet<String> {
    diff_changed_names(old, new, |a| a.name.as_str(), alias_semantic_eq)
}

/// Build a reverse-dependency graph: maps a type name → the set of declaration
/// names that reference it. E.g. a class `Foo` with a field typed `Bar` produces
/// an edge `Bar → Foo`, so when `Bar` changes we know `Foo` is affected even
/// though `Foo`'s own source may not mention `Bar` by name in a way the textual
/// filter would catch. Used to expand the set of changed declarations into the
/// full set of declarations whose resolved types could shift.
pub(super) fn build_reverse_dep_graph<'a>(
    classes: impl IntoIterator<Item = &'a ClassDecl>,
    aliases: impl IntoIterator<Item = &'a AliasDecl>,
    globals: impl IntoIterator<Item = &'a ExternalGlobal>,
) -> HashMap<String, HashSet<String>> {
    let mut rev: HashMap<String, HashSet<String>> = HashMap::new();
    for c in classes {
        let mut names = HashSet::new();
        crate::annotations::class_referenced_names(c, &mut names);
        for r in names {
            if r != c.name {
                rev.entry(r).or_default().insert(c.name.clone());
            }
        }
    }
    for a in aliases {
        let mut names = HashSet::new();
        crate::annotations::collect_referenced_type_names(&a.typ, &mut names);
        for r in names {
            if r != a.name {
                rev.entry(r).or_default().insert(a.name.clone());
            }
        }
    }
    // Globals: if a global function's @param/@return references a class/alias,
    // files calling that global (mentioning its name) must be re-analyzed when
    // the referenced declaration changes.
    for g in globals {
        let mut names = HashSet::new();
        crate::annotations::global_referenced_names(g, &mut names);
        for r in names {
            if r != g.name {
                rev.entry(r).or_default().insert(g.name.clone());
            }
        }
    }
    rev
}

/// Transitive closure of `seed` over the reverse-dependency graph: every name that
/// is reachable from a changed name by following "is referenced by" edges. The
/// result is the full set of declaration names whose diagnostics could change.
pub(super) fn expand_affected_names(
    seed: HashSet<String>,
    rev: &HashMap<String, HashSet<String>>,
) -> HashSet<String> {
    let mut result: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = seed.into_iter().collect();
    while let Some(name) = stack.pop() {
        if !result.insert(name.clone()) {
            continue;
        }
        if let Some(deps) = rev.get(&name) {
            for d in deps {
                if !result.contains(d) {
                    stack.push(d.clone());
                }
            }
        }
    }
    result
}

/// Compare event declarations ignoring positional fields (def_range, def_path)
/// and display-only fields (documentation).
// IMPORTANT: Update this function when adding semantic fields to EventDecl.
pub(super) fn events_match(a: &[EventDecl], b: &[EventDecl]) -> bool {
    if a.len() != b.len() { return false; }
    a.iter().zip(b.iter()).all(|(x, y)| {
        x.event_type == y.event_type
            && x.event_name == y.event_name
            && x.params == y.params
    })
}

/// Compare self-field declarations ignoring positional field (byte_range).
// IMPORTANT: Update this function when adding semantic fields to TypedSelfField.
pub(super) fn self_fields_match(a: &[TypedSelfField], b: &[TypedSelfField]) -> bool {
    if a.len() != b.len() { return false; }
    a.iter().zip(b.iter()).all(|(x, y)| {
        x.class_name == y.class_name
            && x.field_name == y.field_name
            && x.annotation_type == y.annotation_type
            && x.visibility == y.visibility
            && x.inferred == y.inferred
    })
}

impl RebuildScope {
    pub(super) fn is_rebuild(&self) -> bool {
        !matches!(self, RebuildScope::None)
    }

    /// Merge another scope into this one, taking the more conservative of the two.
    /// Precedence: `None` < `Incremental` < `Full`; two `Incremental`s union their
    /// name sets.
    pub(super) fn merge(self, other: RebuildScope) -> RebuildScope {
        match (self, other) {
            (RebuildScope::Full, _) | (_, RebuildScope::Full) => RebuildScope::Full,
            (RebuildScope::Incremental(mut a), RebuildScope::Incremental(b)) => {
                a.extend(b);
                RebuildScope::Incremental(a)
            }
            (RebuildScope::Incremental(a), RebuildScope::None)
            | (RebuildScope::None, RebuildScope::Incremental(a)) => RebuildScope::Incremental(a),
            (RebuildScope::None, RebuildScope::None) => RebuildScope::None,
        }
    }
}

pub(super) fn maybe_rebuild_workspace(uri: &lsp_types::Uri, root: crate::syntax::SyntaxNode<'_>, ws: &mut WorkspaceState) -> RebuildScope {
    use crate::annotations::scan_defclass_calls;

    let file_path = match uri_to_path(uri, &ws.root) {
        Some(p) => p,
        None => return RebuildScope::None,
    };

    let synth = ws.configs.correlated_return_overloads_for(&file_path);
    let ipp = ws.configs.implicit_protected_prefix_for(&file_path);
    let (new_globals, addon_ns_class) = crate::annotations::scan_file_globals_with_synth(root, Some(&file_path), synth, ipp);
    if let Some(name) = addon_ns_class {
        ws.ws_file_addon_ns_class.insert(file_path.clone(), name);
    } else {
        ws.ws_file_addon_ns_class.remove(&file_path);
    }

    // Re-scan dynamic global prefixes for this file and update configs if changed.
    let new_prefixes = crate::annotations::scan_dynamic_global_prefixes(root);
    let old_prefixes = ws.ws_file_dynamic_prefixes.get(&file_path);
    let prefixes_changed = old_prefixes.map_or(!new_prefixes.is_empty(), |old| *old != new_prefixes);
    if prefixes_changed {
        if new_prefixes.is_empty() {
            ws.ws_file_dynamic_prefixes.remove(&file_path);
        } else {
            ws.ws_file_dynamic_prefixes.insert(file_path.clone(), new_prefixes);
        }
        let all_prefixes = super::scan::collect_all_dynamic_prefixes(&ws.ws_file_dynamic_prefixes);
        Arc::make_mut(&mut ws.configs).set_dynamic_global_prefixes(all_prefixes);
    }

    let mut scan = scan_all_annotations(root);
    // Attach file path to classes/aliases so class_locations/alias_locations
    // are populated during rebuild (matches what scan_lua_file does).
    for class in &mut scan.classes {
        if class.def_range.is_some() {
            class.def_path = Some(file_path.clone());
        }
    }
    for alias in &mut scan.aliases {
        if alias.def_range.is_some() {
            alias.def_path = Some(file_path.clone());
        }
    }
    for event in &mut scan.events {
        if event.def_range.is_some() {
            event.def_path = Some(file_path.clone());
        }
    }

    let globals_changed = ws.ws_file_globals.get(&file_path)
        .is_none_or(|old| !globals_match(old, &new_globals));
    let classes_changed = ws.ws_file_classes.get(&file_path)
        .is_none_or(|old| !classes_match(old, &scan.classes));
    let aliases_changed = ws.ws_file_aliases.get(&file_path)
        .is_none_or(|old| !aliases_match(old, &scan.aliases));
    // Events are removed from ws_file_events when empty, so None + empty = unchanged.
    let events_changed = ws.ws_file_events.get(&file_path)
        .map_or(!scan.events.is_empty(), |old| !events_match(old, &scan.events));

    // Compute the set of declaration names that changed (added/removed/modified),
    // for the incremental warm scope. For a brand-new file (no prior entry) every
    // declared name counts as changed. Must run before the inserts below move the
    // new values. These drive *which files* are re-analyzed, not *whether* we
    // rebuild — that is still decided by the `*_changed` booleans above.
    let changed_decl_names: HashSet<String> = {
        let mut names = HashSet::new();
        if globals_changed {
            match ws.ws_file_globals.get(&file_path) {
                Some(old) => names.extend(globals_changed_names(old, &new_globals)),
                None => names.extend(new_globals.iter().map(|g| g.name.clone())),
            }
        }
        if classes_changed {
            match ws.ws_file_classes.get(&file_path) {
                Some(old) => names.extend(classes_changed_names(old, &scan.classes)),
                None => names.extend(scan.classes.iter().map(|c| c.name.clone())),
            }
        }
        if aliases_changed {
            match ws.ws_file_aliases.get(&file_path) {
                Some(old) => names.extend(aliases_changed_names(old, &scan.aliases)),
                None => names.extend(scan.aliases.iter().map(|a| a.name.clone())),
            }
        }
        names
    };

    // Always store fresh values so positions stay current for hover/go-to-def.
    // Only rebuild when semantic content (types, names, fields) actually changed.
    ws.ws_file_globals.insert(file_path.clone(), new_globals);
    ws.ws_file_classes.insert(file_path.clone(), scan.classes);
    ws.ws_file_aliases.insert(file_path.clone(), scan.aliases);
    if scan.callable_classes.is_empty() {
        ws.ws_file_callable_classes.remove(&file_path);
    } else {
        ws.ws_file_callable_classes.insert(file_path.clone(), scan.callable_classes);
    }
    if scan.events.is_empty() {
        ws.ws_file_events.remove(&file_path);
    } else {
        ws.ws_file_events.insert(file_path.clone(), scan.events);
    }
    if globals_changed || classes_changed || aliases_changed || events_changed {
        ws.rebuild_caches();
    }

    // Re-scan for defclass/built-name discoveries. Builder chain changes
    // (e.g. AddOptionalClassField → AddDeferredClassField) change the discovered
    // fields without changing any exported globals/classes/aliases. Without this,
    // stale built class fields persist in PreResolvedGlobals until full reload.
    // Use cached merged vectors instead of cloning ~100K items per keystroke.
    //
    // Optimizations to avoid the ~25ms defclass scan cost on every keystroke:
    // 1. Quick text check: skip if the file doesn't contain any defclass/built-name
    //    function names as substrings. This eliminates the scan for ~90% of files.
    // 2. Skip if the file has syntax errors and declarations didn't change
    //    (prevents phantom rebuilds from broken ASTs).
    let declarations_changed = globals_changed || classes_changed || aliases_changed;
    let has_syntax_errors = !root.tree.errors.is_empty();

    // Quick substring check: does the file text contain any defclass/built-name func names?
    let source = root.tree.source();
    let text_has_defclass = ws.cached_needs_defclass
        && ws.cached_defclass_func_names.iter().any(|name| source.contains(name.as_str()));
    let text_has_built_name = ws.cached_needs_built_name
        && ws.cached_built_name_func_names.iter().any(|name| source.contains(name.as_str()));
    let might_have_calls = text_has_defclass || text_has_built_name;

    // Skip the expensive scan when:
    // - File text doesn't contain any relevant function names, OR
    // - Declarations didn't change AND file has syntax errors (prevents phantom rebuilds)
    let skip_scan = !might_have_calls
        || (!declarations_changed && has_syntax_errors);

    let defclasses_changed = if skip_scan {
        // If we previously had results but the file no longer contains relevant calls,
        // clear the cache and trigger a rebuild.
        let had_results = ws.ws_file_defclasses.get(&file_path)
            .is_some_and(|old| !old.is_empty());
        if had_results && !might_have_calls {
            ws.ws_file_defclasses.insert(file_path.clone(), Vec::new());
            true
        } else {
            false
        }
    } else {
        let mut discovered = Vec::new();
        if text_has_defclass {
            discovered.extend(scan_defclass_calls(root, &ws.cached_all_globals, &ws.cached_all_classes, ipp));
        }
        if text_has_built_name {
            discovered.extend(scan_built_name_calls(root, &ws.cached_all_globals, ipp));
        }
        for decl in &mut discovered {
            if decl.def_range.is_some() || !decl.field_ranges.is_empty() {
                decl.def_path = Some(file_path.clone());
            }
        }
        let changed = ws.ws_file_defclasses.get(&file_path)
            .map_or(!discovered.is_empty(), |old| !classes_match(old, &discovered));
        ws.ws_file_defclasses.insert(file_path.clone(), discovered);
        changed
    };

    // Re-scan for self-field assignments (self.field = expr in methods).
    // Quick text check: only scan if the file contains "self." as a substring.
    let self_fields_changed = if !has_syntax_errors && source.contains("self.") {
        use crate::annotations::{scan_method_typed_self_fields, scan_method_funcall_self_fields, scan_method_bare_self_fields};
        let known_classes: HashSet<String> = ws.cached_all_classes.iter().map(|c| c.name.clone()).collect();
        if known_classes.is_empty() {
            false
        } else {
            let typed_field_names = collect_typed_field_names(ws.cached_all_classes.iter());
            let typed = scan_method_typed_self_fields(root, &known_classes, ipp);
            let funcall = scan_method_funcall_self_fields(
                root, &known_classes, ipp, &typed_field_names, Some(file_path.clone()),
            );
            let bare = scan_method_bare_self_fields(root, &known_classes, ipp, &typed_field_names);

            let new_self_fields = merge_self_field_results(typed, &funcall, bare);

            let sf_changed = ws.ws_file_self_fields.get(&file_path)
                .map_or(!new_self_fields.is_empty(), |old| !self_fields_match(old, &new_self_fields));
            let sfg_changed = ws.ws_file_self_field_globals.get(&file_path)
                .map_or(!funcall.is_empty(), |old| !globals_match(old, &funcall));
            if new_self_fields.is_empty() {
                ws.ws_file_self_fields.remove(&file_path);
            } else {
                ws.ws_file_self_fields.insert(file_path.clone(), new_self_fields);
            }
            if funcall.is_empty() {
                ws.ws_file_self_field_globals.remove(&file_path);
            } else {
                ws.ws_file_self_field_globals.insert(file_path.clone(), funcall);
            }
            sf_changed || sfg_changed
        }
    } else {
        // If file no longer contains "self.", clear any previous results
        let had_sf = ws.ws_file_self_fields.remove(&file_path).is_some();
        let had_sfg = ws.ws_file_self_field_globals.remove(&file_path).is_some();
        had_sf || had_sfg
    };

    if globals_changed || classes_changed || aliases_changed || defclasses_changed || self_fields_changed || events_changed {
        log::info!(
            "Workspace rebuild triggered by didOpen: {} (globals={} classes={} aliases={} defclasses={} self_fields={} events={})",
            file_path.display(),
            globals_changed,
            classes_changed,
            aliases_changed,
            defclasses_changed,
            self_fields_changed,
            events_changed,
        );
        ws.rebuild();
        // defclass/self-field/event changes are hard to express as a precise set
        // of changed declaration names (they flow through builder chains and
        // method bodies), so fall back to a Full warm in those cases. When only
        // class/global/alias declarations changed, the reverse-dependency closure
        // over `changed_decl_names` is sufficient and we can warm incrementally.
        if defclasses_changed || self_fields_changed || events_changed {
            RebuildScope::Full
        } else {
            RebuildScope::Incremental(changed_decl_names)
        }
    } else {
        RebuildScope::None
    }
}
