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
    "allow_slash_commands": true
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

Paths to Lua diagnostic plugin scripts. Relative to the config file's directory. The nearest (deepest) config with a `plugins` key wins — plugin lists are not merged across ancestors. See the [Diagnostic Plugins guide](/guide/plugins) for the full API.

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

### `library`

- **Type:** `string[]`
- **Default:** `[]`

Paths to scan for type information but with all diagnostics suppressed. Useful for third-party libraries where you want classes, globals, and type information available to the rest of your project, but can't fix any diagnostic issues in the library code. Relative entries use the same pattern syntax as `ignore`. Absolute paths are also supported for libraries outside the workspace.

```json
{ "library": ["Libs/", "/home/user/shared-libs/"] }
```

Unlike `ignore` (which skips files entirely), `library` files are fully scanned and analyzed — their `@class`, `@alias`, global functions, and other type information are available throughout the workspace. Only diagnostic output is suppressed.

Absolute paths are automatically added as extra scan directories, so external libraries don't need to be inside your workspace.

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

### `diagnostics.disable`

- **Type:** `string[]`
- **Default:** `[]`

Diagnostic codes to suppress.

### `diagnostics.enable`

- **Type:** `string[]`
- **Default:** `[]`

Diagnostic codes to enable. Used for default-off diagnostics or to override a parent's `disable`.

### `diagnostics.severity`

- **Type:** `Record<string, "warning" | "info" | "hint">`
- **Default:** `{}`

Override severity for specific diagnostic codes.

## Hierarchy behavior

| Setting | Merge behavior |
|---|---|
| `addon_root` | Nearest (deepest) config wins |
| `plugins` | Nearest (deepest) config wins |
| `ignore` | Relative to containing directory |
| `library` | Relative to containing directory |
| `framexml` | Nearest (deepest) config wins |
| `flavors` | Nearest (deepest) config wins |
| `globals.read` | Unioned across ancestors |
| `globals.write` | Unioned across ancestors |
| `globals.allow_slash_commands` | Nearest (deepest) config wins |
| `inference.*` | Nearest (deepest) config wins |
| `hint.*` | Nearest (deepest) config wins |
| `codeLens.*` | Nearest (deepest) config wins |
| `editor.*` | Nearest (deepest) config wins |
| `diagnostics.disable` | Unioned across ancestors |
| `diagnostics.enable` | Applied after `disable` at each level |
| `diagnostics.severity` | Deeper configs take precedence |
