use super::*;

impl PendingEditMap {
    /// Compose an existing `Single` map with a new edit (given in pending_text
    /// coordinates).  Returns an updated `Single` when the new edit is within
    /// or adjacent to the existing replacement region, otherwise falls back to
    /// `Prefix`.
    pub(super) fn compose_single(
        s: usize, oe: usize, d: isize,
        edit_start: usize, edit_end: usize, new_text_len: usize,
    ) -> PendingEditMap {
        // In pending_text the replacement region occupies [s, pt_end).
        debug_assert!(oe as isize + d >= 0, "edit map: pt_end underflow");
        let pt_end = (oe as isize + d) as usize;
        if edit_start >= s && edit_start <= pt_end {
            // New edit is within or adjacent to the existing replacement —
            // extend the Single map.
            let extra = edit_end.saturating_sub(pt_end);
            let new_oe = oe + extra;
            // Total replacement length in pending_text:
            //   kept prefix + new text + kept suffix
            let new_repl_len = (edit_start - s) + new_text_len + pt_end.saturating_sub(edit_end);
            let new_d = new_repl_len as isize - (new_oe - s) as isize;
            PendingEditMap::Single { start: s, old_end: new_oe, delta: new_d }
        } else {
            PendingEditMap::Prefix(s.min(edit_start))
        }
    }
}

/// Collect (class_name, field_name) pairs from all @field entries on the given classes.
/// Used to tell the self-field scan which fields are already declared.
pub(super) fn collect_typed_field_names<'a>(classes: impl Iterator<Item = &'a ClassDecl>) -> HashSet<(String, String)> {
    let mut names = HashSet::new();
    for class in classes {
        for (field_name, _, _) in &class.fields {
            names.insert((class.name.clone(), field_name.clone()));
        }
    }
    names
}

/// Merge typed + bare self-fields, skipping bare fields when a funcall field
/// covers the same (class, field) pair. Funcall fields take priority because
/// build_on_stubs resolves their return type through the normal call chain.
pub(super) fn merge_self_field_results(
    typed: Vec<TypedSelfField>,
    funcall: &[ExternalGlobal],
    bare: Vec<TypedSelfField>,
) -> Vec<TypedSelfField> {
    let funcall_field_names: HashSet<(String, String)> = funcall.iter()
        .filter_map(|g| {
            if let ExternalGlobalKind::TableField(_, fn_name, _) = &g.kind {
                Some((g.name.clone(), fn_name.clone()))
            } else {
                None
            }
        })
        .collect();
    let mut result = typed;
    for tsf in bare {
        if !funcall_field_names.contains(&(tsf.class_name.clone(), tsf.field_name.clone())) {
            result.push(tsf);
        }
    }
    result
}

/// Merge `@defclass` / `@built-name`-discovered `ClassDecl`s into an input set
/// of workspace `@class` overlays. When a defclass/built-name entry has the
/// same name as an existing overlay (or stub class), its data is merged into
/// the overlay; otherwise it becomes a new entry.
///
/// Must merge every field that affects `PreResolvedGlobals::build_on_stubs`
/// downstream — in particular `field_built_names`, which drives the Pass 3c
/// substitution that resolves per-subclass `@built-name` overrides on
/// inherited class-static fields (e.g. `_STATE_SCHEMA`). Dropping any of
/// these fields caused diagnostics to silently disappear in the LSP path.
/// `ClassDecl` fields not merged here (`accessors`, `overloads`, `generics`,
/// `type_params`, `constructor_methods`, `is_enum`, `correlated_groups`,
/// `def_range`, `def_path`) are never populated by `scan_defclass_calls` or
/// `scan_built_name_calls`, so there's nothing to merge.
pub(super) fn merge_defclass_into_overlays(
    mut ws_classes: Vec<ClassDecl>,
    stub_classes: &[ClassDecl],
    defclass_decls: Vec<&ClassDecl>,
) -> Vec<ClassDecl> {
    let class_names: HashSet<String> = stub_classes.iter().map(|c| c.name.clone())
        .chain(ws_classes.iter().map(|c| c.name.clone()))
        .collect();
    for decl in defclass_decls {
        if class_names.contains(&decl.name) {
            if let Some(existing) = ws_classes.iter_mut().find(|c| c.name == decl.name) {
                let overlay_names: HashSet<String> = existing.fields.iter()
                    .map(|(n, _, _)| n.clone()).collect();
                for field in &decl.fields {
                    if !overlay_names.contains(&field.0) {
                        existing.fields.push(field.clone());
                    }
                }
                for parent in &decl.parents {
                    if !existing.parents.contains(parent) {
                        existing.parents.push(parent.clone());
                    }
                }
                for sub in &decl.constraint_type_arg_subs {
                    if !existing.constraint_type_arg_subs.contains(sub) {
                        existing.constraint_type_arg_subs.push(sub.clone());
                    }
                }
                for (k, v) in &decl.field_built_names {
                    existing.field_built_names.entry(k.clone()).or_insert_with(|| v.clone());
                }
                for (name, range) in &decl.field_ranges {
                    existing.field_ranges.entry(name.clone()).or_insert(*range);
                }
                for (name, path) in &decl.field_paths {
                    existing.field_paths.entry(name.clone()).or_insert_with(|| path.clone());
                }
            }
        } else {
            ws_classes.push(decl.clone());
        }
    }
    ws_classes
}

