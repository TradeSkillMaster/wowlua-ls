# Configuration

wowlua-ls is configured via `.wowluarc.json` files placed in your project directories. No configuration is required to get started — the defaults work for most addons.

## File placement

Config files are hierarchical, like `.gitignore`. Place one at your workspace root for project-wide settings:

```
MyAddon/
├── .wowluarc.json         ← project-wide config
├── Core/
│   └── Core.lua
├── Libs/
│   ├── .wowluarc.json     ← directory override (e.g., ignore everything)
│   └── LibStub/
└── Tests/
    ├── .wowluarc.json     ← enable strict diagnostics for tests
    └── TestSuite.lua
```

How settings combine depends on whether they affect diagnostics:

- **Anything that affects diagnostics is *isolated*** — a file's effective value comes solely from its single nearest ancestor config. Keys that config doesn't set fall back to their **default**, *not* to a parent's value. This covers `ignore`, `diagnostics.*`, `globals.*`, `framexml`, `flavors`, `inference.*`, and `plugins`. The point is that a check run from a subdirectory produces the same results as one run from the project root.
- **Editor-experience settings are *inherited*** — `hint.*`, `codeLens.*`, `editor.*`, and `completion.*` use the deepest config that sets the key, falling back to ancestors and then the default.
- **`library` is *inherited downward*** — once any ancestor marks a subtree as a library, every file beneath it has diagnostics suppressed, and a nested config inside that subtree cannot opt back in.

`ignore` and `library` patterns are always relative to the config file's directory. For the full per-setting policy table, see the [Configuration reference](/reference/configuration#hierarchy-behavior).

::: warning Isolated settings do not inherit
If a subdirectory has its own `.wowluarc.json`, any isolated setting it does **not** restate reverts to the default — it does *not* pick up the parent's value. A child config that only sets `diagnostics.enable` will lose a parent's `flavors` and `framexml` unless it repeats them.
:::

## Full reference

```json
{
  "addon_root": false,
  "ignore": ["Libs/", "External/"],
  "library": ["Vendor/", "../shared-libs"],
  "framexml": false,
  "flavors": ["retail", "classic"],
  "globals": {
    "read": ["LibStub", "AceDB"],
    "write": ["MyAddonDB"]
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
  "inference": {
    "backward_param_types": true,
    "correlated_return_overloads": true,
    "implicit_protected_prefix": false
  },
  "plugins": [".wowlua-ls/my-check.lua"],
  "diagnostics": {
    "disable": ["unused-local", "inject-field"],
    "enable": ["need-check-nil"],
    "severity": {
      "unused-local": "warning",
      "unused-function": "warning"
    }
  }
}
```

### `plugins`

Array of paths to Lua diagnostic plugin scripts. Paths are relative to the config file's directory. See the [Diagnostic Plugins](/guide/plugins) guide for the full API.

```json
{ "plugins": [".wowlua-ls/my-check.lua"] }
```

### `ignore`

Array of path prefixes to exclude from scanning. Relative to the config file's directory. Patterns ending with `/` match directory prefixes.

```json
{ "ignore": ["Libs/", "External/", "scratch.lua"] }
```

Use this for vendored libraries, generated code, or anything you don't want diagnostics on.

Files starting with a shebang (`#!/usr/bin/lua`) are always skipped automatically — no `ignore` entry needed.

### `library`

Array of paths to scan for type information but with all diagnostics **suppressed**. Use this for third-party libraries you can't fix but whose types you still want available throughout your project.

```json
{ "library": ["Vendor/", "../shared-libs", "/opt/wow/shared"] }
```

This differs from `ignore` in an important way:

- **`ignore`** skips the files entirely — no scanning, no types, no diagnostics.
- **`library`** fully scans and analyzes the files — their `@class`, `@alias`, global functions, and other type information flow to the rest of your workspace — but no diagnostics are reported on the library files themselves.

Relative entries use the same pattern syntax as `ignore`. Absolute paths are supported, as are relative paths that point *outside* the workspace (e.g. `../shared`) — these are resolved against the config file's directory and scanned as extra external directories, so a library doesn't have to live inside your workspace. Prefer a relative `../` path over an absolute one for libraries checked into version control, since it stays portable across machines.

Marking a directory as a library suppresses diagnostics for the **whole subtree**. Unlike the isolated diagnostics settings, this is inherited downward: a vendored library that ships its own `.wowluarc.json` cannot re-enable diagnostics for itself — the ancestor's `library` declaration wins. See the [reference](/reference/configuration#library) for the full hierarchy semantics.

### `framexml`

Whether FrameXML API globals are available. Default: `true`.

```json
{ "framexml": false }
```

Set to `false` to treat FrameXML-specific globals (e.g. `SetUIPanelAttribute`) as undefined. Useful for library code that shouldn't depend on FrameXML.

### `flavors`

Array of WoW flavor names the project targets. Enables the `wrong-flavor-api` diagnostic. See [Flavor Filtering](/guide/flavor-filtering).

```json
{ "flavors": ["retail", "classic"] }
```

When omitted or empty, flavor filtering is disabled.

### `globals`

Declare external globals that shouldn't trigger diagnostics:

```json
{
  "globals": {
    "read": ["LibStub", "AceDB", "AceLocale"],
    "write": ["MyAddonDB"]
  }
}
```

- **`read`** — global names that may be accessed without `undefined-global`
- **`write`** — global names that may be created/assigned without `create-global`

Use `read` for globals provided by other addons or libraries not in stubs. Use `write` for globals your addon intentionally exports.

Globals matching `SLASH_*` (slash command definitions like `SLASH_MYADDON1 = "/myaddon"`) are automatically allowed by default. Set `"allow_slash_commands": false` inside `globals` to disable this.

