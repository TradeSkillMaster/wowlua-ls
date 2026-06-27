# Configuration Reference

Complete `.wowluarc.json` schema. For practical guidance, see the [Configuration guide](/guide/configuration).

## Editor support

A JSON Schema is provided for autocompletion and validation. The VS Code extension registers it automatically. For other editors, add a `$schema` property to your config file:

```json
{
  "$schema": "https://raw.githubusercontent.com/TradeSkillMaster/wowlua-ls/main/editors/vscode/wowluarc.schema.json",
  "flavors": ["retail"]
}
```

## Schema

```json
{
  "addon_root": false,
  "ignore": ["string"],
  "library": ["string"],
  "framexml": true,
  "flavors": ["retail", "classic", "classic_era"],
  "globals": {
    "read": ["string"],
    "write": ["string"],
    "allow_slash_commands": true,
    "allow_binding_globals": true
  },
  "inference": {
    "backward_param_types": true,
    "correlated_return_overloads": true,
    "implicit_protected_prefix": false
  },
  "hint": {
    "enable": true,
    "parameterNames": true,
    "variableTypes": true,
    "functionReturnTypes": false,
    "forVariableTypes": true,
    "parameterTypes": false,
    "chainedReturnTypes": false
  },
  "codeLens": {
    "enable": true,
    "references": true,
    "implementations": true,
    "overrides": true
  },
  "editor": {
    "autoInsertEnd": true
  },
  "completion": {
    "snippets": true,
    "callSnippets": true
  },
  "plugins": ["string"],
  "diagnostics": {
    "disable": ["string"],
    "enable": ["string"],
    "severity": {
      "diagnostic-code": "warning | info | hint"
    }
  }
}
```

## Fields

### `addon_root`

- **Type:** `boolean`
- **Default:** `false`

Marks this directory as a separate addon root. In multi-addon workspaces, each addon root gets its own isolated addon namespace table (`local _, ns = ...`), so fields defined in one addon aren't visible in another. Lua globals remain shared across all addon roots.

```json
{ "addon_root": true }
```

Not needed for single-addon projects.

### `plugins`

- **Type:** `string[]`
- **Default:** `[]`

Paths to Lua diagnostic plugin scripts. Relative to the config file's directory. Isolated per file: only the plugins declared by a file's nearest config run against it — plugin lists are not inherited from parent configs. See the [Diagnostic Plugins guide](/guide/plugins) for the full API.

```json
{ "plugins": [".wowlua-ls/my-check.lua"] }
```

### `ignore`

- **Type:** `string[]`
- **Default:** `[]`

Path prefixes to exclude from scanning. Relative to the config file's directory. Patterns ending with `/` match directory prefixes. Entries may also use glob wildcards: `*` (any characters within a path component), `?` (single character), and `**` (any number of directory levels).

```json
{ "ignore": ["Libs/", "External/*.lua", "Generated/**/*.lua"] }
```

::: tip Built-in default
`.github/` directories are **always** skipped, no configuration needed. They hold GitHub repository metadata and CI/build tooling (build scripts run by the standalone Lua interpreter, where `io`/`os`/etc. are real standard-library globals), never addon code that ships in-game — so analyzing them as WoW Lua would only produce spurious diagnostics. Anything you add to `ignore` is excluded *in addition* to this default.
:::

### `library`

- **Type:** `string[]`
- **Default:** `[]`

Paths to scan for type information but with all diagnostics suppressed. Useful for third-party libraries where you want classes, globals, and type information available to the rest of your project, but can't fix any diagnostic issues in the library code. Relative entries use the same pattern syntax as `ignore`. Absolute paths — and relative paths that point *outside* the workspace (for example `../shared`) — are also supported for libraries that live next to your addon rather than inside it.

```json
{ "library": ["Libs/", "../shared-libs", "/home/user/shared-libs/"] }
```

Unlike `ignore` (which skips files entirely), `library` files are fully scanned and analyzed — their `@class`, `@alias`, global functions, and other type information are available throughout the workspace. Only diagnostic output is suppressed.

External library directories are automatically added as extra scan directories, so libraries don't need to be inside your workspace. A relative entry that escapes the workspace root (one containing `..`) is resolved against the directory holding the `.wowluarc.json` and scanned as an external directory — prefer this over an absolute path for libraries checked into version control, since it stays portable across machines.

Marking a directory as a library suppresses diagnostics for the **whole subtree**, including any nested `.wowluarc.json` files inside it. A vendored library that ships its own `.wowluarc.json` (for example one pulled in via a symlink) cannot re-enable diagnostics for itself — the parent's `library` declaration wins. See [Hierarchy behavior](#hierarchy-behavior) for how this differs from other settings.

### `framexml`

- **Type:** `boolean`
- **Default:** `true`

Whether FrameXML API globals are available. Set to `false` to treat FrameXML-specific globals as undefined.

