use super::*;

/// Scan all `.lua` files under `dir` for global function definitions that have
/// `---@return` annotations.  Returns the set of function names that already have
/// return type annotations (and thus should not be overridden by inferred stubs).
pub(in crate::stub_gen) fn get_functions_with_return(dir: &Path) -> HashSet<String> {
    let func_re = regex_lite::Regex::new(r"(?m)^function\s+([A-Za-z_]\w*)\s*\(").unwrap();
    let mut result = HashSet::new();
    let mut lua_files = Vec::new();
    collect_lua_paths(dir, &mut lua_files);
    for path in &lua_files {
        if let Ok(content) = std::fs::read_to_string(path) {
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if let Some(cap) = func_re.captures(line) {
                    let name = cap.get(1).unwrap().as_str();
                    // Look backward from this function definition for ---@return
                    // in the preceding annotation block (consecutive --- lines).
                    let mut j = i;
                    while j > 0 {
                        j -= 1;
                        let prev = lines[j].trim();
                        if prev.starts_with("---") {
                            if prev.starts_with("---@return") {
                                result.insert(name.to_string());
                                break;
                            }
                        } else {
                            break; // End of annotation block
                        }
                    }
                }
            }
        }
    }
    result
}


/// Extract return types and parameter names from a resolved function.
/// Returns `None` if inference produced nothing useful (empty, all any/nil).
pub(in crate::stub_gen) fn extract_inferred_return(
    ar: &crate::analysis::AnalysisResult,
    func: &crate::types::Function,
) -> Option<InferredReturn> {
    use crate::types::SymbolIdentifier;

    let return_types: Vec<String> = if !func.return_annotations.is_empty() {
        func.return_annotations.iter()
            .map(|vt| ar.format_type_depth(vt, 1))
            .collect()
    } else {
        ar.format_inferred_returns(func, 1)
    };

    if return_types.is_empty()
        || return_types.iter().all(|t| t == "any" || t == "?" || t == "nil")
    {
        return None;
    }

    let mut params: Vec<String> = func.args.iter().filter_map(|&arg_idx| {
        if arg_idx.is_external() { return None; }
        Some(match &ar.ir.sym(arg_idx).id {
            SymbolIdentifier::Name(n) => n.clone(),
            _ => "_".to_string(),
        })
    }).collect();
    // `func.args` holds only the named parameters; a trailing `...` is tracked
    // separately on `is_vararg`. Re-append it so the emitted stub keeps the
    // vararg (e.g. `GenerateClosure(f, ...)`, `LinkUtil.FormatLink(t, d, ...)`,
    // `DataProviderMixin:Remove(...)`) — otherwise callers passing extra args
    // get a spurious `redundant-parameter`.
    if func.is_vararg {
        params.push("...".to_string());
    }

    // A body like `return string.match(...)` tail-calls a function whose return
    // is itself unbounded (vararg). The per-file engine treats such a return as
    // open-ended (no over-destructure warning), but a frozen single-slot stub
    // loses that — so emit a matching `...T` vararg return for the generated stub.
    let returns = match tail_call_vararg_elem(ar, func, &return_types) {
        Some(elem) => vec![format!("...{elem}")],
        None => return_types,
    };

    Some(InferredReturn { params, returns })
}

/// When `func`'s body is a single tail call `return f(...)` whose callee `f` has
/// an unbounded (vararg) return, returns the vararg element type. The defining
/// file's engine reports this function's arity as open-ended (a tail call may
/// yield more values than the one inferred slot, see `destructure_arity`), so the
/// generated stub must carry a `...T` vararg return to stay consistent — otherwise
/// a cross-file `local a, b = f()` over-destructure false-positives. Returns
/// `None` for any non-tail-call or fixed-arity-callee body (those keep their
/// precise inferred returns).
fn tail_call_vararg_elem(
    ar: &crate::analysis::AnalysisResult,
    func: &crate::types::Function,
    return_types: &[String],
) -> Option<String> {
    use crate::types::{SymbolIdentifier, Expr, ValueType};

    // Only the collapsed single-slot case (a multi-slot inference is already precise).
    if return_types.len() != 1 { return None; }
    if func.rets.is_empty() { return None; }

    // Every return must be a lone slot-0 `FunctionCall(ret_index 0)` to a vararg
    // callee. Requiring slot 0 on every `rets` entry also rejects multi-value
    // returns (`return a, b` emits a slot-1 FunctionRet) and non-tail-call returns
    // — each return statement contributes exactly one slot-0 symbol.
    for &sym_idx in &func.rets {
        let sym = ar.ir.sym(sym_idx);
        let SymbolIdentifier::FunctionRet(_, 0) = sym.id else { return None };
        let ver = sym.versions.first()?;
        let Expr::FunctionCall { func: callee_expr, ret_index: 0, .. } = ar.ir.expr(ver.type_source?)
        else { return None };
        let callee_idx = match ar.resolved_expr_cache_get(*callee_expr)?.clone().into_strip_opaque() {
            ValueType::Function(Some(idx)) => idx,
            ValueType::Union(types) | ValueType::Intersection(types) => {
                types.into_iter().find_map(|t| match t {
                    ValueType::Function(Some(idx)) => Some(idx),
                    _ => None,
                })?
            }
            _ => return None,
        };
        let callee = ar.ir.func(callee_idx);
        let unbounded = callee.has_vararg_return
            || callee.overloads.iter().any(|o| !o.is_return_only && o.has_vararg_tail);
        if !unbounded { return None; }
    }

    // Use the inferred slot-0 type as the vararg element (drop a trailing `?` —
    // the vararg tail itself is optional, so per-slot nilability is redundant).
    let elem = return_types[0].trim_end_matches('?').trim();
    if elem.is_empty() || elem == "any" || elem == "nil" { return None; }
    Some(elem.to_string())
}


