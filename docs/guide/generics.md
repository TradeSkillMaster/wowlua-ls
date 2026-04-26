# Generics

Generic type parameters let you write functions and classes that work with any type while preserving type information through calls.

## Generic functions

Declare type parameters with `@generic`:

```lua
---@generic T
---@param value T
---@return T
function identity(value) return value end

local s = identity("hello") -- s: string
local n = identity(42)       -- n: number
```

The LS infers `T` from the argument and propagates it to the return type. No explicit type annotation needed at the call site.

### Constrained generics

Restrict a type parameter to a specific class or type:

```lua
---@generic T: Frame
---@param frame T
---@return T
function configure(frame)
    frame:SetPoint("CENTER")
    return frame
end

local f = configure(CreateFrame("Frame")) -- T is Frame
configure("not a frame") -- type-mismatch: string doesn't satisfy Frame
```

### Multiple type parameters

```lua
---@generic K, V
---@param key K
---@param value V
---@return table<K, V>
function makePair(key, value)
    return { [key] = value }
end
```

### Backtick annotations (`` `T` ``)

When a parameter is a string literal that names a class, use backticks to resolve it as a type:

```lua
---@generic T
---@param name `T`
---@return T
function CreateObject(name) return {} end

local dog = CreateObject("Dog") -- T resolves to the Dog class
```

This is how WoW's `CreateFrame` works — the first argument is a string like `"Frame"` or `"Button"`, and the return type matches.

## Parameterized classes

Classes can declare type parameters that their methods and fields reference:

```lua
---@class Container<T>
---@field items T[]
local Container = {}

---@param item T
function Container:Add(item) end

---@return T
function Container:Get() end
```

When a value is typed with a concrete parameter, fields and methods resolve accordingly:

```lua
---@type Container<string>
local names = {}

names:Add("Arthas")     -- T is string
local n = names:Get()   -- n: string
names.items             -- string[]
```

Methods on a parameterized class automatically inherit the class-level type parameters — no need to redeclare them with `@generic`:

```lua
-- Just use T directly
---@param value T
function MyClass:Set(value) end
```

### Type parameter constraints

```lua
---@class NumericBox<T: number|string>
local NumericBox = {}

---@type NumericBox<string>   -- OK
local a = {}

---@type NumericBox<boolean>  -- generic-constraint-mismatch
local b = {}
```

### Bracket-index fields

Type parameters work in bracket-index field declarations:

```lua
---@class TypedMap<K, V>
---@field [K] V

---@type TypedMap<string, number>
local scores = {}
local val = scores["player1"] -- val: number
```

## Function-type projections

When a generic class wraps a function type, `params<F>` and `returns<F>` let methods reference the function's shape:

```lua
---@class EventRegistry<F>
local EventRegistry = {}

---@generic F
---@param self EventRegistry<F>
---@param key string
---@param ... params<F>
---@return returns<F>
function EventRegistry:Fire(key, ...) end
```

```lua
---@type EventRegistry<fun(name: string, count: number): boolean>
local reg = {}

local ok = reg:Fire("event", "hello", 5) -- ok: boolean
reg:Fire("event", 42, 5) -- type-mismatch: position 1 expects string
```

- `params<F>` is only valid in the vararg slot (`@param ... params<F>`)
- `returns<F>` resolves to F's return type

## How inference works

The LS infers generic bindings from multiple sources:

1. **Direct argument types** — if `@param x T`, and you pass a `string`, T = string
2. **Structural matching** — `T[]` extracts T from an array's element type; `table<K,V>` extracts from map types
3. **Backtick resolution** — `` `T` `` resolves a string literal as a class name
4. **Function-type extraction** — `fun(): T` extracts T from a callback's return type
5. **Receiver type_args** — for `@class Foo<T>`, calling a method on `---@type Foo<string>` binds T from the receiver

Inference runs per-call and doesn't persist — each call site resolves its own bindings independently.