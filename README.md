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
- **Semantic Tokens** — Marks function-valued names (e.g. globals passed as callbacks) with the `function` token so themes color them like a call site, plus `defaultLibrary` for WoW API stubs and `deprecated` for `@deprecated` functions
- **Diagnostics** — 30+ semantic checks (type mismatches, undefined globals/fields, unused locals, nil safety, and more)

### Annotation support
Supports [LuaLS](https://luals.github.io/)-style annotations:

| Annotation | Description |
|---|---|
| `@param` | Function parameter types and optionality |
| `@return` | Return types (use separate lines for multiple returns; `@return ...T` for variadic) |
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
| `@correlated` | Declare fields that are always nil/non-nil together (see below) |
| `@see` | Cross-reference link(s) to related symbols or URLs, shown in hover |
| `@flavor-narrows` | Mark a function as a flavor guard (`@flavor-narrows retail`) — see below |

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

### Tuple-union returns (`@return (A, B) | (C, D)`)

Functions that return either all values or nothing (or have discriminated returns like `pcall`) use a **tuple-union** `@return` annotation — a union of parenthesized tuples, each representing one case. The LS derives per-position column types for hover, generates per-case overloads for call-site narrowing, and picks up labels from the first tuple.

```lua
---@return (string name, number level)
---      | (nil, nil)
function getPlayer(id)
    local player = findPlayer(id)
    if not player then return end
    return player.name, player.level
end
```

When any return value is nil-checked, all siblings from the same multi-return assignment are narrowed together:

```lua
local name, level = getPlayer(id)
-- name: string | nil, level: number | nil

if name then
    -- name: string, level: number (both narrowed)
end
```

This works with all narrowing patterns: `if x then`, `if x ~= nil then`, `if not x then error() end`, `if x == nil then return end`, `assert(x)`, `type(x) == "typename"`, and `while not x do ... end` (post-loop). The `type()` guard also works on field chains: `if type(obj.field) == "table" then` narrows `obj.field` to `table`.

**Labels** come from the first tuple's position names. Subsequent tuples list only types:

```lua
---@return (string name, number level)
---      | (nil, nil)
---      | (nil, number)
```

Position 0 is labeled `name`, position 1 is labeled `level`, for all cases.

**Per-case descriptions** can be added as trailing text after the closing `)`, optionally with an `@` prefix:

```lua
---@return (true ok, number value) success
---      | (false, string) @ failure
```

The description shows next to each case in hover.

**Single-tuple shorthand** works as the preferred form for labeled multi-returns, replacing the legacy multi-line `@return T name` style:

```lua
---@return (string firstName, number age)
function getPerson() ... end
```

A single tuple without a union declares a non-correlated multi-return — it provides labels and per-position types, but no sibling narrowing (since there's only one case).

**Parse rule:** a `@return` line is tuple-form when its top-level body is a parenthesized list of ≥2 comma-separated positions, or a `|`-union where every alternative is such a tuple. `(T)` with a single element is plain grouping, not a tuple.

**Continuation lines** use `---| (tuple)` (any indentation after `---`). The union extends across continuation lines:

```lua
---@return (A) | (B)
---      | (C)
```

**Inline uses** — tuple-union works inside `fun()` return types and `@alias` bodies:

```lua
---@alias ParseResult (true ok, number value) | (false, string error)

---@param cb fun(): (true ok, number v) | (false, string)
function runCallback(cb) ... end

---@return ParseResult
function parse() ... end
```

**Legacy `@return T name` parsing** is still accepted for per-position multi-returns so files shared with LuaLS-based projects don't need migration. Mixing legacy `@return` lines with a tuple-union line on the same function emits `malformed-annotation`.

### Variadic returns (`@return ...T`)

When the last `@return` annotation uses `...T` syntax, it fills all remaining return slots with the inner type:

```lua
---@return number uuid
---@return ...any
function getStuff()
    return 1, "a", true, nil
end

local uuid, a, b, c = getStuff()
-- uuid: number, a: any, b: any, c: any
```

Comma-separated returns on a single `@return` line are supported only inside parens (tuple-form); otherwise use separate `@return` lines.

The `grouped-return-mismatch` diagnostic enforces that each return statement in the function body matches one of the declared tuple-union cases, catching partial returns like `return name, nil`.

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

---@generic T
---@param name string
---@param class T|`T`
---@builds-field 1 T!
---@return self
function Schema:AddDeferred(name, class) return self end
```

The field type supports `T!` (lateinit) — fields created with `T!` allow nil assignment without producing `field-type-mismatch`, and hover shows the `!` marker.

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

### Correlated nil fields (`@correlated`)

`@correlated` declares groups of optional fields on a `@class` that are always nil or non-nil together. When a nil guard narrows any field in the group, all other fields in the group are automatically narrowed too. This eliminates false-positive `type-mismatch` and `need-check-nil` warnings when checking one field implies the others are also set.

```lua
---@class AuctionState
---@correlated itemString, duration, buyout, bid
---@field itemString string?
---@field duration number?
---@field buyout number?
---@field bid number?
---@field cache string?  -- not in the group, remains independently nullable

---@param self AuctionState
local function process(self)
    if self.itemString then
        -- All correlated fields are narrowed to non-nil:
        print(self.duration * 2)    -- no warning
        print(self.buyout + 1)      -- no warning
        -- cache is not correlated, still nullable:
        print(self.cache:upper())   -- need-check-nil warning
    end
end
```

Multiple independent groups can be declared on the same class:

```lua
---@class TradeState
---@correlated handler, money
---@correlated pendingItem, pendingCount
---@field handler function?
---@field money number?
---@field pendingItem string?
---@field pendingCount number?
```

Correlated groups are inherited by child classes.

### Correlated local variables (inferred)

When multiple local variables are assigned in **every branch** of an `if/elseif` chain (without an explicit `else`), the LS automatically infers that they are correlated. Narrowing any one of them via a nil guard also narrows all the others. No annotation is needed — the correlation is detected from the assignment pattern.

```lua
local money = nil    ---@type number?
local tradeType = nil ---@type string?
if condition1 then
    tradeType = "buy"
    money = 100
elseif condition2 then
    tradeType = "sell"
    money = 200
end
if not tradeType then return end
-- Both are narrowed: tradeType is string, money is number
SomeFunction(money)  -- no false type-mismatch
```

This works with all narrowing patterns: `if x then`, `if x ~= nil then`, `if not x then return end`, `if x == nil then return end`, `assert(x)`, and `while not x do ... end` (post-loop).

### Metatable type inference

The LS understands `setmetatable()` and resolves `__index` chains for field/method propagation, `__call` for callable tables, `getmetatable()` return types, and operator metamethods — all without requiring annotations on the metatable itself.

#### `__index` field propagation

```lua
local MyClass = {}
MyClass.__index = MyClass

function MyClass:greet()
    return "hello"
end

local obj = setmetatable({}, MyClass)
obj:greet()  -- resolves to string via __index → MyClass
```

This works with:
- **Self-referential metatables**: `mt.__index = mt` (the most common WoW addon OOP pattern)
- **Inline metatable tables**: `setmetatable({}, { __index = { x = 1 } })`
- **`@class` tables as `__index`**: fields from `@field` declarations propagate through `__index`
- **Factory functions**: `function M.new() return setmetatable({}, M) end`
- **Chained metatables**: `inst → Child → Base` via nested `setmetatable` + `__index`
- **Statement-form**: `setmetatable(t, mt)` without assignment still sets the metatable on `t`
- **Instance field priority**: direct fields on the table take precedence over `__index` fields

#### `__call` metamethod

Tables with a `__call` metamethod (set via `setmetatable`) become callable:

```lua
local Counter = setmetatable({ n = 0 }, {
    __call = function(self) self.n = self.n + 1; return self.n end
})
local val = Counter()  -- resolves to number
```

#### Operator metamethods

Arithmetic and other operators check metatables for the corresponding metamethods. The metamethod function's `@return` annotation determines the operator result type:

```lua
---@class Vec2
---@field x number
---@field y number
local Vec2 = {}
Vec2.__index = Vec2

---@param a Vec2
---@param b Vec2
---@return Vec2
Vec2.__add = function(a, b) ... end

---@type Vec2
local a, b
local c = a + b  -- resolves to Vec2 via __add
c.x              -- resolves to number
```

Supported metamethods: `__add` (+), `__sub` (-), `__mul` (*), `__div` (/), `__mod` (%), `__pow` (^), `__concat` (..), `__unm` (unary -), `__len` (#).

#### `getmetatable()`

Returns the raw metatable that was set via `setmetatable()`:

```lua
local mt = { __index = { z = 3 } }
local obj = setmetatable({}, mt)
local m = getmetatable(obj)  -- resolves to mt's table type
```

#### Cross-file support

Metatable inference works cross-file when the `__index` target is an annotated `@class` table (since those are registered globally). For unannotated metatables defined in other files, add a `---@class` annotation to get cross-file type support.

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

### Flavor filtering (`flavors` + `@flavor-narrows`)

WoW ships three flavor families matching Blizzard's install folder names: `retail`, `classic` (the rolling progression, including MoP Classic), and `classic_era`. Many APIs are only available in a subset of these. When a project declares its target flavors in `.wowluarc.json`, the language server emits the `wrong-flavor-api` diagnostic on calls to APIs that aren't available in every declared flavor.

```json
{
  "flavors": ["retail", "classic"]
}
```

Accepted flavor names:

| Name | Meaning |
|---|---|
| `retail` (alias: `mainline`) | The live retail game |
| `classic` | The rolling Classic progression, including MoP Classic |
| `classic_era` | Classic Era (vanilla) |

Stubs carry per-API availability data from Ketho's `vscode-wow-api`. Hovering over such a symbol shows `Flavors: Retail, Classic` so the availability is visible.

Conditional blocks narrow the active flavor set:

```lua
if WOW_PROJECT_ID == WOW_PROJECT_MAINLINE then
    -- active flavors narrowed to "retail" here
    AbbreviateLargeNumbers(100)  -- OK, retail-only API
else
    -- active flavors exclude "retail"
end
```

Mark your own flavor-guard functions with `@flavor-narrows`:

```lua
---@flavor-narrows retail
---@return boolean
local function IsRetail()
    return WOW_PROJECT_ID == WOW_PROJECT_MAINLINE
end

if IsRetail() then
    -- narrowed to retail here
end
```

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
| `implicit-nil-return` | Hint | Bare `return` in function with all-optional `@return` types (disabled by default) |
| `redundant-return-value` | Warning | Return with more values than `@return` |
| `grouped-return-mismatch` | Warning | Return values don't match any tuple-union `@return` case |
| `missing-return` | Warning | Function missing return statement |
| `undefined-global` | Warning | Reference to unresolved global name |
| `undefined-field` | Warning | Accessing nonexistent field on `@class` |
| `need-check-nil` | Warning | Field/method access or call on possibly-nil value (disabled by default) |
| `access-private` | Warning | Accessing `@field private` from outside |
| `access-protected` | Warning | Accessing `@field protected` or `_`-prefixed field from outside hierarchy |
| `duplicate-index` | Warning | Duplicate keys in table constructors |
| `redundant-value` | Warning | Extra values in assignments |
| `unbalanced-assignments` | Warning | More variables than values in assignments |
| `missing-fields` | Warning | Missing required fields when constructing `@class` tables |
| `undefined-doc-class` | Warning | References to undefined class names in `@class Foo: Parent` inheritance position |
| `undefined-doc-name` | Warning | References to undefined type names in annotations (`@param`, `@return`, `@type`, `@field`, `@alias`, etc.) |
| `undefined-doc-param` | Warning | `@param` name not matching function parameters |
| `duplicate-doc-param` | Warning | Duplicate `@param` annotations |
| `duplicate-doc-field` | Warning | Duplicate `@field` annotations |
| `duplicate-doc-alias` | Warning | Duplicate `@alias` declarations for the same name |
| `doc-field-no-class` | Warning | `@field` on a non-`@class` table |
| `circle-doc-class` | Warning | Circular `@class` inheritance chains |
| `malformed-annotation` | Warning | Unknown or incomplete `---@` annotations |
| `builds-field-not-self` | Warning | `@builds-field` method uses `@return ClassName` instead of `@return self` |
| `unknown-diag-code` | Warning | Unknown code in `@diagnostic` directives |
| `duplicate-constructor` | Warning | Multiple `@constructor` annotations on a single class |
| `constructor-return` | Warning | `@constructor` method has return annotations other than `@return self` |
| `count-down-loop` | Warning | Numeric for-loop step direction doesn't match start/end values |
| `wrong-flavor-api` | Warning | API call not available in all declared project flavors (see `flavors` config) |
| `return-self-class-name` | Hint | Method uses `@return ClassName` instead of `@return self` |
| `unused-local` | Hint | Unreferenced local variables |
| `unused-function` | Hint | Unused function definitions |
| `unused-vararg` | Hint | Function declares `...` but never uses it (disabled by default) |
| `redefined-local` | Hint | Same-scope local variable redefinition |
| `create-global` | Hint | Implicit global creation (assignment/function definition without `local`) |
| `inject-field` | Hint | Setting undeclared fields on `@class` tables |
| `duplicate-set-field` | Hint | Setting an already-set field on `@class` tables |
| `unreachable-code` | Hint | Code after return |
| `code-after-break` | Hint | Code after break |
| `incomplete-signature-doc` | Hint | Function has partial `@param`/`@return` annotations — some params undocumented or return undocumented (disabled by default) |
| `empty-block` | Hint | Empty `if`/`elseif`/`else`/`while`/`for`/`repeat` body (suppressed if the body contains a comment) |
| `redundant-return` | Hint | Bare `return` as the final statement of a function's top block |
| `trailing-space` | Hint | Line ends with whitespace (blank lines skipped) |
| `not-precedence` | Hint | `not x <cmp> y` parses as `(not x) <cmp> y` — likely unintended |

## Project Configuration

Place a `.wowluarc.json` file in any directory to configure the language server for that directory and its subdirectories. All fields are optional.

```json
{
  "ignore": ["Libs/", "External/"],
  "framexml": false,
  "flavors": ["retail", "classic"],
  "globals": {
    "read": ["LibStub", "AceDB"],
    "write": ["MyAddonDB", "SLASH_MYADDON1"]
  },
  "inference": {
    "backward_param_types": true,
    "correlated_return_overloads": true
  },
  "diagnostics": {
    "disable": ["unused-local", "inject-field"],
    "enable": ["need-check-nil"],
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
| `flavors` | Array of WoW flavor names the project targets. Enables the `wrong-flavor-api` diagnostic. Accepts `retail` (or `mainline`), `classic`, `classic_era`. When omitted or empty, flavor filtering is disabled (backward-compatible default). |
| `globals.read` | Array of global names that may be accessed without triggering `undefined-global`. Use for globals provided by other addons or libraries not in stubs. |
| `globals.write` | Array of global names that may be created/assigned without triggering `create-global`. Use for globals your addon intentionally exports. |
| `inference.backward_param_types` | Boolean. Infer unannotated function-parameter types from body usage (arithmetic ops, concatenation, unary minus, typed-function arg calls). Default: `true`. Set to `false` in strict-typing projects where missing `@param` annotations should stay visible. |
| `inference.correlated_return_overloads` | Boolean. Infer correlated return-only overloads for functions whose return statements form a clear all-set-or-all-nil pattern (no `@return` annotations, matching arity ≥ 2, ≥ 1 all-nil tuple, ≥ 1 all-set tuple, no mixed-nil tuples). Lets call sites get sibling narrowing — guarding one return value narrows the others. Default: `true`. Set to `false` if the inferred narrowing would suppress `need-check-nil` warnings you actually want. See [Correlated return-only overload inference](#correlated-return-only-overload-inference) below. |
| `diagnostics.disable` | Array of diagnostic codes to suppress for files in this directory tree. |
| `diagnostics.enable` | Array of diagnostic codes to opt back in for files in this directory tree. Use this to re-enable diagnostics that are disabled by default (currently `implicit-nil-return`, `need-check-nil`, and `unused-vararg`) or to override a `disable` in a parent config. |
| `diagnostics.severity` | Map of diagnostic code to severity override (`"error"`, `"warning"`, `"info"`, `"hint"`). |

Config files are hierarchical, like `.gitignore`: place one at the workspace root for project-wide settings, and additional ones in subdirectories for directory-specific overrides. Ignore patterns are relative to the directory containing the config file. Disabled diagnostics and allowed globals are unioned across all ancestor configs, with `diagnostics.enable` applied after `diagnostics.disable` at each level so a child can re-enable what a parent disabled. Severity overrides from deeper configs take precedence. The `framexml` setting uses the nearest (deepest) config value.

Configs are discovered during workspace scanning and automatically reloaded when any `.wowluarc.json` is saved.

### Correlated return-only overload inference

Setting `inference.correlated_return_overloads: true` opts in to a synthesis pass that detects "all-set or all-nil" return patterns and gives them the same sibling narrowing that a hand-written tuple-union `@return` provides. For example:

```lua
-- Correlated returns: a and b are always set together or both nil
local function getThing()
    if found then
        return name, level
    end
    return nil, nil
end

local a, b = getThing()
if a then
    -- With the inference flag on, b also narrows to non-nil here
    print(a, b)
end
```

A function qualifies for inference when **all** of the following hold:

* No `@return` annotation is declared on it.
* The function isn't `@return ...T` (variadic) or annotated as void-returning.
* It has at least two `return` statements with matching arity ≥ 2.
* Every `return` tuple is either entirely `nil` literals OR has no `nil` literal positions — mixed tuples like `return "ok", nil` are skipped to avoid false correlations.
* At least one tuple is all-nil and at least one tuple is all-set.

When all these hold, one synthetic return-only overload per unique tuple is added (string/number/boolean literals normalize to their generic types; non-literal expressions become `any`; `nil` stays `nil`). Sibling narrowing then propagates through the existing return-only overload pipeline. The flag is **on** by default since the pattern is common in legacy WoW code; set it to `false` if your project relies on per-value `need-check-nil` warnings that the inferred narrowing would silently suppress.

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

WoW API type definitions are loaded from `stubs/` (precomputed from [Ketho/vscode-wow-api](https://github.com/Ketho/vscode-wow-api)). These provide type information for the retail WoW API (frames, widgets, global functions, enums, etc.). Local overrides live in `stubs/overrides/`. Run `cargo run -- regenerate-stubs` to regenerate them (clones the upstream repo to a temp directory).

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