impl WorkspaceState {
    /// Rebuild the cached merged globals/classes vectors from stubs + workspace data.
    /// Call this whenever ws_file_globals or ws_file_classes change.
    pub(super) fn rebuild_caches(&mut self) {
        self.cached_all_globals = self.stub_globals.iter()
            .chain(self.ws_file_globals.values().flatten())
            .cloned()
            .collect();
        self.cached_all_classes = self.stub_classes.iter()
            .chain(self.ws_file_classes.values().flatten())
            .cloned()
            .collect();
        self.cached_needs_defclass = self.stubs_have_defclass
            || self.ws_file_globals.values().flatten().any(|g| g.defclass.is_some());
        self.cached_needs_built_name = self.stubs_have_built_name
            || self.ws_file_globals.values().flatten().any(|g| g.built_name.is_some());

        // Extract unique function names for quick text-contains checks.
        // Use just the leaf method name (e.g. "DefineClass" from "Environment.DefineClass").
        let leaf_name = |g: &ExternalGlobal| -> Option<String> {
            match &g.kind {
                ExternalGlobalKind::Function => Some(g.name.split('.').next_back().unwrap_or(&g.name).to_string()),
                ExternalGlobalKind::Method(_, method_name, _) => Some(method_name.clone()),
                _ => None,
            }
        };
        let mut defclass_names: HashSet<String> = std::collections::HashSet::new();
        let mut built_name_names: HashSet<String> = std::collections::HashSet::new();
        // Track class names whose methods have @built-name, so we can find wrapper functions.
        let mut class_with_built_name_method: HashSet<String> = std::collections::HashSet::new();
        for g in &self.cached_all_globals {
            if g.defclass.is_some() && let Some(leaf) = leaf_name(g) {
                defclass_names.insert(leaf);
            }
            if g.built_name.is_some() {
                if matches!(&g.kind, ExternalGlobalKind::Method(_, _, _)) {
                    class_with_built_name_method.insert(g.name.clone());
                }
                if let Some(leaf) = leaf_name(g) { built_name_names.insert(leaf); }
            }
        }
        // Propagate: include wrapper functions whose return type is a class that has
        // a @built-name method. This mirrors the propagation in scan_built_name_calls().
        if !class_with_built_name_method.is_empty() {
            for g in self.cached_all_globals.iter().filter(|g| g.built_name.is_none()) {
                let is_wrapper = g.returns.first().is_some_and(|rt| {
                    if let AnnotationType::Simple(name) = rt {
                        class_with_built_name_method.contains(name)
                    } else {
                        false
                    }
                });
                if is_wrapper && let Some(leaf) = leaf_name(g) {
                    built_name_names.insert(leaf);
                }
            }
        }
        self.cached_defclass_func_names = defclass_names.into_iter().collect();
        self.cached_built_name_func_names = built_name_names.into_iter().collect();
        self.cached_callable_classes = self.ws_file_callable_classes.values().flatten().cloned().collect();
    }

