# Configuration Reference

Complete `.wowluarc.json` schema. For practical guidance, see the [Configuration guide](/guide/configuration).

## Schema

```json
{
  "ignore": ["string"],
  "framexml": true,
  "flavors": ["retail", "classic", "classic_era"],
  "globals": {
    "read": ["string"],
    "write": ["string"]
  },
  "inference": {
    "backward_param_types": true,
    "correlated_return_overloads": true,
    "implicit_protected_prefix": false
  },
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

### `ignore`

- **Type:** `string[]`
- **Default:** `[]`

Path prefixes to exclude from scanning. Relative to the config file's directory. Patterns ending with `/` match directory prefixes.

### `framexml`

- **Type:** `boolean`
- **Default:** `true`

Whether FrameXML API globals are available. Set to `false` to treat FrameXML-specific globals as undefined.

### `flavors`

- **Type:** `string[]`
- **Default:** `[]` (flavor filtering disabled)
- **Values:** `"retail"` (alias `"mainline"`), `"classic"`, `"classic_era"`

WoW flavor names the project targets. Enables `wrong-flavor-api` diagnostic when non-empty.

### `globals.read`

- **Type:** `string[]`
- **Default:** `[]`

Global names that may be accessed without triggering `undefined-global`.

> **Tip:** `SavedVariables` and `SavedVariablesPerCharacter` declared in `.toc` files are automatically added to both `globals.read` and `globals.write` — no manual configuration needed.

### `globals.write`

- **Type:** `string[]`
- **Default:** `[]`

Global names that may be created/assigned without triggering `create-global`.

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
| `ignore` | Relative to containing directory |
| `framexml` | Nearest (deepest) config wins |
| `flavors` | Nearest (deepest) config wins |
| `globals.read` | Unioned across ancestors |
| `globals.write` | Unioned across ancestors |
| `inference.*` | Nearest (deepest) config wins |
| `diagnostics.disable` | Unioned across ancestors |
| `diagnostics.enable` | Applied after `disable` at each level |
| `diagnostics.severity` | Deeper configs take precedence |