### `flavors`

- **Type:** `string[]`
- **Default:** `[]` (flavor filtering disabled)
- **Values:** `"retail"` (alias `"mainline"`), `"classic"`, `"classic_era"`

WoW flavor names the project targets. Enables `wrong-flavor-api` diagnostic when non-empty.

> **Note:** Flavor filtering can also be derived automatically from `.toc` file listings — see the [Flavor Filtering guide](/guide/flavor-filtering). When both sources are present, the effective flavor for each file is the intersection of the project-level `flavors` and the TOC-derived per-file flavor.

### `globals.read`

- **Type:** `string[]`
- **Default:** `[]`

Global names that may be accessed without triggering `undefined-global`. Entries may use glob wildcards: `*` (any characters) and `?` (single character).

```json
{ "globals": { "read": ["LibStub", "MyAddon*Mixin"] } }
```

> **Tip:** `SavedVariables` and `SavedVariablesPerCharacter` declared in `.toc` files are automatically added to both `globals.read` and `globals.write` — no manual configuration needed.

### `globals.write`

- **Type:** `string[]`
- **Default:** `[]`

Global names that may be created/assigned without triggering `create-global`. Entries may use glob wildcards: `*` (any characters) and `?` (single character).

```json
{ "globals": { "write": ["MyAddon*", "SavedVar*"] } }
```

### `globals.allow_slash_commands`

- **Type:** `boolean`
- **Default:** `true`

Automatically treat globals matching `SLASH_*` as allowed write/read globals. WoW slash commands are defined by assigning `SLASH_COMMANDNAME1`, `SLASH_COMMANDNAME2`, etc. to global variables, so these are always intentional. Set to `false` to require explicit listing in `globals.write`.

### `globals.allow_binding_globals`

- **Type:** `boolean`
- **Default:** `true`

Automatically treat globals matching `BINDING_HEADER_*` and `BINDING_NAME_*` as allowed write/read globals. WoW keybinding labels are defined by assigning `BINDING_HEADER_ADDON` and `BINDING_NAME_ACTION` to global variables, and the binding system reads them at runtime. Set to `false` to require explicit listing in `globals.write`.

::: info Automatic dynamic prefix detection
In addition to the settings above, the language server automatically detects dynamic global creation via `_G["PREFIX" .. key] = value` (or `_G[name .. "SUFFIX"]`) patterns in your workspace. When detected, a wildcard glob (e.g. `PREFIX*`) is registered so reads of those globals don't trigger `undefined-global`. This requires no configuration. The prefix/suffix must be at least 3 characters to avoid overly broad matching.
:::

### `inference.backward_param_types`

- **Type:** `boolean`
- **Default:** `true`

Infer parameter types from body usage (arithmetic, concatenation, typed-function argument calls).

### `inference.correlated_return_overloads`

- **Type:** `boolean`
- **Default:** `true`

Infer correlated return patterns (all-set-or-all-nil) for automatic sibling narrowing.

### `inference.implicit_protected_prefix`

- **Type:** `boolean`
- **Default:** `false`

Treat runtime-discovered data fields starting with `_` as implicitly `protected`. Does not affect explicit `@field` declarations or methods.

### `hint.enable`

- **Type:** `boolean`
- **Default:** `true`

Master switch for inlay hints. When `false`, no inlay hints are shown regardless of individual category settings.

### `hint.parameterNames`

- **Type:** `boolean`
- **Default:** `true`

Show parameter name hints at call sites (e.g. `foo(/*name:*/ "hello")`). Suppressed when the argument text already matches the parameter name.

### `hint.variableTypes`

- **Type:** `boolean`
- **Default:** `true`

Show inferred type hints on `local` variable declarations that have no explicit `@type` annotation. Suppressed for `nil`, `any`, and function-valued variables.

### `hint.functionReturnTypes`

- **Type:** `boolean`
- **Default:** `false`

Show inferred return type hints on function definitions that have no `@return` annotation.

### `hint.forVariableTypes`

- **Type:** `boolean`
- **Default:** `true`

Show inferred type hints on `for ... in` loop variables.

### `hint.parameterTypes`

- **Type:** `boolean`
- **Default:** `false`

Show inferred type hints on function parameters that have no `@param` annotation. Suppressed for `self`, `any`, and `nil` parameters.

### `hint.chainedReturnTypes`

- **Type:** `boolean`
- **Default:** `false`

Show intermediate return type hints in method chains. When a method call's return value is immediately used as the receiver of another method/field access, the return type is shown after the closing `)`. Suppressed when the return type is `any`, `nil`, or `?`.

### `codeLens.enable`

- **Type:** `boolean`
- **Default:** `true`

Master switch for code lenses. When `false`, no code lenses are shown regardless of individual category settings.

### `codeLens.references`

- **Type:** `boolean`
- **Default:** `true`

