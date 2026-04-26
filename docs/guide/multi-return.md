# Multi-Return Functions

Lua functions can return multiple values, and WoW addon code uses this constantly — `pcall` returns `ok, result`, lookup functions return `value, error`, iterators return `key, value`. wowlua-ls has first-class support for typing multi-return functions, including correlated narrowing across return positions.

## Tuple-union returns

The tuple-union syntax declares that a function returns one of several specific combinations:

```lua
---@return (string name, number level) | (nil, nil)
function getPlayer(id)
    local player = findPlayer(id)
    if not player then return nil, nil end
    return player.name, player.level
end
```

Each parenthesized group is a **case**. The `|` separates cases. The LS derives per-position types from the union of all cases:

- Position 0: `string | nil`
- Position 1: `number | nil`

But it also knows the cases are correlated — when you narrow one position, the incompatible cases are filtered and the other positions narrow too.

### Labels

Names in the first tuple become labels for all cases:

```lua
---@return (string name, number level)
---      | (nil, nil)
```

Position 0 is labeled `name`, position 1 is labeled `level`. Labels show in hover and signature help.

### Per-case descriptions

Add trailing text after `)` to describe each case:

```lua
---@return (true ok, number value) success
---      | (false, string error) @ failure
```

The `@` prefix on the description is optional. Descriptions show in hover next to each case.

### Continuation lines

Use `---|` to continue a tuple-union across lines:

```lua
---@return (number uuid, ...any)
---      | (nil)
```

Single-position tuples like `(nil)` are allowed on continuation lines.

### Mismatched arity

Cases don't need the same number of positions. Shorter cases are implicitly nil-padded:

```lua
---@return (number uuid, ...any)
---      | (nil)
function getFields(n, ...)
    if n == 0 then return nil end
    return n, ...
end

local uuid, a, b = getFields(1, "x", "y")
if uuid then
    -- case 1 only: uuid is number, a and b are any
end
```

## Variadic returns (`@return ...T`)

When the last return annotation uses `...T`, it fills all remaining positions:

```lua
---@return number uuid
---@return ...any
function getStuff()
    return 1, "a", true, nil
end

local uuid, a, b, c = getStuff()
-- uuid: number, a: any, b: any, c: any
```

Variadic returns combine with tuple-union cases:

```lua
---@return (number uuid, ...any)
---      | (nil)
```

After narrowing past the nil case, the vararg tail fills all remaining positions.

## The `grouped-return-mismatch` diagnostic

When a function has tuple-union returns, the LS enforces that every `return` statement matches one of the declared cases:

```lua
---@return (string, number) | (nil, nil)
function example()
    return "hello", 42    -- OK: matches case 1
    return nil, nil        -- OK: matches case 2
    return "hello", nil    -- warning: grouped-return-mismatch
end
```

The partial return `"hello", nil` doesn't match either case — it's likely a bug where you forgot to return the second value.

## Inline uses

Tuple-union works inside `fun()` types and `@alias` bodies:

```lua
---@alias ParseResult (true ok, number value) | (false, string error)

---@param cb fun(): (true, number) | (false, string)
function runCallback(cb) end

---@return ParseResult
function parse() end
```

## Single-tuple shorthand

A single tuple (no `|`) is shorthand for a labeled multi-return without correlation:

```lua
---@return (string firstName, number age)
function getPerson() end
```

This is equivalent to separate `@return` lines but more compact.

## Legacy syntax

The legacy multi-line `@return` syntax still works:

```lua
---@return string name
---@return number level
function getInfo() end
```

Don't mix legacy `@return` lines with tuple-union syntax on the same function — the LS will emit `malformed-annotation`.