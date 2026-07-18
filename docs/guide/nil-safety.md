# Nil Safety and Narrowing

Most runtime crashes in WoW addons are nil errors. A function returns `nil` on failure, a field is unset, a table lookup misses, and your addon throws `attempt to index a nil value` in the middle of a raid. wowlua-ls is built to catch these at edit time.

## The `need-check-nil` diagnostic

The `need-check-nil` diagnostic warns when you access a field or call a method on a value that might be nil:

```lua
---@param name string?
function greet(name)
    print(name:upper()) -- warning: need-check-nil
end
```

This diagnostic is **off by default** because it can be noisy on unannotated codebases. Enable it in `.wowluarc.json` when you're ready:

```json
{
  "diagnostics": {
    "enable": ["need-check-nil"]
  }
}
```

::: tip Enable it early
`need-check-nil` is the single most valuable diagnostic for catching real bugs. Enable it as soon as your core types are annotated. The false positives it produces are usually signals that your annotations are incomplete. Fixing them improves your type coverage everywhere.
:::

## Narrowing

Narrowing is how the LS tracks that a value is no longer nil after you've checked it. wowlua-ls understands every common Lua guard pattern:

### `if x then` / `if x ~= nil then`

```lua
---@param name string?
function greet(name)
    if name then
        print(name:upper()) -- OK, narrowed to string
    end

    if name ~= nil then
        print(name:upper()) -- also works
    end
end
```

### Early exit (`if not x then return end`)

Guard at the top, use freely below:

```lua
---@param player Player?
function processPlayer(player)
    if not player then return end

    -- player is non-nil for the rest of the function
    player:Update()
    print(player.name)
end
```

All early-exit forms work: `return`, `return value`, `error()`, `break` (in loops).

### `assert()`

```lua
---@param frame Frame?
function setupFrame(frame)
    assert(frame)
    frame:SetPoint("CENTER") -- narrowed
end

-- Also works with assert(x, message)
assert(frame, "frame not created")
frame:Show() -- narrowed
```

### `type()` guards

```lua
---@param value string|number|table
function process(value)
    if type(value) == "string" then
        print(value:upper()) -- narrowed to string
    elseif type(value) == "number" then
        print(value + 1)     -- narrowed to number
    else
        -- narrowed to table
    end
end
```

`type()` guards also work on field chains:

```lua
---@param obj {data: string|table}
function handle(obj)
    if type(obj.data) == "table" then
        obj.data[1] -- narrowed to table
    end
end
```

### `while` post-conditions

After a `while not x do` loop, the LS knows `x` is non-nil (since the loop only exits when the condition is false):

```lua
---@type string?
local result = nil
while not result do
    result = tryFetch()
end
-- result: string (narrowed)
```

### Compound `and`/`or`

```lua
---@param a string?
---@param b string?
function combine(a, b)
    if a and b then
        print(a .. b) -- both narrowed
    end

    local name = a or "default" -- name: string (never nil)
end
```

## Correlated multi-return narrowing

This is where wowlua-ls really shines. When a function returns multiple values that are correlated (either all set or all nil), guarding **one** narrows **all of them**:

```lua
---@return (string name, number level) | (nil, nil)
function getPlayer(id) ... end

local name, level = getPlayer(id)
-- name: string | nil, level: number | nil

if name then
    -- Both narrowed: name is string, level is number
    print(name .. " is level " .. level)
end
```

You checked `name`, but `level` also narrowed to non-nil. This works because the tuple-union `@return` tells the LS that `name` and `level` always come together: if one is non-nil, the other must be too.

This eliminates a huge class of false positives. Without correlated narrowing, you'd either need to check every return value individually or suppress the warnings.

### How it works

The tuple-union return syntax `(A, B) | (C, D)` declares that the function returns one of those specific combinations. When you narrow any position, the LS filters the possible cases and derives the types for all other positions from the surviving cases.

```lua
---@return (true ok, number value) | (false, string error)
function parse(input) ... end

local ok, result = parse(input)
if ok then
    -- ok is true, so only case 1 survives → result is number
    print(result + 1)
else
    -- ok is false, so only case 2 survives → result is string
    print("Error: " .. result)
end
```

### Inferred correlations