Show "N usages" lenses on function definitions.

### `codeLens.implementations`

- **Type:** `boolean`
- **Default:** `true`

Show "N implementations" lenses on `@class` declarations.

### `codeLens.overrides`

- **Type:** `boolean`
- **Default:** `true`

Show "overrides Parent" lenses on methods that override a parent class method.

### `editor.autoInsertEnd`

- **Type:** `boolean`
- **Default:** `true`

Automatically insert `end` or `until` when Enter is pressed after a block-opening keyword (`if … then`, `while … do`, `for … do`, `function`, `repeat`). The closing keyword is only inserted when the block isn't already closed further down in the file.

```json
{ "editor": { "autoInsertEnd": false } }
```

### `completion.snippets`

- **Type:** `boolean`
- **Default:** `true`

Emit snippet completions (`InsertTextFormat.Snippet`). This covers both function-call parameter auto-fill and annotation-tag bodies (e.g. `@param`). Set to `false` to disable all snippet completions; items then insert plain text only. Requires snippet support from the editor.

### `completion.callSnippets`

- **Type:** `boolean`
- **Default:** `true`

Auto-fill a function's parameters when you complete a function name — e.g. completing `strmatch` inserts `strmatch(${1:s}, ${2:pattern})` with the cursor on the first parameter. Set to `false` to insert just the function name (`strmatch`) and let you type the call yourself. This is independent of [`completion.snippets`](#completion-snippets): annotation-tag snippets still work when only `callSnippets` is disabled.

```json
{ "completion": { "callSnippets": false } }
```

### `diagnostics.disable`

- **Type:** `string[]`
- **Default:** `[]`

Diagnostic codes to suppress.

### `diagnostics.enable`

- **Type:** `string[]`
- **Default:** `[]`

Diagnostic codes to enable. Used for default-off diagnostics or to counteract a `disable` entry in the same config.

### `diagnostics.severity`

- **Type:** `Record<string, "warning" | "info" | "hint">`
- **Default:** `{}`

Override severity for specific diagnostic codes.

## Hierarchy behavior

When `.wowluarc.json` files are nested, settings combine according to one of these policies:

- **Isolated** — the single **nearest** ancestor config fully determines the setting. Keys it does not set fall back to their **default**, *not* to a parent config's value. This applies to everything that affects diagnostics, so that running a check from a subdirectory produces the same results as running it from the project root (configs above the scan root are never consulted).
- **Inherited** — the deepest config that sets the key wins, falling back to ancestor configs and then the default. This applies to editor-experience settings that do not affect diagnostics.
- **Inherited downward** — the `library` setting alone uses this: once any ancestor config marks a subtree as a library, every file beneath it has diagnostics suppressed, and a nested config inside that subtree cannot opt back in. (Whether a subtree *is* a library still affects only diagnostics, so this is the one diagnostics setting that is not isolated — its entire purpose is to silence a whole vendored subtree, including any config files that subtree carries.)

| Setting | Policy |
|---|---|
| `diagnostics.disable` | **Isolated** — nearest config's `disable` only |
| `diagnostics.enable` | **Isolated** — applied after the nearest config's `disable` |
| `diagnostics.severity` | **Isolated** — nearest config's severity map only |
| `globals.read` | **Isolated** — nearest config only (includes that directory's `.toc` `SavedVariables`) |
| `globals.write` | **Isolated** — nearest config only |
| `globals.allow_slash_commands` | **Isolated** |
| `globals.allow_binding_globals` | **Isolated** |
| `framexml` | **Isolated** |
| `flavors` | **Isolated** (then intersected with any TOC-derived per-file mask) |
| `inference.*` | **Isolated** |
| `ignore` | **Isolated** — nearest config's patterns, relative to its directory |
| `library` | **Inherited downward** — any ancestor config that marks a subtree as a library suppresses diagnostics for that whole subtree, and a nested config inside it cannot un-mark itself; absolute library directories are scanned workspace-wide |
| `plugins` | **Isolated** — only the nearest config's plugins run against a file |
| `hint.*` | Inherited |
| `codeLens.*` | Inherited |
| `editor.*` | Inherited |
| `completion.*` | Inherited |
| `addon_root` | Nearest (deepest) `addon_root: true` wins (structural) |

::: warning Isolated settings do not inherit
If a nested directory has its own `.wowluarc.json`, it only inherits the **inherited** settings above. Any **isolated** setting it does not restate reverts to its default — it does *not* pick up the parent's value. For example, a subdirectory config that only sets `diagnostics.enable` will lose a parent's `flavors` and `framexml` settings unless it repeats them. This also applies to auto-discovered TOC `SavedVariables` globals — they are merged into the config entry for the directory containing the `.toc` file, so a child config in a subdirectory will not see them.

The one exception is [`library`](#library), which is inherited downward.
:::
