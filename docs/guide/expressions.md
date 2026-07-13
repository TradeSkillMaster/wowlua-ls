# Expression Strings

Some addons embed Lua expressions inside string literals. For example, [LibTSMReactive](https://github.com/TradeSkillMaster/LibTSMReactive) evaluates expressions against state fields when dependencies change. wowlua-ls supports the `expression<C, R>` type to bring full language server features into these embedded expressions:

- **Hover** on identifiers inside expression strings
- **Completions** for available fields
- **Go-to-definition** for field declarations
- **Diagnostics** for undefined variables and type mismatches
- **Syntax highlighting** for identifiers, keywords, numbers, and operators

## Basic usage

Define a class whose fields become the available variables in the expression, then use `expression<ClassName>` as a parameter type:

```lua
---@class ScanState
---@field scanProgress number
---@field isScanning boolean
---@field doneScanning boolean
---@field scanIsPaused boolean?

---@param expressionStr expression<ScanState>
---@return ReactivePublisher
function ScanState:Publisher(expressionStr) end
```

Now string arguments to `Publisher` get full LS support:

```lua
state:Publisher([[scanProgress == 1 and not isScanning]])
--               ^ hover: scanProgress: number
--                                          ^ completions: scanProgress, isScanning, doneScanning, ...
```

Identifiers inside the expression string resolve against the fields of `ScanState`. Unknown identifiers produce an `undefined-field` warning.

## `expression<self>`

Use `expression<self>` to resolve fields from the receiver's actual class at the call site. This is how LibTSMReactive's `Publisher` method works - the same method supports any state schema:

```lua
---@class ReactiveState
---@param expressionStr expression<self>
---@return ReactivePublisher
function ReactiveState:Publisher(expressionStr) end
```

Each call site resolves `self` to the receiver's concrete type:

```lua
local STATE_SCHEMA = Reactive.CreateStateSchema("AuctioningState")
    :AddNumberField("scanProgress", 0)
    :AddBooleanField("isScanning", false)
    :AddBooleanField("doneScanning", false)
    :AddBooleanField("canProcess", false)
    :Commit()

local state = STATE_SCHEMA:CreateState()

-- Fields resolve against AuctioningState specifically
state:Publisher([[scanProgress == 1 and not isScanning]])
state:Publisher([[doneScanning or scanIsPaused]])
```

## Return type constraints

The optional second type parameter constrains what the expression must evaluate to:

```lua
---@param expr expression<ScanState, boolean>
function ScanState:PublishBool(expr) end
```

With a return type constraint, the LS infers the expression's type and warns if it doesn't match:

```lua
-- OK: comparison returns boolean
state:PublishBool([[scanProgress == 1]])

-- OK: boolean operators return boolean
state:PublishBool([[not isScanning]])

-- Warning: expression returns 'number', expected 'boolean'
state:PublishBool([[scanProgress]])
```

Without the second parameter (`expression<C>`), any return type is accepted.

## Inferring the result type with a generic

When the second type parameter is a `@generic`, it is **inferred** from the expression body rather than checked against it, and the inferred type flows into the method's return type. This lets a single method return a result type that depends on what the caller's expression evaluates to:

```lua
---@class ReactivePublisherSchema<R>
local ReactivePublisherSchema = {}

---@generic R
---@param expr expression<ReactiveState, R>
---@return ReactivePublisherSchema<R>
function ReactiveState:Publisher(expr) end
```

The LS infers `R` from the expression and substitutes it into the return:

```lua
-- numProgress is a number field → R = number
local p1 = state:Publisher([[progress + 1]])
-- p1: ReactivePublisherSchema<number>

-- comparison → R = boolean
local p2 = state:Publisher([[progress == 1]])
-- p2: ReactivePublisherSchema<boolean>
```

If the expression's type can't be inferred (for example it references an unknown name), `R` falls back to `any`.

## Additional functions with intersection types

Expression DSLs often provide utility functions (like `min`, `max`) alongside state fields. Use an intersection type in the first parameter to compose the state class with a class declaring available functions:

```lua
---@class ReactiveExprBuiltins
---@field min fun(a: number, b: number): number
---@field max fun(a: number, b: number): number

---@class ReactiveState
---@param expressionStr expression<self & ReactiveExprBuiltins>
---@return ReactivePublisher
function ReactiveState:Publisher(expressionStr) end
```

Now both state fields and the declared functions are recognized inside expressions:

```lua
state:Publisher([[min(baseItemBagQuantity, maxItemStack)]])
--               ^ hover: min: fun(a: number, b: number): number
--                   ^ completions: ..., min, max
```

You can intersect any number of classes: `expression<State & Builtins & MoreStuff>`.

All identifiers in the expression (including function call names) must be declared in one of the context classes, or they will produce an `undefined-field` warning.

## What counts as an expression

Expression strings are parsed as Lua expressions (not statements). Valid expressions include:

- Field references: `scanProgress`, `isScanning`
- Comparisons: `scanProgress == 1`, `quantity > maxCanAfford`
- Boolean operators: `not isScanning`, `doneScanning or scanIsPaused`
- Arithmetic: `num1 + num2`, `-1 * num1`
- String concatenation: `prefix .. suffix`
- Literals: `true`, `nil`, `42`, `"hello"`
- Parenthesized groups: `(doneScanning or scanIsPaused) and not pausePending`

## Type inference rules

The LS uses simple rule-based inference for expression return types:

| Expression | Inferred type |
|---|---|
| Field reference | Field's declared type |
| `==`, `~=`, `<`, `>`, `<=`, `>=` | `boolean` |
| `not x` | `boolean` |
| `and` | Type of right operand |
| `or` | Union of left and right types |
| `+`, `-`, `*`, `/`, `%`, `^` | `number` |
| `-x` (unary minus) | `number` |
| `#x` | `number` |
| `..` | `string` |
| Number literal | `number` |
| String literal | `string` |
| `true`, `false` | `boolean` |
| `nil` | `nil` |

## String formats

Expression strings work with all Lua string literal formats:

```lua
state:Publisher([[scanProgress == 1]])       -- long brackets (most common)
state:Publisher("scanProgress == 1")         -- double quotes
state:Publisher('scanProgress == 1')         -- single quotes
state:Publisher([=[scanProgress == 1]=])     -- level-1 long brackets
```

## Diagnostics

Expression strings reuse existing diagnostic codes:

- **`undefined-field`**: identifier not found in the expression class's fields
- **`type-mismatch`**: expression return type doesn't match the declared constraint

Both can be suppressed with `@diagnostic disable:code` as usual.

## Inheritance

Expression fields are resolved from the class and all its parent classes:

```lua
---@class BaseState
---@field enabled boolean

---@class ScanState : BaseState
---@field scanProgress number

---@param expr expression<ScanState>
function ScanState:Publisher(expr) end

-- Both 'enabled' (from BaseState) and 'scanProgress' are available
state:Publisher([[enabled and scanProgress > 0]])
```
