//! Lua UserData bridge types.
//!
//! Each type is a thin wrapper around query results, holding an `Arc<AnalysisSnapshot>`
//! for deferred lookups. Plugins see these as Lua userdata with named methods/fields.

use std::sync::{Arc, Mutex};

use lsp_types::DiagnosticSeverity;
use mlua::prelude::*;

use crate::lsp::uri::abs_path_to_uri;
use crate::types::*;
use super::PluginDiagnostic;
use super::query::{self, AnalysisSnapshot, LiteralValue};

// ── FileContext ─────────────────────────────────────────────────────────────────

/// The main object passed to a plugin's `run(ctx)` function.
pub(super) struct LuaFileContext {
    snap: Arc<AnalysisSnapshot>,
    #[allow(dead_code)] // reserved for future text-based queries
    source: String,
    file_uri: String,
    file_name: String,
    plugin_code: String,
    diags: Arc<Mutex<Vec<PluginDiagnostic>>>,
}

impl LuaFileContext {
    pub(super) fn new(
        snap: Arc<AnalysisSnapshot>,
        source: String,
        file_uri: String,
        file_name: String,
        plugin_code: String,
        diags: Arc<Mutex<Vec<PluginDiagnostic>>>,
    ) -> Self {
        LuaFileContext { snap, source, file_uri, file_name, plugin_code, diags }
    }

    fn emit(&self, range: &LuaTable, message: String, severity: DiagnosticSeverity) -> LuaResult<()> {
        let start: usize = range.get("start")?;
        let end: usize = range.get("end")?;
        self.diags.lock().unwrap().push(PluginDiagnostic {
            code: self.plugin_code.clone(),
            message,
            severity,
            start,
            end,
        });
        Ok(())
    }
}

impl LuaUserData for LuaFileContext {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("uri", |_, this| Ok(this.file_uri.clone()));
        fields.add_field_method_get("file_name", |_, this| Ok(this.file_name.clone()));
    }

    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("find_locals", |lua, this, opts: LuaValue| {
            let (name_filter, init_filter) = parse_find_opts(&opts)?;
            let vars = query::find_locals(
                &this.snap,
                name_filter.as_deref(),
                init_filter.as_deref(),
            );
            let result = lua.create_table()?;
            for (i, v) in vars.into_iter().enumerate() {
                let var = LuaVariable {
                    snap: this.snap.clone(),
                    sym_idx: v.sym_idx,
                    name: v.name,
                    def_start: v.def_start,
                    def_end: v.def_end,
                    init_expr: v.init_expr,
                };
                result.set(i + 1, lua.create_userdata(var)?)?;
            }
            Ok(result)
        });

        methods.add_method("find_event_declarations", |lua, this, type_name: Option<String>| {
            let events = query::find_event_declarations(
                &this.snap,
                type_name.as_deref(),
            );
            let result = lua.create_table()?;
            for (i, ev) in events.into_iter().enumerate() {
                let entry = lua.create_table()?;
                entry.set("type_name", ev.type_name)?;
                entry.set("event_name", ev.event_name)?;
                match ev.range {
                    Some((start, end)) => entry.set("range", make_range_table(lua, start, end)?)?,
                    None => entry.set("range", LuaNil)?,
                }
                match ev.source_path {
                    Some(p) => {
                        let uri_str = abs_path_to_uri(&p)
                            .map(|u| u.to_string());
                        match uri_str {
                            Some(s) => entry.set("source_uri", s)?,
                            None => entry.set("source_uri", LuaNil)?,
                        }
                    }
                    None => entry.set("source_uri", LuaNil)?,
                }
                let params = lua.create_table()?;
                for (j, p) in ev.params.into_iter().enumerate() {
                    let param = lua.create_table()?;
                    param.set("name", p.name)?;
                    param.set("type_name", p.type_name)?;
                    param.set("nilable", p.nilable)?;
                    match p.description {
                        Some(d) => param.set("description", d)?,
                        None => param.set("description", LuaNil)?,
                    }
                    params.set(j + 1, param)?;
                }
                entry.set("params", params)?;
                result.set(i + 1, entry)?;
            }
            Ok(result)
        });

        methods.add_method("warn", |_, this, (range, msg): (LuaTable, String)| {
            this.emit(&range, msg, DiagnosticSeverity::WARNING)
        });

        methods.add_method("hint", |_, this, (range, msg): (LuaTable, String)| {
            this.emit(&range, msg, DiagnosticSeverity::HINT)
        });

        methods.add_method("error", |_, this, (range, msg): (LuaTable, String)| {
            this.emit(&range, msg, DiagnosticSeverity::ERROR)
        });

        methods.add_method("info", |_, this, (range, msg): (LuaTable, String)| {
            this.emit(&range, msg, DiagnosticSeverity::INFORMATION)
        });
    }
}