    pub(super) fn rebuild(&mut self) {
        // Collect only workspace data (stubs are already in stub_pre_globals)
        let mut ws_globals: Vec<ExternalGlobal> = self.ws_file_globals.values().flatten()
            .cloned()
            .collect();
        // Collect workspace classes. Lua @class annotations take precedence over
        // XML-generated classes with the same name: XML classes whose name already
        // appears in a Lua file are routed through the overlay merge path so that
        // user-defined @field types are preserved.
        let (ws_classes_input, xml_overlay_classes) = partition_xml_overlay_classes(&self.ws_file_classes);
        let mut ws_aliases: Vec<AliasDecl> = self.ws_file_aliases.values().flatten()
            .cloned()
            .collect();

        let ws_events: Vec<crate::annotations::EventDecl> = self.ws_file_events.values().flatten().cloned().collect();
        crate::annotations::register_event_type_aliases(&mut ws_aliases, &ws_events);

        let defclass_decls: Vec<&ClassDecl> = self.ws_file_defclasses.values().flatten()
            .chain(xml_overlay_classes.iter())
            .collect();
        let mut ws_classes = merge_defclass_into_overlays(ws_classes_input, &self.stub_classes, defclass_decls);

        // Merge self-field scan results into classes and globals.
        // Typed + bare self-fields are added to ClassDecl.fields; funcall self-fields
        // become globals so build_on_stubs can resolve return types through the normal
        // funcall chain.
        if !self.ws_file_self_fields.is_empty() || !self.ws_file_self_field_globals.is_empty() {
            let class_index: HashMap<String, usize> = ws_classes.iter()
                .enumerate()
                .map(|(i, c)| (c.name.clone(), i))
                .collect();
            for (source_path, self_fields) in &self.ws_file_self_fields {
                for tsf in self_fields {
                    if let Some(&idx) = class_index.get(&tsf.class_name) {
                        let already_has = ws_classes[idx].fields.iter().any(|(n, _, _)| n == &tsf.field_name);
                        if !already_has {
                            ws_classes[idx].fields.push((tsf.field_name.clone(), tsf.annotation_type.clone(), tsf.visibility));
                            ws_classes[idx].field_ranges.entry(tsf.field_name.clone()).or_insert(tsf.byte_range);
                            ws_classes[idx].field_paths.entry(tsf.field_name.clone()).or_insert_with(|| source_path.clone());
                        }
                    }
                }
            }
            ws_globals.extend(self.ws_file_self_field_globals.values().flatten().cloned());
        }

        let implicit_protected = self.root.as_ref()
            .map(|r| self.configs.implicit_protected_prefix_for(r))
            .unwrap_or(false);
        let mut pg = PreResolvedGlobals::build_on_stubs(
            &self.stub_pre_globals, &ws_globals, &ws_classes, &ws_aliases,
            implicit_protected, &self.ws_file_addon_ns_class, &self.cached_callable_classes,
        );
        pg.merge_events(&ws_events);

        // Build per-addon namespace tables if addon roots are configured.
        let addon_roots = self.configs.addon_roots();
        if !addon_roots.is_empty() {
            // Map each source file to its addon root
            let mut file_addon_roots: HashMap<PathBuf, PathBuf> = HashMap::new();
            for file_path in self.ws_file_globals.keys() {
                if let Some(root) = self.configs.addon_root_for(file_path) {
                    file_addon_roots.insert(file_path.clone(), root.to_path_buf());
                }
            }
            let per_addon_class_names = self.configs.group_addon_ns_classes_by_root(&self.ws_file_addon_ns_class);
            pg.build_per_addon_tables(&file_addon_roots, &per_addon_class_names);
        }

        // Inject project configs so the deferred harvester can build the
        // correct AnalysisConfig for each file it re-analyzes.
        pg.set_project_configs(Arc::clone(&self.configs));
        self.pre_globals = Arc::new(pg);
        self.ws_generation += 1;
        // Intentionally retain `cached_ws_diagnostics` (now stale: its stored
        // generation no longer matches `ws_generation`). The generation mismatch
        // already prevents it from being served as fresh, but keeping the entries
        // lets (1) the next incremental warm reuse them as the prior baseline and
        // (2) `handle_workspace_diagnostic` serve them while a background warm is
        // in flight (avoiding a blocking synchronous recompute / diagnostic
        // flicker). A fresh full warm overwrites them when no prior is reusable.
    }