Globals matching `BINDING_HEADER_*` and `BINDING_NAME_*` (keybinding label definitions like `BINDING_HEADER_MYADDON = "MyAddon"`) are also automatically allowed. Set `"allow_binding_globals": false` inside `globals` to disable this.

Dynamic global prefixes are also detected automatically. When a file writes globals through a pattern like `_G["PREFIX" .. key] = value` (or `_G[name .. "SUFFIX"]`), the language server registers a wildcard entry (`PREFIX*` or `*SUFFIX`) so that reads of those globals in other files don't trigger `undefined-global`. This is common in localization code that exports translation strings (e.g. `_G["MYADDON_L_" .. key] = value`).

`SavedVariables` and `SavedVariablesPerCharacter` from `.toc` files are automatically added to both lists — you don't need to configure them manually.

### `inference`

Control the LS's type inference behavior:

| Setting | Default | Description |
|---|---|---|
| `backward_param_types` | `true` | Infer parameter types from body usage (arithmetic, concatenation, typed-function args) |
| `correlated_return_overloads` | `true` | Infer correlated return patterns for sibling narrowing |
| `implicit_protected_prefix` | `false` | Treat `_`-prefixed data fields as implicitly `protected` |

```json
{
  "inference": {
    "backward_param_types": false
  }
}
```

Set `backward_param_types` to `false` in strict-typing projects where you want unannotated parameters to stay visible as unknown types.

Set `correlated_return_overloads` to `false` if the inferred narrowing suppresses `need-check-nil` warnings you actually want.

Set `implicit_protected_prefix` to `true` if your project follows the `_`-prefix convention for internal fields and you want `access-protected` diagnostics on external access. See [Implicit protected for `_` prefixes](/guide/classes#implicit-protected-for-prefixes).

### `hint`

Configure inlay hints — inline annotations the editor shows next to your code. Hints are **enabled by default**.

| Setting | Default | Description |
|---|---|---|
| `enable` | `true` | Master switch — set to `false` to disable all hints |
| `parameterNames` | `true` | Parameter names at call sites |
| `variableTypes` | `true` | Inferred types on `local` declarations |
| `functionReturnTypes` | `false` | Inferred return types on function definitions |
| `forVariableTypes` | `true` | Inferred types on `for ... in` loop variables |
| `parameterTypes` | `false` | Inferred types on function parameters |
| `chainedReturnTypes` | `false` | Intermediate return types in method chains |

By default you get parameter names, variable types, and for-loop types. Return type, parameter type, and chained return type hints are off by default because they can be noisy on large codebases.

To enable everything:

```json
{
  "hint": {
    "functionReturnTypes": true,
    "parameterTypes": true,
    "chainedReturnTypes": true
  }
}
```

To disable hints entirely:

```json
{
  "hint": {
    "enable": false
  }
}
```

### `diagnostics`

Fine-grained control over which diagnostics fire and at what severity:

```json
{
  "diagnostics": {
    "disable": ["unused-local", "inject-field"],
    "enable": ["need-check-nil", "implicit-nil-return"],
    "severity": {
      "unused-local": "warning",
      "unused-function": "warning"
    }
  }
}
```

- **`disable`** — suppress these diagnostic codes
- **`enable`** — opt into diagnostics that are off by default, or counteract a `disable` entry in the same config
- **`severity`** — override severity: `"warning"`, `"info"`, `"hint"`

#### Diagnostics disabled by default

These diagnostics are off unless you explicitly enable them:

`need-check-nil`, `nil-index`, `implicit-nil-return`, `invalid-op`, `unused-vararg`, `unused-function`, `incomplete-signature-doc`, `missing-param-annotation`, `missing-return-annotation`, `redundant-or`, `redundant-and`, `redundant-condition`, `unknown-param-type`, `unknown-return-type`, `unknown-local-type`, `unknown-field-type`

### `addon_root`

Marks this directory as a separate addon root for namespace isolation. Default: `false`.

When your workspace contains multiple addons side by side, the addon namespace (`local _, ns = ...`) is shared across all files by default. Setting `addon_root: true` in each addon's `.wowluarc.json` isolates their namespace tables so fields defined in one addon aren't visible in another.

```
workspace/
├── AddonA/
│   ├── .wowluarc.json     ← { "addon_root": true }
│   ├── Core.lua
│   └── Libs/
│       └── LibStub/        ← no addon_root — part of AddonA
└── AddonB/
    ├── .wowluarc.json     ← { "addon_root": true }
    └── Main.lua
```

```json
{ "addon_root": true }
```

Lua globals remain shared across addon roots — only the addon namespace table is isolated. If addon roots are nested, the deepest one wins.

Not needed for single-addon projects (the default behavior is unchanged).

### Recommended starting config

For a typical WoW addon:

```json
{
  "ignore": ["Libs/"],
  "diagnostics": {
    "enable": ["need-check-nil"]
  }
}
```

For a multi-flavor addon:

```json
{
  "ignore": ["Libs/"],
  "flavors": ["retail", "classic"],
  "diagnostics": {
    "enable": ["need-check-nil"]
  }
}
```

For strict typing:

```json
{
  "ignore": ["Libs/"],
  "diagnostics": {
    "enable": [
      "need-check-nil",
      "unknown-param-type",
      "unknown-return-type",
      "unknown-local-type"
    ]
  }
}
```

## Inline suppression

Any diagnostic can be suppressed on a per-line basis with `@diagnostic`:

```lua
---@diagnostic disable-next-line: unused-local
local unused = computeSomething()
```

Or for a block:

```lua
---@diagnostic disable: undefined-global
MY_GLOBAL = true
OTHER_GLOBAL = false
---@diagnostic enable: undefined-global
```

## Auto-reload

Config files are automatically reloaded when saved. No need to restart the language server.