fn parse_find_opts(opts: &LuaValue) -> LuaResult<(Option<String>, Option<String>)> {
    match opts {
        LuaValue::Table(t) => {
            let name: Option<String> = t.get("name")?;
            let init: Option<String> = t.get("init")?;
            Ok((name, init))
        }
        LuaValue::Nil => Ok((None, None)),
        _ => Err(LuaError::runtime("find_locals expects a table or nil")),
    }
}

// ── Variable ────────────────────────────────────────────────────────────────────

struct LuaVariable {
    snap: Arc<AnalysisSnapshot>,
    sym_idx: SymbolIndex,
    name: String,
    def_start: u32,
    def_end: u32,
    init_expr: Option<ExprId>,
}

impl LuaUserData for LuaVariable {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("name", |_, this| Ok(this.name.clone()));
        fields.add_field_method_get("range", |lua, this| make_range(lua, this.def_start, this.def_end));
        fields.add_field_method_get("init", |lua, this| {
            match this.init_expr {
                Some(expr_id) => Ok(LuaValue::UserData(lua.create_userdata(LuaInitializer {
                    snap: this.snap.clone(),
                    expr_id,
                })?)),
                None => Ok(LuaNil),
            }
        });
    }

    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("field_reads", |lua, this, ()| {
            let reads = query::field_reads(&this.snap, this.sym_idx);
            to_field_access_table(lua, reads)
        });

        methods.add_method("field_writes", |lua, this, ()| {
            let writes = query::field_writes(&this.snap, this.sym_idx);
            to_field_access_table(lua, writes)
        });

        methods.add_method("method_calls", |lua, this, ()| {
            let calls = query::method_calls(&this.snap, this.sym_idx);
            let result = lua.create_table()?;
            for (i, c) in calls.into_iter().enumerate() {
                result.set(i + 1, lua.create_userdata(LuaMethodCall {
                    snap: this.snap.clone(),
                    method_name: c.method_name,
                    range_start: c.range_start,
                    range_end: c.range_end,
                    arg_exprs: c.arg_exprs,
                    arg_ranges: c.arg_ranges,
                })?)?;
            }
            Ok(result)
        });

        methods.add_method("method_defs", |lua, this, ()| {
            let defs = query::method_defs(&this.snap, this.sym_idx);
            let result = lua.create_table()?;
            for (i, d) in defs.into_iter().enumerate() {
                result.set(i + 1, lua.create_userdata(LuaMethodDef {
                    snap: this.snap.clone(),
                    method_name: d.method_name,
                    range_start: d.range_start,
                    range_end: d.range_end,
                    func_idx: d.func_idx,
                })?)?;
            }
            Ok(result)
        });
    }
}

// ── Initializer ─────────────────────────────────────────────────────────────────

struct LuaInitializer {
    snap: Arc<AnalysisSnapshot>,
    expr_id: ExprId,
}

impl LuaUserData for LuaInitializer {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("kind", |_, this| {
            Ok(query::init_kind(&this.snap, this.expr_id))
        });
    }

    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("fields", |lua, this, ()| {
            let fields = query::table_fields(&this.snap, this.expr_id);
            let result = lua.create_table()?;
            for (i, f) in fields.into_iter().enumerate() {
                let entry = lua.create_table()?;
                entry.set("name", f.name)?;
                entry.set("range", make_range_table(lua, f.range_start, f.range_end)?)?;
                entry.set("value_kind", f.value_kind.as_str())?;
                result.set(i + 1, entry)?;
            }
            Ok(result)
        });

        methods.add_method("receiver", |_, this, ()| {
            Ok(query::call_init_info(&this.snap, this.expr_id)
                .and_then(|c| c.receiver))
        });

        methods.add_method("method", |_, this, ()| {
            Ok(query::call_init_info(&this.snap, this.expr_id)
                .and_then(|c| c.method))
        });

        methods.add_method("args", |lua, this, ()| {
            let Some(call_info) = query::call_init_info(&this.snap, this.expr_id) else {
                return lua.create_table();
            };
            let args = query::args_info(&this.snap, &call_info.arg_exprs, &call_info.arg_ranges);
            to_arg_table(lua, &this.snap, args)
        });
    }
}

// ── FieldAccess ─────────────────────────────────────────────────────────────────

struct LuaFieldAccess {
    field_name: String,
    range_start: u32,
    range_end: u32,
}

impl LuaUserData for LuaFieldAccess {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("field_name", |_, this| Ok(this.field_name.clone()));
        fields.add_field_method_get("range", |lua, this| make_range(lua, this.range_start, this.range_end));
    }
}

