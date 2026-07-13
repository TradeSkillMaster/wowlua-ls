# Basic Annotations

Annotations are special comments that tell the language server about your code's types. They use the `---@` prefix (three dashes, then `@`), which is the same syntax as LuaLS. If you've used LuaLS before, everything you know still works.

## `@param`: Parameter types

Declare what a function expects:

```lua
---@param name string
---@param level number
---@param guild string?
function registerPlayer(name, level, guild)
    -- name: string, level: number, guild: string | nil
end
```

The `?` suffix makes a parameter optional (its type becomes `T | nil`). Callers that omit optional arguments won't get a `missing-parameter` warning.

### When to use it

Annotate parameters on functions that are called from other files or that form your addon's API. For small local helpers, the LS can often infer parameter types from how they're used in the body - see [backward inference](#backward-inference) below.

### Common patterns

```lua
-- Multiple types
---@param value string|number
function display(value) end

-- Callback parameter
---@param handler fun(item: string, count: number): boolean
function forEach(handler) end

-- Table shape parameter
---@param opts {name: string, verbose?: boolean}
function configure(opts) end
```

## `@return`: Return types

Declare what a function returns:

```lua
---@return string
function getName() return self.name end
```

### Multiple return values

Lua functions can return multiple values. Use separate `@return` lines for each:

```lua
---@return string name
---@return number level
function getInfo()
    return "Arthas", 80
end

local name, level = getInfo()
-- name: string, level: number
```

The names after the type (`name`, `level`) are labels - they show in hover and signature help but don't affect type checking.

### Labeled tuple syntax

For multi-return functions, the tuple syntax is often clearer:

```lua
---@return (string name, number level)
function getInfo()
    return "Arthas", 80
end
```

This is equivalent to separate `@return` lines but keeps the signature compact. Where it really shines is [tuple-union returns](/guide/multi-return) for correlated values.

### Optional returns

```lua
---@return string?
function maybeGetName()
    if not self.loaded then return nil end
    return self.name
end
```

The caller sees `string | nil` and the LS will enforce nil checks if `need-check-nil` is enabled.

## `@type`: Variable types

Force a variable's type:

```lua
---@type AuctionEntry[]
local entries = {}

---@type number?
local cachedValue = nil
```

::: tip When to use `@type`
`@type` is most useful when the LS can't infer the type from the right-hand side: empty tables, `nil` initializers, or values from external APIs. For simple assignments like `local x = 5`, the LS already knows the type, so the annotation is optional.
:::

### Inline `@as` casts

When you need a type assertion on an expression (not a variable), use `@as` in a block comment:

```lua
local frame = CreateFrame("Frame") --[[@as Frame]]
doSomething(value --[[@as string]])
```

`@as` is an escape hatch. It tells the LS "trust me, this is the type." Use it when you know more than the LS can infer, but prefer fixing the root cause (adding annotations upstream) when possible.

### `@cast`: Modify a variable's type

`@cast` changes a variable's type from that point forward. It supports three modes:

```lua
---@type string|number|nil
local x = getValue()

---@cast x string          -- replace: x is now string
---@cast x +boolean        -- add: x is now string | boolean
---@cast x -nil            -- remove: strip nil from x
```

`@cast` is useful after runtime checks that the LS can't follow - for example, after a custom validation function that you know guarantees a type.

## Type syntax

Annotations use a rich type syntax for describing values:

| Syntax | Meaning |
|---|---|
| `string`, `number`, `boolean`, `nil`, `any` | Primitive types |
| `table` | Any table |
| `function` | Any function |
| `A \| B` | Union: value is A or B |
| `A & B` | Intersection: value is both A and B |
| `T[]` | Array of T |
| `T?` | Shorthand for `T \| nil` |
| `T!` | Non-nil assertion / lateinit (see [Nil Safety](/guide/nil-safety)) |
| `table<K, V>` | Map with key type K, value type V |
| `fun(a: T, b: U): R` | Function type |
| `{name: string, age: number}` | Anonymous table shape |
| `"literal"` | String literal type |
| `true`, `false` | Boolean literal types |
| `integer` | Integer subtype of number |

### Unions and intersections

Unions are the most common compound type. A value that might be a string or nil is `string | nil` (or `string?`):

```lua
---@param name string|nil
function greet(name)
    if name then
        print("Hello " .. name) -- narrowed to string
    end
end
```

Intersections represent WoW's mixin pattern: a value that has the fields and methods of multiple types combined. The most common case is frames created with templates:

```lua
---@param widget Frame & BackdropTemplate
function setupWidget(widget)
    widget:SetPoint("CENTER")  -- Frame method
    widget:SetBackdrop({})     -- BackdropTemplate mixin method
end
```

`CreateFrame("Frame", nil, nil, "BackdropTemplate")` automatically returns `Frame & BackdropTemplate` - no annotation needed at the call site. See [Mixins and Templates](/guide/classes#mixins-and-templates) for more.

### Anonymous table shapes

When you don't need a named class, use an inline table shape:

```lua
---@param opts {name: string, count: number, verbose?: boolean}
function create(opts)
    print(opts.name)     -- string
    print(opts.verbose)  -- boolean | nil
end
```

Optional fields use `?` before the colon: `verbose?`. They allow `nil`.

### Function types

Function types describe callable values:

```lua
---@param callback fun(item: string, index: number): boolean
function filter(callback) end

---@type fun(): string, number
local getValues
```

Parameter names in function types are for documentation - they show in hover and signature help.

## Backward inference {#backward-inference}

wowlua-ls can often figure out parameter types without annotations by analyzing how parameters are used in the function body. This is called **backward inference**.

```lua
-- No annotations needed: the LS infers x is number from the arithmetic
local function double(x)
    return x * 2
end

-- Infers callback is fun(item: string) from the typed call
---@param items string[]
local function forEach(items, callback)
    for _, item in ipairs(items) do
        callback(item)
    end
end
```

Backward inference works from:
- **Arithmetic and concatenation**: `x + 1` implies `number`, `x .. "hi"` implies `string | number`
- **Typed function arguments**: if `x` is passed to a function expecting `number`, that's a hint
- **Unary operators**: `-x` implies `number`, `#x` implies `table | string`

::: info Inference is conservative
Backward inference treats each usage as an upper bound and intersects them. If a parameter is used in conflicting ways (passed to a function expecting `string` and another expecting `number`), the LS leaves it untyped rather than guessing wrong. When that happens, add an explicit `@param`.
:::

The inference also bails when a function is called with incompatible types at different sites:

```lua
local function register(frame)
    frame:Show()
end

register(GameTooltip)
register(ItemRefTooltip)
-- Two different frame types → inference bails, parameter stays untyped
```

This is intentional. The LS won't pick one caller's type and reject the other. Add `@param frame Frame` (or whatever the common base type is) to resolve it.