# wowlua-ls

A Language Server Protocol implementation for World of Warcraft addon development. Provides intelligent Lua editing with full support for LuaLS-style annotations and the WoW API.

## Features

### LSP capabilities
- **Hover** — Type information and documentation on mouse-over
- **Go to Definition** — Jump to symbol definitions, including across files
- **Completions** — Context-aware suggestions for `.`/`:` field access, scope symbols, and `---@` annotation tags, parameter names, and types
- **Signature Help** — Parameter hints for function calls
- **Find References** — Locate all usages of a symbol
- **Rename** — Safe symbol renaming across scopes
- **Diagnostics** — 30+ semantic checks (type mismatches, undefined globals/fields, unused locals, nil safety, and more)

### Annotation support
Supports [LuaLS](https://luals.github.io/)-style annotations:

| Annotation | Description |
|---|---|
| `@param` | Function parameter types and optionality |
| `@return` | Return types |
| `@type` | Variable type annotation |
| `@class` | Class definition with inheritance and type parameters |
| `@field` | Class field with visibility (public/private/protected). Fields starting with `_` are implicitly protected |
| `@alias` | Type aliases (supports type parameters: `@alias Foo<K,V> V[]`) |
| `@overload` | Function overload signatures (`fun(...)` and `return:` variants) |
| `@generic` | Generic type parameters on functions |
| `@defclass` | Class factory pattern (see below) |
| `@deprecated` | Mark symbols as deprecated |
| `@nodiscard` | Warn when return values are ignored |
| `@meta` | Declaration-only files (suppresses all diagnostics) |
| `@diagnostic` | Suppress specific diagnostics inline |
| `@cast` | Type cast assertion for variables (`+` add, `-` remove, or replace) |
| `@as` | Inline expression type assertion (`--[[@as Type]]`) |
| `@builds-field` | Builder method that adds a typed field (see below) |
| `@return built` | Return the accumulated built type (see below) |
| `@built-name` | Name the built type from a string literal parameter (see below) |
| `@built-extends` | New built type inherits from receiver's current built type (see below) |
| `@type-narrows` | Custom type guard function for narrowing (see below) |

Type syntax supports unions (`A | B`), intersections (`A & B`), arrays (`T[]`), parameterized types (`table<K, V>`), anonymous table shapes (`{field: type}`), generics, optionals (`T?`), and non-nil assertions (`T!`).

### Anonymous table shapes (`{field: type}`)

Anonymous table shapes describe inline table structures with named fields:

```lua
---@param opts {name: string, count: number, verbose?: boolean}
local function create(opts)
    print(opts.name)    -- string
    print(opts.verbose) -- boolean | nil
end
```

They can be combined with intersections and used in aliases:

```lua
---@alias EncodedData string[]&{compressed: boolean}
```

### Intersection types (`T & U`)

Intersection types represent values that have all properties of every member type. Use `&` to combine types:

```lua
---@param widget Frame & BackdropTemplate
local function setupWidget(widget)
    widget:SetPoint("CENTER")  -- Frame method
    widget:SetBackdrop({})     -- BackdropTemplate method
end
```

`&` binds tighter than `|`, so `A | B & C` means `A | (B & C)`. An intersection is assignable to any of its member types, and field access checks all members.

### Non-nil assertions (`T!`)

The `!` suffix on a type declares a "lateinit" field — one that may be `nil` at runtime (e.g., in object pools between acquire/release) but should be treated as non-nil by the type checker. This is similar to Swift's implicitly unwrapped optionals (`T!`) or Kotlin's `lateinit`.

```lua
---@class PooledQuery
---@field _db DatabaseTable!
---@field _clause QueryClause!

-- Assigning nil is allowed (no field-type-mismatch):
self._db = nil

-- Accessing the field doesn't require a nil guard (no need-check-nil):
self._db:Query()

-- Hover shows the type with !: _db: DatabaseTable!
```

Also works with inline `---@type` on field initializers:

```lua
self._db = nil ---@type DatabaseTable!
```

### Type casting

`@cast` changes a variable's type from that point onward. Supports replace, add (`+`), and remove (`-`) modes:

```lua
---@type string|number|nil
local x = getValue()

---@cast x string          -- replace: x is now string
---@cast x +boolean        -- add: x is now string | boolean
---@cast x -nil            -- remove: strip nil from x's type
```

`@as` asserts a type inline on an expression using block comment syntax:

```lua
local x = getValue() --[[@as string]]
doSomething(x --[[@as MyClass]])
```

### Generics

Functions can declare generic type parameters with `@generic`:

```lua
---@generic T
---@param value T
---@return T
function identity(value) return value end
```

Generic parameters can be constrained to a class: `@generic T: SomeClass`.

### Return-only overloads (`@overload return:`)

Functions that return either all values or nothing (or have discriminated returns like `pcall`) can use return-only overloads to enable sibling narrowing at call sites. Unlike `@overload fun(...)` which duplicates parameter lists, `@overload return:` specifies only return type variants:

```lua
---@return string? name
---@return number? level
---@overload return: string, number
---@overload return: nil
function getPlayer(id)
    local player = findPlayer(id)
    if not player then return end
    return player.name, player.level
end
```

When any return value from such a function is nil-checked, all siblings from the same multi-return assignment are narrowed together:

```lua
local name, level = getPlayer(id)
-- name: string?, level: number?

if name then
    -- name: string, level: number (both narrowed)
end
```

This works with all narrowing patterns: `if x then`, `if x ~= nil then`, `if not x then error() end`, `if x == nil then return end`, and `assert(x)`.

The `grouped-return-mismatch` diagnostic enforces that each return statement in the function body matches one of the declared `@overload return:` patterns, catching partial returns like `return name, nil`.

### Class factory pattern (`@defclass`)

The `@defclass` annotation declares a function as a class factory — calling it creates a new class whose name is inferred from the first string argument. This enables hover, completion, and diagnostics for OOP patterns like LibTSMClass.

```lua
---@generic T: MyBaseClass
---@defclass T
---@param name `T`
---@return T
function CreateClass(name) return {} end
```

Classes created via `@defclass` inherit fields and methods from their constraint class. Use `@defclass T : P` to support a parent class parameter:

```lua
---@class MyBase<S>
---@field __super S

---@generic T: MyBase<P>
---@generic P: MyBase
---@defclass T : P
---@param name `T`
---@param parent? P
---@return T
function CreateClass(name, parent) return {} end
```

With parameterized classes (`@class MyBase<S>`), type parameters on fields like `@field __super S` are automatically substituted with the concrete parent class at each call site:

```lua
local Animal = CreateClass("Animal")
local Dog = CreateClass("Dog", Animal)
Dog.__super  -- typed as Animal
```

### Builder pattern (`@builds-field` + `@return built`)

The `@builds-field` and `@return built` annotations support method-chaining builder patterns where each call adds a typed field to a result type.

`@builds-field <param_index> <type>` declares that a builder method adds a field whose name is the string literal at the given 1-based parameter index:

```lua
---@class Schema
local Schema = {}

---@param name string
---@builds-field 1 string
---@return self
function Schema:AddString(name) return self end

---@param name string
---@builds-field 1 number?
---@return self
function Schema:AddNumber(name) return self end
```

`@return built` returns the accumulated type with all fields added by the chain:

```lua
---@return built
function Schema:Build() return {} end

local inst = Schema:AddString("label"):AddNumber("count"):Build()
inst.label  -- string
inst.count  -- number?
```

The built type can optionally inherit from a parent class with `@return built : ParentClass`:

```lua
---@class State
---@field GetValue fun(self, key: string): any

---@return built : State
function Schema:CreateState() return {} end

local state = Schema:AddString("name"):CreateState()
state.name       -- string (from builder chain)
state:GetValue() -- inherited from State
```

#### Naming built types (`@built-name`)

By default, built types inherit their schema's class name. Use `@built-name <param_idx>` on the chain entry point to give the built type a custom name from a string literal argument. This registers the name globally so other files can reference it in `@param`/`@type` annotations:

```lua
---@built-name 1
---@return self
function Schema.Create(name) return Schema end

local MY_SCHEMA = Schema.Create("MyStateType")
    :AddString("label")
    :Commit()

local state = MY_SCHEMA:Build()
-- state has type MyStateType { label: string }

---@param s MyStateType   -- works in @param, @type, etc.
function useIt(s) end
```

#### Extending builder schemas (`@built-extends`)

Use `@built-extends` with `@built-name` on a method to create a new built type that inherits from the receiver's current built type. This supports schema extension patterns where a base schema defines common fields and subclasses extend it:

```lua
---@param name string
---@built-name 1
---@built-extends
---@return self
function Schema:Extend(name)
    return self
end

-- Base schema with common fields
local BASE = Schema:AddString("baseName"):AddBool("active"):Commit()

-- Child extends base — inherits baseName and active
local CHILD = BASE:Extend("ChildState"):AddString("childField"):Commit()

local inst = CHILD:Build()
inst.childField  -- string (own field)
inst.baseName    -- string (inherited from base)
inst.active      -- boolean (inherited from base)

-- Multi-level: grandchild extends child, inherits from both
local GRAND = CHILD:Extend("GrandState"):AddNumber("grandNum"):Commit()
local g = GRAND:Build()
g.grandNum    -- number (own field)
g.childField  -- string (from child)
g.baseName    -- string (from base, through child)
```

### Custom type guards (`@type-narrows`)

`@type-narrows` marks a function as a type guard that narrows a variable's type when used as a truthiness condition (in `if`, early-exit, or `assert()`).

**Index-based form**: `@type-narrows <target_param> <classname_param>` — both indices are 1-based call-site argument positions. Use `0` for the receiver (self) in colon method calls.

```lua
---@param element Element
---@param typeName string
---@type-narrows 1 2
---@return boolean
function UIElements.IsType(element, typeName) end

---@param parent Element
local function example(parent)
    if UIElements.IsType(parent, "BaseScrollFrame") then
        parent._scrollbar  -- parent is now BaseScrollFrame
    end

    if not UIElements.IsType(parent, "BaseScrollFrame") then return end
    parent._scrollbar  -- also narrowed after early exit
end
```

**Method-style form**: `@type-narrows ClassName` — narrows `self` to the specified class. Useful for boolean predicate methods on class hierarchies.

```lua
---@class AuctionRow
---@class AuctionSubRow : AuctionRow

---@type-narrows AuctionSubRow
---@return boolean
function AuctionRow:IsSubRow() return false end

---@param row AuctionRow
local function example(row)
    if row:IsSubRow() then
        row  -- narrowed to AuctionSubRow
    end

    -- Also works inside assert(), including compound conditions:
    assert(row:IsSubRow())
    row  -- narrowed to AuctionSubRow
end
```

### Literal boolean union discrimination

When a union type has a method that returns literal `true` on some members and literal `false` on others, the LS automatically narrows the union in conditional branches. No extra annotations are needed beyond `@return true` / `@return false`.

```lua
---@class AuctionRow
---@field buyout number
local AuctionRow = {}

---@return false
function AuctionRow:IsSubRow() return false end

---@class AuctionSubRow
---@field parentRowId number
local AuctionSubRow = {}

---@return true
function AuctionSubRow:IsSubRow() return true end

---@param row AuctionRow | AuctionSubRow
local function example(row)
    if row:IsSubRow() then
        row  -- narrowed to AuctionSubRow (returns true)
    else
        row  -- narrowed to AuctionRow (returns false)
    end

    -- Also works with early-exit and assert():
    if not row:IsSubRow() then return end
    row  -- narrowed to AuctionSubRow

    assert(row:IsSubRow())
    row  -- narrowed to AuctionSubRow
end
```

Requirements for auto-discrimination:
- **All** union member types must define the method
- **Every** method must return either literal `true` or literal `false` (not generic `boolean`)
- At least one member must return `true` and at least one must return `false`

Works with 3+ member unions: types returning `true` are kept in the then-branch, types returning `false` in the else-branch.

### Implicit protected for `_`-prefixed fields

Data fields whose names start with `_` are implicitly treated as `protected`. They can be accessed from within the same class or its subclasses, but accessing them from outside the class hierarchy produces an `access-protected` warning. This matches the common Lua convention of using `_` to indicate internal data.

```lua
---@class MyClass
---@field _internal number    -- implicitly protected
---@field public _exposed string  -- explicit public overrides
---@field private _secret boolean -- explicit private stays private
```

The implicit protection applies to data fields only — not methods:
- `@field` declarations without an explicit visibility keyword
- Runtime field assignments (e.g. `self._data = 42`)
- Table constructor fields (e.g. `{ _key = value }`)

Methods (`function Foo:_helper()`) are **not** affected and remain public by default. Use `@private` or `@protected` to restrict method access explicitly.

To make a `_`-prefixed field public, use `@field public _name type`.

### Diagnostics

Each diagnostic can be individually suppressed with `---@diagnostic disable:diagnostic-name`.

For compatibility with LuaLS, the following diagnostic code aliases are also accepted:

| Alias | Maps to |
|---|---|
| `invisible` | `access-private`, `access-protected` |
| `param-type-mismatch` | `type-mismatch` |
| `return-type-mismatch` | `return-mismatch` |

| Diagnostic | Severity | Description |
|---|---|---|
| `deprecated` | Warning | Usage of `@deprecated` symbols |
| `discard-returns` | Warning | Ignoring `@nodiscard` return values |
| `type-mismatch` | Warning | Argument type vs `@param` mismatch |
| `return-mismatch` | Warning | Return type vs `@return` mismatch |
| `field-type-mismatch` | Warning | Field assignment vs `@field` type mismatch |
| `assign-type-mismatch` | Warning | Reassignment vs `@type` mismatch |
| `generic-constraint-mismatch` | Warning | Generic argument doesn't satisfy class constraint |
| `missing-parameter` | Warning | Missing required function arguments |
| `redundant-parameter` | Warning | Extra function arguments |
| `missing-return-value` | Warning | Return with fewer values than `@return` |
| `implicit-nil-return` | Hint | Bare `return` in function with all-optional `@return` types |
| `redundant-return-value` | Warning | Return with more values than `@return` |
| `grouped-return-mismatch` | Warning | Return values don't match any `@overload return:` pattern |
| `missing-return` | Warning | Function missing return statement |
| `undefined-global` | Warning | Reference to unresolved global name |
| `undefined-field` | Warning | Accessing nonexistent field on `@class` |
| `need-check-nil` | Warning | Field/method access or call on possibly-nil value |
| `access-private` | Warning | Accessing `@field private` from outside |
| `access-protected` | Warning | Accessing `@field protected` or `_`-prefixed field from outside hierarchy |
| `duplicate-index` | Warning | Duplicate keys in table constructors |
| `redundant-value` | Warning | Extra values in assignments |
| `unbalanced-assignments` | Warning | More variables than values in assignments |
| `missing-fields` | Warning | Missing required fields when constructing `@class` tables |
| `undefined-doc-class` | Warning | References to undefined class names in annotations |
| `undefined-doc-param` | Warning | `@param` name not matching function parameters |
| `duplicate-doc-param` | Warning | Duplicate `@param` annotations |
| `duplicate-doc-field` | Warning | Duplicate `@field` annotations |
| `doc-field-no-class` | Warning | `@field` on a non-`@class` table |
| `circle-doc-class` | Warning | Circular `@class` inheritance chains |
| `malformed-annotation` | Warning | Unknown or incomplete `---@` annotations |
| `builds-field-not-self` | Warning | `@builds-field` method uses `@return ClassName` instead of `@return self` |
| `unknown-diag-code` | Warning | Unknown code in `@diagnostic` directives |
| `duplicate-constructor` | Warning | Multiple `@constructor` annotations on a single class |
| `constructor-return` | Warning | `@constructor` method has return annotations other than `@return self` |
| `return-self-class-name` | Hint | Method uses `@return ClassName` instead of `@return self` |
| `unused-local` | Hint | Unreferenced local variables |
| `unused-function` | Hint | Unused function definitions |
| `redefined-local` | Hint | Same-scope local variable redefinition |
| `create-global` | Hint | Implicit global creation (assignment/function definition without `local`) |
| `inject-field` | Hint | Setting undeclared fields on `@class` tables |
| `duplicate-set-field` | Hint | Setting an already-set field on `@class` tables |
| `unreachable-code` | Hint | Code after return |
| `code-after-break` | Hint | Code after break |

## Project Configuration

Place a `.wowluarc.json` file in any directory to configure the language server for that directory and its subdirectories. All fields are optional.

```json
{
  "ignore": ["Libs/", "External/"],
  "framexml": false,
  "globals": {
    "read": ["LibStub", "AceDB"],
    "write": ["MyAddonDB", "SLASH_MYADDON1"]
  },
  "diagnostics": {
    "disable": ["unused-local", "inject-field"],
    "severity": {
      "undefined-global": "error",
      "unused-function": "warning"
    }
  }
}
```

| Field | Description |
|---|---|
| `ignore` | Array of path prefixes to exclude from scanning, relative to the config file's directory. Patterns ending with `/` match directory prefixes. |
| `framexml` | Boolean. Whether FrameXML API globals (e.g. `SetUIPanelAttribute`, `UpdateUIPanelPositions`) are available. Default: `true`. Set to `false` to treat FrameXML globals as undefined in this directory tree. |
| `globals.read` | Array of global names that may be accessed without triggering `undefined-global`. Use for globals provided by other addons or libraries not in stubs. |
| `globals.write` | Array of global names that may be created/assigned without triggering `create-global`. Use for globals your addon intentionally exports. |
| `diagnostics.disable` | Array of diagnostic codes to suppress for files in this directory tree. |
| `diagnostics.severity` | Map of diagnostic code to severity override (`"error"`, `"warning"`, `"info"`, `"hint"`). |

Config files are hierarchical, like `.gitignore`: place one at the workspace root for project-wide settings, and additional ones in subdirectories for directory-specific overrides. Ignore patterns are relative to the directory containing the config file. Disabled diagnostics and allowed globals are unioned across all ancestor configs. Severity overrides from deeper configs take precedence. The `framexml` setting uses the nearest (deepest) config value.

Configs are discovered during workspace scanning and automatically reloaded when any `.wowluarc.json` is saved.

## Building

```bash
cargo build --release
```

## Usage

### As a language server

Run the binary with no arguments to start the LSP server over stdio. Configure your editor to use it for Lua files.

### CLI tools

```bash
# Check a directory for diagnostics (errors + warnings by default)
cargo run -- check path/to/addon

# Include hints (unused locals, inject-field, etc.)
cargo run -- check path/to/addon --severity hint

# Use custom stubs directory instead of built-in WoW API stubs
cargo run -- check path/to/addon --stubs path/to/stubs

# Evaluate a file — prints AST, type info, symbols, and diagnostics
cargo run -- evaluate path/to/file.lua --with-stubs

# Test a query at a specific location (hover, definition, signature, completions, diagnostics)
cargo run -- test-query path/to/file.lua:10:5 --with-stubs
```

The `check` command exits with code 1 if any diagnostics are found, making it suitable for CI pipelines.

## WoW API Stubs

WoW API type definitions are loaded from `stubs/vscode-wow-api/` (a git submodule of [Ketho/vscode-wow-api](https://github.com/Ketho/vscode-wow-api)). These provide type information for the retail WoW API (frames, widgets, global functions, enums, etc.). Local overrides live in `stubs/overrides/`.

### Global strings and variables

WoW defines ~47k global variables at runtime (localized string constants, frame names, UI mixins, etc.) that aren't covered by the Lua annotation stubs. These are generated from `vscode-wow-api/src/data/globals.ts` and `globalstring/enUS.ts`:

- `stubs/overrides/GlobalStrings.lua` — ~21k string constants with actual enUS values
- `stubs/overrides/GlobalVariables.lua` — ~25k other globals (frames, mixins, color constants)

To regenerate:

```bash
python3 generate_global_stubs.py
```

### Classic-only stubs

Classic-era and classic APIs that don't exist in retail are in `stubs/classic/ClassicGlobals.lua`. These are auto-generated by scraping [warcraft.wiki.gg](https://warcraft.wiki.gg) for function signatures and parameter types.

To regenerate:

```bash
python3 generate_classic_stubs.py --include-undocumented
```

This requires Python 3.8+ with no extra dependencies. It downloads the GlobalAPI lists from [BlizzardInterfaceResources](https://github.com/Ketho/BlizzardInterfaceResources) for retail, classic_era, and classic, diffs them to find classic-only APIs, then bulk-exports and parses their wiki pages to produce typed `@param`/`@return` annotations. APIs without wiki pages get bare function stubs when `--include-undocumented` is passed.

## License

GPL-3.0