// ── MethodCall ──────────────────────────────────────────────────────────────────

struct LuaMethodCall {
    snap: Arc<AnalysisSnapshot>,
    method_name: String,
    range_start: u32,
    range_end: u32,
    arg_exprs: Vec<ExprId>,
    arg_ranges: Vec<(u32, u32)>,
}

impl LuaUserData for LuaMethodCall {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("method_name", |_, this| Ok(this.method_name.clone()));
        fields.add_field_method_get("range", |lua, this| make_range(lua, this.range_start, this.range_end));
    }

    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("args", |lua, this, ()| {
            let args = query::args_info(&this.snap, &this.arg_exprs, &this.arg_ranges);
            to_arg_table(lua, &this.snap, args)
        });
    }
}

// ── MethodDef ───────────────────────────────────────────────────────────────────

struct LuaMethodDef {
    snap: Arc<AnalysisSnapshot>,
    method_name: String,
    range_start: u32,
    range_end: u32,
    func_idx: FunctionIndex,
}

impl LuaUserData for LuaMethodDef {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("method_name", |_, this| Ok(this.method_name.clone()));
        fields.add_field_method_get("range", |lua, this| make_range(lua, this.range_start, this.range_end));
    }

    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("params", |lua, this, ()| {
            let params = query::function_params(&this.snap, this.func_idx);
            let result = lua.create_table()?;
            for (i, p) in params.into_iter().enumerate() {
                result.set(i + 1, lua.create_userdata(LuaParam {
                    snap: this.snap.clone(),
                    name: p.name,
                    sym_idx: p.sym_idx,
                    param_index: p.param_index,
                })?)?;
            }
            Ok(result)
        });
    }
}

// ── Param ───────────────────────────────────────────────────────────────────────

struct LuaParam {
    snap: Arc<AnalysisSnapshot>,
    name: String,
    sym_idx: SymbolIndex,
    param_index: usize,
}

impl LuaUserData for LuaParam {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("name", |_, this| Ok(this.name.clone()));
        fields.add_field_method_get("index", |_, this| Ok(this.param_index + 1)); // 1-based for Lua
    }

    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("comparisons", |lua, this, ()| {
            let comps = query::symbol_comparisons(&this.snap, this.sym_idx);
            let result = lua.create_table()?;
            for (i, c) in comps.into_iter().enumerate() {
                let entry = lua.create_table()?;
                entry.set("range", make_range_table(lua, c.range_start, c.range_end)?)?;
                match c.literal {
                    Some(LiteralValue::String(s)) => entry.set("literal", s)?,
                    Some(LiteralValue::Number(n)) => entry.set("literal", n)?,
                    Some(LiteralValue::Boolean(b)) => entry.set("literal", b)?,
                    Some(LiteralValue::Nil) | None => entry.set("literal", LuaNil)?,
                }
                result.set(i + 1, entry)?;
            }
            Ok(result)
        });
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────────

fn make_range(lua: &Lua, start: u32, end: u32) -> LuaResult<LuaTable> {
    make_range_table(lua, start, end)
}

fn make_range_table(lua: &Lua, start: u32, end: u32) -> LuaResult<LuaTable> {
    let t = lua.create_table()?;
    t.set("start", start as usize)?;
    t.set("end", end as usize)?;
    Ok(t)
}

fn to_field_access_table(lua: &Lua, accesses: Vec<query::FieldAccessInfo>) -> LuaResult<LuaTable> {
    let result = lua.create_table()?;
    for (i, a) in accesses.into_iter().enumerate() {
        result.set(i + 1, lua.create_userdata(LuaFieldAccess {
            field_name: a.field_name,
            range_start: a.range_start,
            range_end: a.range_end,
        })?)?;
    }
    Ok(result)
}

fn to_arg_table(lua: &Lua, _snap: &Arc<AnalysisSnapshot>, args: Vec<query::ArgInfo>) -> LuaResult<LuaTable> {
    let result = lua.create_table()?;
    for (i, a) in args.into_iter().enumerate() {
        let entry = lua.create_table()?;
        entry.set("range", make_range_table(lua, a.range_start, a.range_end)?)?;
        entry.set("kind", a.kind)?;
        match a.literal {
            Some(LiteralValue::String(s)) => entry.set("literal", s)?,
            Some(LiteralValue::Number(n)) => entry.set("literal", n)?,
            Some(LiteralValue::Boolean(b)) => entry.set("literal", b)?,
            Some(LiteralValue::Nil) => entry.set("literal", LuaNil)?,
            None => entry.set("literal", LuaNil)?,
        }
        result.set(i + 1, entry)?;
    }
    Ok(result)
}
