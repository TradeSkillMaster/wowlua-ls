mod bridge;
mod query;
mod sandbox;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lsp_types::DiagnosticSeverity;
use mlua::prelude::*;

use crate::analysis::AnalysisResult;

/// A diagnostic emitted by a Lua plugin (owned code string, unlike built-in WowDiagnostic).
#[derive(Debug, Clone)]
pub struct PluginDiagnostic {
    pub code: String,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub start: usize,
    pub end: usize,
}

/// A loaded plugin: validated return table with a stored `run` function.
struct LoadedPlugin {
    code: String,
    run_fn: LuaRegistryKey,
    source_path: PathBuf,
    /// Consecutive failure count. Plugin is disabled after MAX_FAILURES.
    failures: usize,
}

const MAX_FAILURES: usize = 5;

/// The plugin engine: holds a Lua VM and loaded plugins for a workspace.
pub struct PluginEngine {
    lua: Lua,
    plugins: Vec<LoadedPlugin>,
}

impl PluginEngine {
    /// Create a new plugin engine, loading plugin files from the given paths.
    /// Paths that fail to load are logged and skipped.
    pub fn new(plugin_paths: &[PathBuf]) -> Self {
        let lua = sandbox::create_sandbox();
        let mut plugins = Vec::new();

        for path in plugin_paths {
            match Self::load_plugin(&lua, path) {
                Ok(plugin) => {
                    log::info!("loaded plugin '{}' from {}", plugin.code, path.display());
                    plugins.push(plugin);
                }
                Err(e) => {
                    log::warn!("failed to load plugin {}: {}", path.display(), e);
                }
            }
        }

        PluginEngine { lua, plugins }
    }

    /// Reload a plugin file (e.g. after file change notification).
    pub fn reload(&mut self, path: &Path) {
        // Find and replace the existing plugin, or append if new.
        match Self::load_plugin(&self.lua, path) {
            Ok(plugin) => {
                let code = plugin.code.clone();
                if let Some(existing) = self.plugins.iter_mut().find(|p| p.source_path == path) {
                    *existing = plugin;
                    log::info!("reloaded plugin '{}' from {}", code, path.display());
                } else {
                    log::info!("loaded new plugin '{}' from {}", code, path.display());
                    self.plugins.push(plugin);
                }
            }
            Err(e) => {
                log::warn!("failed to reload plugin {}: {}", path.display(), e);
            }
        }
    }

    fn load_plugin(lua: &Lua, path: &Path) -> Result<LoadedPlugin, String> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read file: {e}"))?;

        let chunk = lua.load(&source).set_name(path.to_string_lossy());
        let table: LuaTable = chunk.eval()
            .map_err(|e| format!("plugin must return a table: {e}"))?;

        let code: String = table.get::<String>("code")
            .map_err(|_| "plugin must have a 'code' field (string)".to_string())?;
        if code.is_empty() {
            return Err("'code' must not be empty".into());
        }

        let run_fn: LuaFunction = table.get::<LuaFunction>("run")
            .map_err(|_| "plugin must have a 'run' function".to_string())?;
        let run_key = lua.create_registry_value(run_fn)
            .map_err(|e| format!("failed to store run function: {e}"))?;

        Ok(LoadedPlugin {
            code,
            run_fn: run_key,
            source_path: path.to_path_buf(),
            failures: 0,
        })
    }

    /// Run all loaded plugins against a single file's analysis result.
    /// Returns plugin diagnostics. Never panics — plugin errors are logged and skipped.
    pub fn run_plugins(
        &mut self,
        analysis: &AnalysisResult,
        source: &str,
        file_uri: &str,
        file_name: &str,
        allowed: &[PathBuf],
    ) -> Vec<PluginDiagnostic> {
        let analysis = Arc::new(query::AnalysisSnapshot::from_result(analysis));
        let mut all_diags = Vec::new();

        for plugin in &mut self.plugins {
            if plugin.failures >= MAX_FAILURES {
                continue; // disabled after too many consecutive failures
            }
            // Plugins are isolated per-file: only run those declared by the
            // file's nearest config (matched by source path).
            if !allowed.contains(&plugin.source_path) {
                continue;
            }

            match Self::run_single_plugin(
                &self.lua, plugin, &analysis, source, file_uri, file_name,
            ) {
                Ok(diags) => {
                    plugin.failures = 0; // reset on success
                    all_diags.extend(diags);
                }
                Err(e) => {
                    plugin.failures += 1;
                    if plugin.failures >= MAX_FAILURES {
                        log::error!(
                            "plugin '{}' disabled after {} consecutive failures: {}",
                            plugin.code, MAX_FAILURES, e
                        );
                    } else {
                        log::warn!(
                            "plugin '{}' failed on {}: {}",
                            plugin.code, file_name, e
                        );
                    }
                }
            }
        }

        all_diags
    }

    fn run_single_plugin(
        lua: &Lua,
        plugin: &LoadedPlugin,
        analysis: &Arc<query::AnalysisSnapshot>,
        source: &str,
        file_uri: &str,
        file_name: &str,
    ) -> Result<Vec<PluginDiagnostic>, String> {
        sandbox::reset_instruction_limit(lua);

        let run_fn: LuaFunction = lua.registry_value(&plugin.run_fn)
            .map_err(|e| format!("registry lookup failed: {e}"))?;

        let diags = Arc::new(std::sync::Mutex::new(Vec::new()));

        let ctx = bridge::LuaFileContext::new(
            analysis.clone(),
            source.to_string(),
            file_uri.to_string(),
            file_name.to_string(),
            plugin.code.clone(),
            diags.clone(),
        );

        let ud = lua.create_userdata(ctx)
            .map_err(|e| format!("failed to create FileContext: {e}"))?;

        run_fn.call::<()>(ud)
            .map_err(|e| format!("{e}"))?;

        let result = match Arc::try_unwrap(diags) {
            Ok(mutex) => mutex.into_inner().unwrap(),
            Err(arc) => arc.lock().unwrap().clone(),
        };
        Ok(result)
    }

    /// Returns the set of diagnostic codes declared by loaded plugins.
    pub fn plugin_codes(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.code.as_str()).collect()
    }
}