/// Run the analysis engine on FrameXML Lua source files to infer return types
/// for global functions that lack `@return` annotations in the vendor stubs.
///
/// This is strictly more powerful than regex-based pattern matching — it catches
/// `CreateFromMixins`, `setmetatable` factories, tail calls through annotated
/// functions, and any other pattern the type inference engine handles.
pub(in crate::stub_gen) fn infer_fxml_return_types(
    ui_source_dir: &Path,
    pre_globals: std::sync::Arc<crate::pre_globals::PreResolvedGlobals>,
    needs_return: &HashSet<String>,
    util_table_names: &HashSet<String>,
) -> HashMap<String, InferredReturn> {
    use rayon::prelude::*;
    use crate::analysis::{Analysis, AnalysisConfig};
    use crate::ast::{AstNode, FunctionDefinition};
    use crate::syntax::SyntaxNode;
    use crate::types::{SymbolIdentifier, ValueType};

    if needs_return.is_empty() && util_table_names.is_empty() {
        return HashMap::new();
    }

    let interface_dir = ui_source_dir.join("Interface");
    if !interface_dir.is_dir() {
        return HashMap::new();
    }

    let mut lua_files = Vec::new();
    collect_lua_paths(&interface_dir, &mut lua_files);

    // Matches top-level global assignments: `GlobalName = expr` or dotted field
    // assignments `GlobalName.Field = expr`.  Lines matching this regex are
    // commented out before analysis, so the engine resolves these names from
    // precomputed stubs (which have proper @class / @return annotations)
    // instead of the source-level assignments.  Without this, source lines like
    //   `AnchorUtil.CreateAnchor = GenerateClosure(CreateAndInitFromMixin, AnchorMixin)`
    // shadow the stub's typed `@return AnchorMixin` definition.
    // Limitation: matches any column-0 uppercase assignment regardless of scope
    // nesting.  This is acceptable for Blizzard FrameXML which conventionally
    // indents code inside function bodies/do-end blocks, so column-0 uppercase
    // assignments are reliably top-level definitions.
    let global_assign_re = regex_lite::Regex::new(r"^[A-Z]\w+(?:\.\w+)*\s*=\s").unwrap();

    // Pre-filter regex: quickly check if a file contains any global function
    // or table method definition (column-0 `function Name(` or `function Name.`
    // or `function Name:`).  Files without this pattern can't define any
    // function we care about, avoiding expensive analysis.
    let func_def_re = regex_lite::Regex::new(r"(?m)^function [A-Z]").unwrap();

    // Analyze files in parallel — each file gets its own Analysis instance
    // with a shared (Arc) copy of PreResolvedGlobals.  The filter + analysis
    // runs in a single pass to avoid double file reads.
    let per_file_results: Vec<Vec<(String, InferredReturn)>> = lua_files.par_iter().map(|path| {
        let Ok(raw_content) = std::fs::read_to_string(path) else { return vec![] };

        // Quick pre-filter: skip files with no global function definitions.
        if !func_def_re.is_match(&raw_content) {
            return vec![];
        }

        // Comment out top-level global assignments to prevent shadowing of
        // precomputed stub classes. FrameXML source defines mixin globals via
        // chains like `TreeDataProviderMixin = CreateFromMixins(CallbackRegistryMixin)`,
        // which collapses class types to the chain root. By removing these, the
        // analysis resolves mixin names from the stubs (where they have @class types).
        let content: String = raw_content.lines()
            .map(|line| {
                if global_assign_re.is_match(line) {
                    format!("--{line}")
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let tree = crate::syntax::parser::parse(&content);
        let mut analysis = Analysis::new_with_tree(&tree, pre_globals.clone(), AnalysisConfig::default());
        analysis.resolve_types();
        let ar = analysis.into_result();

        // Walk scope 0 symbols to find global function definitions.
        if ar.ir.local_scopes().next().is_none() {
            return vec![];
        }
        let mut file_results = Vec::new();
        for (sym_id, sym_idx) in ar.ir.scope0_local_symbols() {
            let SymbolIdentifier::Name(name) = sym_id else { continue };
            if !needs_return.contains(name) {
                continue;
            }
            // External symbols don't exist in per-file ir.symbols — bail out.
            if sym_idx.is_external() {
                continue;
            }
            let sym = ar.ir.sym(sym_idx);
            let Some(ver) = sym.versions.first() else { continue };
            let Some(ref resolved) = ver.resolved_type else { continue };
            let ValueType::Function(Some(func_idx)) = resolved else { continue };
            if func_idx.is_external() {
                continue;
            }
            let func = ar.ir.func(*func_idx);
            if let Some(inferred) = extract_inferred_return(&ar, func) {
                file_results.push((name.clone(), inferred));
            }
        }

        // Walk all local functions to find utility table methods (dotted/coloned
        // names like `function AnchorUtil.CreateAnchorFromPoint(...)`).
        if !util_table_names.is_empty() {
            for (_func_idx, func) in ar.ir.local_functions() {
                let Some(node_id) = func.def_node.node_id else { continue };
                let syntax_node = SyntaxNode { tree: &tree, id: node_id };
                let Some(func_def) = FunctionDefinition::cast(syntax_node) else { continue };
                let Some(ident) = func_def.identifier() else { continue };
                let names = ident.names();
                if names.len() < 2 { continue; }
                if !util_table_names.contains(&names[0]) { continue; }

                if let Some(inferred) = extract_inferred_return(&ar, func) {
                    let sep = if ident.is_call_to_self() { ":" } else { "." };
                    let table_name = &names[0];
                    let method_name = &names[names.len() - 1];
                    let key = format!("{table_name}{sep}{method_name}");
                    file_results.push((key, inferred));
                }
            }
        }

        file_results
    }).collect();

    per_file_results.into_iter().flatten().collect()
}


/// Discover undeclared field accesses on known stub classes by analyzing
/// wow-ui-source Lua files via **structural matching**.
///
/// When a variable is untyped (`any` or unresolved) — common for callback
/// parameters in Blizzard's source — field accesses are grouped by variable
/// name and matched against known class field sets.  If enough of a
/// variable's accessed fields are declared on a single class (≥ 3 or ≥ 25%,
/// whichever is larger), the remaining undeclared fields are attributed to
/// that class.  This catches runtime fields like `TooltipDataLine.gemIcon`
/// that are populated by C++ (via `TooltipUtil.SurfaceArgs`) but read in Lua
/// without type annotations.
///
/// Returns a map from class name → set of undeclared field names.
pub(in crate::stub_gen) fn discover_runtime_fields(
    ui_source_dir: &Path,
    pre_globals: std::sync::Arc<crate::pre_globals::PreResolvedGlobals>,
) -> HashMap<String, HashSet<String>> {
    use rayon::prelude::*;
    use crate::analysis::{Analysis, AnalysisConfig};
    use crate::types::{Expr, ValueType};

    let interface_dir = ui_source_dir.join("Interface");
    if !interface_dir.is_dir() {
        return HashMap::new();
    }

    let mut lua_files = Vec::new();
    collect_lua_paths(&interface_dir, &mut lua_files);

    // Build class → declared field names map from pre_globals.
    // Include own fields + direct parent fields (one level).  Grandparent+
    // fields are not walked — this is fine for the small data structures we
    // target (e.g. TooltipDataLine has no deep inheritance), and avoids the
    // complexity of a transitive closure over the full class hierarchy.
    let class_field_map: HashMap<String, HashSet<String>> = {
        let mut map: HashMap<String, HashSet<String>> = HashMap::new();
        for (class_name, &table_idx) in &pre_globals.classes {
            let Some(table) = pre_globals.try_table(table_idx) else { continue };
            let fields: &mut HashSet<String> = map.entry(class_name.clone()).or_default();
            for field_name in table.fields.keys() {
                fields.insert(field_name.clone());
            }
            // Include parent class fields (for structural matching accuracy)
            for &parent_idx in &table.parent_classes {
                if !parent_idx.is_external() { continue; }
                let Some(parent) = pre_globals.try_table(parent_idx) else { continue };
                for field_name in parent.fields.keys() {
                    fields.insert(field_name.clone());
                }
            }
        }
        map
    };
    // Only consider classes with ≥ 3 declared fields (enough for meaningful matching)
    let matchable_classes: Vec<(&String, &HashSet<String>)> = class_field_map.iter()
        .filter(|(_, fields)| fields.len() >= 3)
        .collect();

    // Same source cleanup as infer_fxml_return_types: comment out top-level
    // global assignments to prevent shadowing of precomputed stub class types.
    let global_assign_re = regex_lite::Regex::new(r"^[A-Z]\w+\s*=\s").unwrap();

    let per_file: Vec<HashMap<String, HashSet<String>>> = lua_files.par_iter().map(|path| {
        let Ok(raw_content) = std::fs::read_to_string(path) else { return HashMap::new() };

        let content: String = raw_content.lines()
            .map(|line| {
                if global_assign_re.is_match(line) {
                    format!("--{line}")
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let tree = crate::syntax::parser::parse(&content);
        let mut analysis = Analysis::new_with_tree(&tree, pre_globals.clone(), AnalysisConfig::default());
        analysis.resolve_types();
        let ar = analysis.into_result();

        let mut discovered: HashMap<String, HashSet<String>> = HashMap::new();

        // ── Structural matching for any-typed variables ──
        // Group field accesses by variable NAME (not index) so that parameters
        // with the same name across different functions in the same file are
        // merged.  E.g. all `lineData` params in TooltipDataRules.lua contribute
        // their accessed fields to a single group.
        let mut any_var_fields: HashMap<String, HashSet<String>> = HashMap::new();
        for (_, expr) in ar.ir.local_exprs() {
            let Expr::FieldAccess { table, field, .. } = expr else { continue };
            let table_type = ar.resolve_expr_type(*table);
            // Include both Any and None (unresolved) — untyped function
            // parameters resolve to None, not Any.
            match &table_type {
                None | Some(ValueType::Any) => {}
                _ => continue,
            }
            // Check if the table expression is a simple variable reference
            if let Expr::SymbolRef(sym_idx, _) = ar.ir.expr(*table) {
                if sym_idx.is_external() { continue; }
                let sym = ar.ir.sym(*sym_idx);
                if let crate::types::SymbolIdentifier::Name(ref name) = sym.id {
                    any_var_fields.entry(name.clone()).or_default().insert(field.clone());
                }
            }
        }

        // Match each variable name's field set against known classes
        for accessed_fields in any_var_fields.values() {
            if accessed_fields.len() < 3 { continue; }
            // Find the class with the highest overlap.  Require at least 3
            // matching declared fields, or ⌊25%⌋ of the accessed field count
            // (whichever is larger).  For small field sets (3–11 fields) the
            // floor is always 3; the 25% rule only kicks in at 13+ fields.
            // This prevents false matches from variables that access dozens of
            // unrelated fields and accidentally overlap with a large class.
            let min_overlap = 3.max(accessed_fields.len() / 4);
            let mut best: Option<(&str, usize)> = None; // (class_name, overlap_count)
            for (class_name, declared_fields) in &matchable_classes {
                // Skip large frame/mixin classes (> 50 declared fields) —
                // they have so many fields that incidental overlaps are common.
                // Data structures we're targeting (like TooltipDataLine) have
                // few declared fields.
                if declared_fields.len() > 50 { continue; }
                let overlap = accessed_fields.iter()
                    .filter(|f| declared_fields.contains(f.as_str()))
                    .count();
                if overlap >= min_overlap {
                    let dominated = match &best {
                        None => true,
                        Some((_, prev)) => overlap > *prev,
                        // Tiebreak alphabetically for determinism
                    };
                    let tied = matches!(&best, Some((_, prev)) if overlap == *prev);
                    if dominated || (tied && class_name.as_str() < best.unwrap().0) {
                        best = Some((class_name, overlap));
                    }
                }
            }
            if let Some((class_name, _overlap)) = best {
                let declared = &class_field_map[class_name];
                for field in accessed_fields {
                    if !declared.contains(field) {
                        discovered.entry(class_name.to_string())
                            .or_default()
                            .insert(field.clone());
                    }
                }
            }
        }

        discovered
    }).collect();

    // Merge per-file results
    let mut all: HashMap<String, HashSet<String>> = HashMap::new();
    for file_result in per_file {
        for (class, fields) in file_result {
            all.entry(class).or_default().extend(fields);
        }
    }

    all
}


/// Generate partial `@class` stubs for runtime fields discovered from
/// wow-ui-source analysis.  Each undeclared field is emitted as `any?`
/// (optional, untyped) since the fields are conditionally populated at
/// runtime and their types can't be reliably inferred from usage alone.
pub(in crate::stub_gen) fn generate_inferred_field_stubs(
    discovered: &HashMap<String, HashSet<String>>,
) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    writeln!(out, "---@meta _").unwrap();
    writeln!(out, "-- Runtime fields discovered from wow-ui-source (auto-generated by analysis engine)").unwrap();
    writeln!(out).unwrap();

    let mut classes: Vec<&String> = discovered.keys().collect();
    classes.sort();
    for class in &classes {
        let field_set = &discovered[*class];
        let mut sorted_fields: Vec<&String> = field_set.iter().collect();
        sorted_fields.sort();
        writeln!(out, "---@class {}", class).unwrap();
        for field in &sorted_fields {
            writeln!(out, "---@field {} any?", field).unwrap();
        }
        writeln!(out).unwrap();
    }

    log::info!("  InferredFields: {} classes, {} total fields",
        discovered.len(),
        discovered.values().map(|f| f.len()).sum::<usize>());
    out
}


/// Generate override stubs for FrameXML functions whose return types were
/// inferred by the analysis engine.  Only emits stubs for functions whose
/// existing vendor definition lacks a `@return` annotation.  Forwards any
/// existing `@param` annotations from the pass 1 globals so the override
/// doesn't drop typed parameter information.
pub(in crate::stub_gen) fn generate_inferred_return_stubs(
    inferred: &HashMap<String, InferredReturn>,
    stubs_dirs: &[&Path],
    pass1_globals: &[crate::annotations::ExternalGlobal],
) -> String {
    if inferred.is_empty() {
        return "---@meta _\n".to_string();
    }
    use crate::annotations::ParamInfo;
    use crate::annotations::annotation_types::format_annotation_type;

    // Find functions that already have @return annotations in vendor stubs
    // and generated files (e.g. GlobalColors.lua defines CreateColor with
    // @return colorRGBA — without scanning gen_dir those would be overridden
    // by inferred types from the FrameXML source body).
    let mut already_annotated = HashSet::new();
    for dir in stubs_dirs {
        already_annotated.extend(get_functions_with_return(dir));
    }

    // Build a lookup from pass 1 globals: name → params, for forwarding
    // existing @param annotations into the generated override stubs.
    let mut vendor_params: HashMap<&str, &[ParamInfo]> = HashMap::new();
    for g in pass1_globals {
        if !g.params.is_empty() {
            vendor_params.insert(&g.name, &g.params);
        }
    }

    let mut lines = vec![
        "---@meta _".to_string(),
        "-- FrameXML function return types (auto-inferred by analysis engine)".to_string(),
        String::new(),
    ];
    let mut names: Vec<&String> = inferred.keys()
        .filter(|n| !already_annotated.contains(n.as_str()))
        .collect();
    names.sort();
    for name in &names {
        let info = &inferred[*name];
        // Forward existing @param annotations from vendor stubs so the
        // override doesn't drop typed parameter information.
        if let Some(params) = vendor_params.get(name.as_str()) {
            for p in *params {
                let opt = if p.optional { "?" } else { "" };
                let typ = format_annotation_type(&p.typ);
                lines.push(format!("---@param {}{opt} {typ}", p.name));
            }
        }
        for ret in &info.returns {
            lines.push(format!("---@return {ret}"));
        }
        let params_str = info.params.join(", ");
        lines.push(format!("function {name}({params_str}) end"));
        lines.push(String::new());
    }
    log::info!("  InferredReturns: {} functions with inferred return types ({} skipped, already annotated)",
        names.len(), inferred.len() - names.len());
    lines.join("\n") + "\n"
}


/// Generate stub annotations for FrameXML utility tables and mixin classes.
///
/// For mixin tables (those with colon methods): emits `@class` + methods.
/// For utility tables (dot methods only): emits table + functions.
/// For `GenerateClosure(CreateAndInitFromMixin, Mixin)` factories: emits `@return Mixin`
/// with params copied from `Mixin:Init()` if found.
///
/// Tables whose name appears in `existing_names` are skipped to avoid conflicting
/// with hand-written overrides that provide richer type annotations.
/// Returns `(lua_content, generated_names)` where `generated_names` is the set
/// of table names that were actually emitted (passed the dedup filter).
pub(in crate::stub_gen) fn generate_framexml_utility_stubs(
    util_tables: &HashMap<String, UtilTableInfo>,
    existing_names: &HashSet<String>,
) -> (String, HashSet<String>) {
    use std::fmt::Write;
    let mut out = String::new();
    writeln!(out, "---@meta _").unwrap();
    writeln!(out, "-- FrameXML utility tables and mixins (auto-generated from wow-ui-source)").unwrap();
    writeln!(out, "-- For type-refined versions, add an override in stubs/overrides/.").unwrap();
    writeln!(out).unwrap();

    // Sort alphabetically for deterministic output.
    let mut names: Vec<&String> = util_tables.keys()
        .filter(|name| !existing_names.contains(name.as_str()))
        .collect();
    names.sort();

    let mut count = 0usize;
    let mut generated_names = HashSet::new();

    for name in &names {
        let info = &util_tables[*name];

        if info.is_mixin {
            writeln!(out, "---@class {name}").unwrap();
        }
        // All generated tables (mixin and non-mixin) need a global declaration
        // so addon code referencing these names doesn't get `undefined-global`.
        // GlobalVariables.lua excludes names with generated utility stubs.
        if !info.methods.is_empty() || !info.factory_closures.is_empty() || info.is_mixin {
            writeln!(out, "{name} = {{}}").unwrap();
        }

        // Emit methods sorted for determinism.
        let mut methods = info.methods.clone();
        methods.sort_by(|a, b| a.name.cmp(&b.name));
        for method in &methods {
            let params_str = method.params.join(", ");
            let sep = if method.is_method { ":" } else { "." };
            writeln!(out, "function {name}{sep}{}({params_str}) end", method.name).unwrap();
        }

        // Emit factory closures with @return and Init params.
        let mut factories = info.factory_closures.clone();
        factories.sort_by(|a, b| a.field_name.cmp(&b.field_name));
        for factory in &factories {
            // Skip if the factory function itself is already defined in overrides.
            let factory_fqn = format!("{name}.{}", factory.field_name);
            if existing_names.contains(&factory_fqn) {
                continue;
            }
            let params_str = find_init_params(util_tables, &factory.mixin_name);
            writeln!(out, "---@return {}", factory.mixin_name).unwrap();
            writeln!(out, "function {name}.{}({params_str}) end", factory.field_name).unwrap();
        }

        writeln!(out).unwrap();
        generated_names.insert((*name).clone());
        count += 1;
    }

    log::info!("  FrameXMLUtilities: {} utility tables/mixins generated", count);
    (out, generated_names)
}


/// Look up the `Init()` method's params for a mixin, to use for factory closures.
pub(in crate::stub_gen) fn find_init_params(util_tables: &HashMap<String, UtilTableInfo>, mixin_name: &str) -> String {
    if let Some(info) = util_tables.get(mixin_name)
        && let Some(init) = info.methods.iter().find(|m| m.name == "Init" && m.is_method)
    {
        return init.params.join(", ");
    }
    "...".to_string()
}

// ── Classic stubs generation (replaces generate_classic_stubs.py) ──────────────


/// Scan FrameXML Lua files for field/method assignments on known frame globals.
/// Returns a map of frame_name → sorted list of (field_name, type_string).
///
/// `mixin_to_frames` maps a mixin table name (e.g. `SpellBookFrameMixin`) to the
/// frames that mix it in via `<Frame mixin="...">`. When the scanner encounters
/// `function MixinName:method(...)` (or `MixinName.field = ...`), the resulting
/// field is attributed to every frame in that list, matching how Blizzard's
/// runtime `Mixin()` helper copies methods onto the instance.
pub(in crate::stub_gen) fn scan_framexml_lua_fields(
    ui_source_dirs: &[PathBuf],
    frame_names: &HashSet<String>,
    mixin_to_frames: &HashMap<String, Vec<String>>,
) -> HashMap<String, Vec<(String, String)>> {
    use rayon::prelude::*;
    // Per-frame field accumulator: frame_name → (field_name → type_str)
    let mut acc: HashMap<String, HashMap<String, String>> = HashMap::new();

    // 1. Field assignment: FrameName.field = rhs
    let field_re = regex_lite::Regex::new(
        r"(?m)^\s*([A-Z]\w+)\.(\w+)\s*=\s*(.+?)\s*$"
    ).unwrap();
    // 2. Method definition: function FrameName:method(...)
    let method_re = regex_lite::Regex::new(
        r"(?m)^\s*function\s+([A-Z]\w+):(\w+)\s*\("
    ).unwrap();
    // 3. Dot function definition: function FrameName.func(...)
    let dot_func_re = regex_lite::Regex::new(
        r"(?m)^\s*function\s+([A-Z]\w+)\.(\w+)\s*\("
    ).unwrap();
    // 4. PanelTemplates_SetNumTabs(FrameName, count) → injects .numTabs, .selectedTab
    //    Anchored to line start to avoid matching inside comments.
    let panel_tabs_re = regex_lite::Regex::new(
        r"(?m)^\s*PanelTemplates_SetNumTabs\s*\(\s*([A-Z]\w+)\s*,"
    ).unwrap();

    // Flatten all .lua files across every dir into one path list, preserving
    // dir order then `collect_lua_paths` order within each dir — the exact same
    // total order the nested serial loop visited. No sort: `par_iter` + `collect`
    // preserves this order so the sequential first-wins fold below is byte-identical.
    let mut lua_files = Vec::new();
    for dir in ui_source_dirs {
        let interface_dir = dir.join("Interface");
        if !interface_dir.is_dir() {
            continue;
        }
        collect_lua_paths(&interface_dir, &mut lua_files);
    }

    // Scan each file into a local per-frame accumulator in parallel, then fold
    // the locals into `acc` in path order with the same first-wins semantics.
    let partials: Vec<HashMap<String, HashMap<String, String>>> = lua_files
        .par_iter()
        .map(|path| {
            let mut local: HashMap<String, HashMap<String, String>> = HashMap::new();
            let Ok(content) = std::fs::read_to_string(path) else { return local };

            for cap in field_re.captures_iter(&content) {
                let name = cap.get(1).unwrap().as_str();
                let field = cap.get(2).unwrap().as_str();
                let rhs = cap.get(3).unwrap().as_str();
                let ftype = infer_rhs_type(rhs);
                attribute_field(&mut local, name, field, &ftype,
                    frame_names, mixin_to_frames);
            }

            for cap in method_re.captures_iter(&content) {
                let name = cap.get(1).unwrap().as_str();
                let method = cap.get(2).unwrap().as_str();
                attribute_field(&mut local, name, method, "function",
                    frame_names, mixin_to_frames);
            }

            for cap in dot_func_re.captures_iter(&content) {
                let name = cap.get(1).unwrap().as_str();
                let func = cap.get(2).unwrap().as_str();
                attribute_field(&mut local, name, func, "function",
                    frame_names, mixin_to_frames);
            }

            for cap in panel_tabs_re.captures_iter(&content) {
                let name = cap.get(1).unwrap().as_str();
                if !frame_names.contains(name) { continue; }
                let fields = local.entry(name.to_string()).or_default();
                fields.entry("numTabs".to_string())
                    .or_insert_with(|| "number".to_string());
                fields.entry("selectedTab".to_string())
                    .or_insert_with(|| "number".to_string());
            }
            local
        })
        .collect();

    for partial in partials {
        for (name, fields) in partial {
            let dst = acc.entry(name).or_default();
            for (field, ftype) in fields {
                dst.entry(field).or_insert(ftype);
            }
        }
    }

    // Convert to sorted Vec per frame
    acc.into_iter()
        .map(|(name, fields)| {
            let mut sorted: Vec<(String, String)> = fields.into_iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            (name, sorted)
        })
        .collect()
}


/// Record `field` of type `ftype` on `name` if it's a tracked frame, and on
/// every frame that mixes in `name` via `<Frame mixin="name">`. Existing field
/// types win — first writer keeps the slot.
pub(in crate::stub_gen) fn attribute_field(
    acc: &mut HashMap<String, HashMap<String, String>>,
    name: &str,
    field: &str,
    ftype: &str,
    frame_names: &HashSet<String>,
    mixin_to_frames: &HashMap<String, Vec<String>>,
) {
    if frame_names.contains(name) {
        acc.entry(name.to_string())
            .or_default()
            .entry(field.to_string())
            .or_insert_with(|| ftype.to_string());
    }
    if let Some(target_frames) = mixin_to_frames.get(name) {
        for frame in target_frames {
            if frame_names.contains(frame) {
                acc.entry(frame.clone())
                    .or_default()
                    .entry(field.to_string())
                    .or_insert_with(|| ftype.to_string());
            }
        }
    }
}


/// Infer a conservative type from a Lua RHS expression.
pub(in crate::stub_gen) fn infer_rhs_type(rhs: &str) -> String {
    let rhs = rhs.trim();
    // Strip trailing Lua comment
    let rhs = rhs.split("--").next().unwrap_or("").trim_end();

    if rhs.is_empty() || rhs == "nil" {
        return "any".to_string();
    }
    if rhs == "true" || rhs == "false" {
        return "boolean".to_string();
    }
    if rhs.starts_with("function") {
        return "function".to_string();
    }
    if rhs.starts_with('"') || rhs.starts_with('\'') || rhs.starts_with("[[") {
        return "string".to_string();
    }
    if rhs.starts_with('{') {
        return "table".to_string();
    }
    // Numeric literal
    let first = rhs.as_bytes()[0];
    if first.is_ascii_digit()
        || (first == b'-' && rhs.len() > 1 && rhs.as_bytes()[1].is_ascii_digit())
    {
        return "number".to_string();
    }

    "any".to_string()
}

// ── Classic stubs generation ──────────────────────────────────────────────────


/// Scan all .lua files under `Interface/` for top-level global constant assignments only.
/// Returns name → (type, value_literal).
///
/// Use `scan_interface_lua_combined` when you also need global function names, to avoid
/// a second traversal of the same directory tree.
pub(in crate::stub_gen) fn scan_framexml_constants(ui_source_dir: &Path) -> HashMap<String, (String, String)> {
    scan_interface_lua_combined(ui_source_dir).0
}


/// Scan FrameXML Lua source files for utility table and mixin definitions.
///
/// Discovers three patterns from column-0 definitions:
/// - `function Table.Method(params)` — dot methods on utility tables
/// - `function Table:Method(params)` — colon methods on mixin classes
/// - `Table.Field = GenerateClosure(CreateAndInitFromMixin, Mixin)` — factory closures
///
/// Returns a map of table name → discovered methods/factories. Tables with at least
/// one colon method are flagged as mixins (emitted as `@class` during generation).
/// Tables with no methods and no factory closures are pruned.
///
/// **Caller is responsible for ordering `ui_source_dirs` by priority** (retail first,
/// then classic flavors). The cross-file fold below is first-writer-wins, so listing
/// retail first keeps retail method signatures authoritative while classic branches
/// contribute only mixins/methods absent from retail (e.g. classic-only UI like
/// `AuctionPostMixin`). Ketho's stubs are retail-only, so scanning the classic clones
/// here is the sole source of classic mixin/template method coverage.
pub(in crate::stub_gen) fn scan_framexml_utility_tables(ui_source_dirs: &[&Path]) -> HashMap<String, UtilTableInfo> {
    use rayon::prelude::*;

    let dot_func_re = regex_lite::Regex::new(
        r"(?m)^function\s+([A-Z]\w+)\.(\w+)\s*\(([^)]*)\)"
    ).unwrap();
    let colon_method_re = regex_lite::Regex::new(
        r"(?m)^function\s+([A-Z]\w+):(\w+)\s*\(([^)]*)\)"
    ).unwrap();
    let factory_re = regex_lite::Regex::new(
        r"(?m)^([A-Z]\w+)\.(\w+)\s*=\s*GenerateClosure\s*\(\s*CreateAndInitFromMixin\s*,\s*([A-Z]\w+)\s*\)"
    ).unwrap();

    // Collect Lua files across every branch in priority order. Retail dirs listed
    // first win the first-writer-wins fold for any method defined in multiple branches.
    let mut lua_files = Vec::new();
    for dir in ui_source_dirs {
        let interface_dir = dir.join("Interface");
        if interface_dir.is_dir() {
            collect_lua_paths(&interface_dir, &mut lua_files);
        }
    }
    if lua_files.is_empty() {
        return HashMap::new();
    }

    // Scan each file into a local map in parallel, then fold with the same
    // dedup semantics (rayon preserves path order in collect).
    let partials: Vec<HashMap<String, UtilTableInfo>> = lua_files.par_iter().map(|path| {
        let mut tables: HashMap<String, UtilTableInfo> = HashMap::new();
        let Ok(content) = std::fs::read_to_string(path) else { return tables };

        for cap in dot_func_re.captures_iter(&content) {
            let table_name = cap.get(1).unwrap().as_str();
            let method_name = cap.get(2).unwrap().as_str();
            let params_raw = cap.get(3).unwrap().as_str();
            let params = parse_param_list(params_raw);

            let info = tables.entry(table_name.to_string()).or_default();
            // Avoid duplicate methods (same method defined across multiple classic/retail branches).
            if !info.methods.iter().any(|m| m.name == method_name && !m.is_method) {
                info.methods.push(UtilMethod {
                    name: method_name.to_string(),
                    params,
                    is_method: false,
                });
            }
        }

        for cap in colon_method_re.captures_iter(&content) {
            let table_name = cap.get(1).unwrap().as_str();
            let method_name = cap.get(2).unwrap().as_str();
            let params_raw = cap.get(3).unwrap().as_str();
            let params = parse_param_list(params_raw);

            let info = tables.entry(table_name.to_string()).or_default();
            info.is_mixin = true;
            if !info.methods.iter().any(|m| m.name == method_name && m.is_method) {
                info.methods.push(UtilMethod {
                    name: method_name.to_string(),
                    params,
                    is_method: true,
                });
            }
        }

        for cap in factory_re.captures_iter(&content) {
            let table_name = cap.get(1).unwrap().as_str();
            let field_name = cap.get(2).unwrap().as_str();
            let mixin_name = cap.get(3).unwrap().as_str();

            let info = tables.entry(table_name.to_string()).or_default();
            if !info.factory_closures.iter().any(|f| f.field_name == field_name) {
                info.factory_closures.push(FactoryClosure {
                    field_name: field_name.to_string(),
                    mixin_name: mixin_name.to_string(),
                });
            }
        }

        tables
    }).collect();

    let mut tables: HashMap<String, UtilTableInfo> = HashMap::new();
    // Persistent seen-sets per table name: track which (name, is_method) pairs and
    // factory-closure field names have already been added across all partials.
    // Avoids O(n²) linear scans of the accumulated Vec on each insert.
    let mut seen_methods: HashMap<String, HashSet<(String, bool)>> = HashMap::new();
    let mut seen_closures: HashMap<String, HashSet<String>> = HashMap::new();
    for partial in partials {
        for (table_name, src) in partial {
            let info = tables.entry(table_name.clone()).or_default();
            // OR-merge: a table with colon methods in ANY branch becomes a mixin.
            // In theory a retail dot-only namespace could be promoted if classic adds a
            // colon method, but in practice no such table exists across the three branches.
            info.is_mixin |= src.is_mixin;
            // Cross-file dedup: first file that defines a (name, is_method) pair
            // wins. Prevents duplicate @field stubs when the same method appears in
            // multiple FrameXML files. (The per-file loop does not perform this dedup
            // — it is new behavior introduced in the fold.)
            let methods_seen = seen_methods.entry(table_name.clone()).or_default();
            for m in src.methods {
                if methods_seen.insert((m.name.clone(), m.is_method)) {
                    info.methods.push(m);
                }
            }
            // Cross-file dedup: first file that defines a factory closure wins.
            let closures_seen = seen_closures.entry(table_name).or_default();
            for f in src.factory_closures {
                if closures_seen.insert(f.field_name.clone()) {
                    info.factory_closures.push(f);
                }
            }
        }
    }

    // Prune tables with no methods and no factory closures.
    tables.retain(|_, info| !info.methods.is_empty() || !info.factory_closures.is_empty());

    tables
}


/// Parse a comma-separated parameter list from a function signature.
pub(in crate::stub_gen) fn parse_param_list(raw: &str) -> Vec<String> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    raw.split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}