    /// All workspace `.lua` paths (the set warmed for closed-file diagnostics).
    pub(super) fn ws_lua_paths(&self) -> Vec<PathBuf> {
        self.ws_file_globals
            .keys()
            .filter(|p| p.extension().is_some_and(|e| e == "lua"))
            .cloned()
            .collect()
    }

    /// Snapshot everything a warm needs as owned/`Arc`-shared, `Send + 'static`
    /// data so the work can run on a background thread without borrowing `self`.
    /// Clones (rather than takes) the prior cache so untouched files reuse their
    /// diagnostics during an incremental warm AND the stale cache stays available
    /// to serve `workspace/diagnostic` pulls while the warm runs. `generation`
    /// lets the caller discard stale results.
    pub(super) fn warm_inputs(&self, affected: Option<HashSet<String>>) -> WarmInputs {
        let prior_entries = match (&affected, &self.cached_ws_diagnostics) {
            (Some(_), Some((_, entries))) => Some(entries.clone()),
            _ => None,
        };
        WarmInputs {
            generation: self.ws_generation,
            paths: self.ws_lua_paths(),
            pre_globals: Arc::clone(&self.pre_globals),
            configs: Arc::clone(&self.configs),
            plugin_codes: self.plugin_codes(),
            affected,
            prior: prior_entries,
        }
    }

    /// Run plugins against an analysis result and return diagnostics.
    /// Returns empty vec when no plugins are loaded.
    pub(super) fn run_plugins(&mut self, result: &AnalysisResult, text: &str, uri: &lsp_types::Uri, file_path: &Path) -> Vec<diagnostics::PluginDiag> {
        let allowed = self.configs.plugins_for(file_path);
        if let Some(ref mut engine) = self.plugin_engine {
            let uri_str = uri.to_string();
            let file_name = file_path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            return engine.run_plugins(result, text, &uri_str, &file_name, &allowed)
                .into_iter()
                .map(|d| diagnostics::PluginDiag {
                    code: d.code,
                    message: d.message,
                    severity: d.severity,
                    start: d.start,
                    end: d.end,
                })
                .collect();
        }
        Vec::new()
    }

    pub(super) fn plugin_codes(&self) -> Vec<String> {
        if let Some(ref engine) = self.plugin_engine {
            return engine.plugin_codes().iter().map(|s| s.to_string()).collect();
        }
        Vec::new()
    }

    #[cfg(test)]
    pub(super) fn for_test(root: Option<PathBuf>) -> Self {
        Self {
            root,
            configs: Arc::new(crate::config::ProjectConfigs::default()),
            stub_globals: Vec::new(),
            stub_classes: Vec::new(),
            stub_pre_globals: Arc::new(PreResolvedGlobals::empty()),
            stubs_have_defclass: false,
            stubs_have_built_name: false,
            ws_file_globals: HashMap::new(),
            ws_file_classes: HashMap::new(),
            ws_file_aliases: HashMap::new(),
            ws_file_defclasses: HashMap::new(),
            ws_file_events: HashMap::new(),
            ws_file_self_fields: HashMap::new(),
            ws_file_self_field_globals: HashMap::new(),
            pre_globals: Arc::new(PreResolvedGlobals::empty()),
            cached_all_globals: Vec::new(),
            cached_all_classes: Vec::new(),
            cached_needs_defclass: false,
            cached_needs_built_name: false,
            cached_defclass_func_names: Vec::new(),
            cached_built_name_func_names: Vec::new(),
            ws_file_dynamic_prefixes: HashMap::new(),
            ws_file_addon_ns_class: HashMap::new(),
            ws_file_callable_classes: HashMap::new(),
            cached_callable_classes: HashSet::new(),
            plugin_engine: None,
            ws_generation: 0,
            cached_ws_diagnostics: None,
            warm_in_flight: false,
            pending_lazy_warm: false,
        }
    }
}