You don't always need to write tuple-union returns. If the LS can see your function's return statements and they follow an all-set-or-all-nil pattern, it infers the correlation automatically:

```lua
-- No annotations needed
local function findItem(name)
    local item = lookup(name)
    if not item then
        return nil, nil
    end
    return item.id, item.count
end

local id, count = findItem("sword")
if id then
    print(count + 1) -- count also narrowed (inferred correlation)
end
```

This inference is on by default (`inference.correlatedReturnOverloads: true`). It requires:
- No `@return` annotations on the function
- At least 2 return statements with matching arity
- Every return is either all-nil or has no nil positions

## Correlated fields (`@correlated`)

Fields on a class can be correlated too. Declare them with `@correlated`:

```lua
---@class AuctionState
---@correlated itemString, duration, buyout
---@field itemString string?
---@field duration number?
---@field buyout number?
---@field cache string?
```

Now checking one field narrows the whole group:

```lua
---@param state AuctionState
function process(state)
    if state.itemString then
        print(state.duration * 2) -- narrowed, no warning
        print(state.buyout + 1)   -- narrowed, no warning
        print(state.cache:upper()) -- still warns (not in the group)
    end
end
```

Multiple independent groups are supported:

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

## Correlated locals (inferred)

When multiple locals are assigned in every branch of an `if/elseif` chain (without `else`), the LS infers they're correlated:

```lua
local tradeType = nil ---@type string?
local money = nil ---@type number?

if condition1 then
    tradeType = "buy"
    money = 100
elseif condition2 then
    tradeType = "sell"
    money = 200
end

if not tradeType then return end
-- money is also narrowed to number
```

No annotation needed. The LS detects the pattern automatically.

## Guard implications

An early-exit guard that combines two variables establishes a relationship the LS remembers for the rest of the function. A guard of the form `if a and not b then return end` says "if `a` is set, then `b` must also be set", so later, whenever `a` is narrowed truthy, `b` is narrowed non-nil too:

```lua
---@param itemString string?
local function filter(itemString)
    local maxPrice = itemString and GetMaxPrice(itemString) or nil
    if itemString and not maxPrice then
        return true -- no price configured for this item
    end
    -- ...
    if itemString then
        return buyout > maxPrice -- maxPrice narrowed to number, no warning
    end
    return false
end
```

The guard rules out the "`itemString` set but `maxPrice` nil" case, so inside the later `if itemString then` branch `maxPrice` can't be nil. The relationship is dropped if either variable is reassigned, and it only applies after a guard that always runs (a guard nested inside another conditional doesn't leak out).

### Manual `@correlated` for locals

When the automatic inference can't detect the correlation (e.g. variables assigned together across loop iterations), you can declare it manually:

```lua
---@type number?
local numModifiers = nil
---@type number?
local modifierOffset = nil
---@correlated numModifiers, modifierOffset
for part in gmatch(str, pattern) do
    if not numModifiers then
        numModifiers = part
        modifierOffset = 0
    elseif modifierOffset < numModifiers * 2 then
        -- modifierOffset is narrowed to number (not number?)
        modifierOffset = modifierOffset + 1
    end
end
```

Place the `---@correlated` annotation between the local declarations and the code that uses them. The annotation applies to local variables visible in the current scope.

## Lateinit fields (`T!`)

Some fields are conceptually non-nil but may be nil at certain points in the object's lifecycle (like object pool acquire/release cycles). The `!` suffix declares a "lateinit" field:

```lua
---@class PooledTooltip
---@field _frame GameTooltip!
---@field _anchor Frame!
```

Lateinit fields:
- Allow `nil` assignment without `field-type-mismatch`
- Don't require nil guards before access (no `need-check-nil`)
- Show as `T!` in hover so you know the contract

```lua
self._frame = nil     -- OK (allowed for lateinit)
self._frame:Show()    -- OK (no nil check needed)
```

This is similar to Swift's `T!` (implicitly unwrapped optionals) or Kotlin's `lateinit`.

## `x = x or y` coalesce narrowing

The common `x = x or default` idiom is understood:

```lua
---@param name string?
function greet(name)
    name = name or "stranger"
    -- name is now string (never nil)
    print(name:upper())
end
```

This extends to correlated values: if `y` is later narrowed to non-nil, `x` (which was assigned via `x or y`) is also narrowed.