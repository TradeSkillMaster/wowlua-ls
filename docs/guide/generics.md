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

### Type argument checking at call sites

When an argument and parameter are the same parameterized class (or the argument is
a subclass that forwards the type parameter, e.g. `Child<T> : Parent<T>`), the type
arguments are compared too — not just the class itself:

```lua
---@param c Container<boolean>
local function wantBool(c) end

---@type Container<number>
local nums = {}
wantBool(nums)   -- type-mismatch: expected Container<boolean>, got Container<number>
```

A union type argument is tolerated when **any** of its members is compatible, so
truthiness idioms whose inferred value type is a union containing the expected type
(e.g. `Container<boolean | number>` against `Container<boolean>`) don't warn.
Positions whose expected or actual type argument is unconstrained (`any` or an
unresolved generic) are skipped.

### Parameterized inheritance

A subclass that forwards its type parameters to a parameterized parent inherits the
parent's fields and methods with the parameter substituted, and is treated as a
subtype for type-argument comparison:

```lua
---@class Box<T>
---@field _value T
local Box = {}
---@return T
function Box:Get() return self._value end

---@class BoolBox<T> : Box<T>   -- forwards T to Box
local BoolBox = {}

---@type BoolBox<number>
local b = {}
local v = b:Get()   -- v: number  (inherited Box:Get with T = number)
```

Only **identity forwarding** parents (`Child<T> : Parent<T>`, same parameters in the
same order) are linked this way. A parent that binds a concrete type
(`Child<T> : Parent<string>`) or reorders parameters is not registered as a
parameterized subtype, since the type arguments can't be compared positionally.

### Type parameter constraints

```lua
---@class NumericBox<T: number|string>
local NumericBox = {}

---@type NumericBox<string>   -- OK
local a = {}

---@type NumericBox<boolean>  -- generic-constraint-mismatch
local b = {}
```

### `keyof` constraints and indexed access types {#keyof-constraints}

The `keyof T` constraint restricts a type parameter to the field names of another type. Combined with `T[K]` (indexed access), this lets you write functions where both the key and the return type are validated:

```lua
---@class Config
---@field name string
---@field value number
---@field enabled boolean

---@generic T, K: keyof T
---@param obj T
---@param key K
---@return T[K]
local function getField(obj, key)
    return obj[key]
end

---@type Config
local cfg = { name = "test", value = 42, enabled = true }

local n = getField(cfg, "name")    -- n: string
local v = getField(cfg, "value")   -- v: number
local e = getField(cfg, "enabled") -- e: boolean

getField(cfg, "bogus")  -- generic-constraint-mismatch: "bogus" is not a field of Config
```

The LS also provides string literal completions for keyof-constrained parameters — typing `getField(cfg, "")` will suggest `enabled`, `name`, `value`.

This pattern is useful for WoW addon code that needs to safely access fields by name:

```lua
---@generic T, K: keyof T
---@param obj T
---@param method K
---@param ... any
function CallMethod(obj, method, ...)
    obj[method](obj, ...)
end
```

#### `keyof self`

Inside a method, `keyof self` resolves to the field names of the call's receiver. This avoids declaring a separate generic for the receiver type when the only thing you need is its key set:

```lua
---@class Widget
local Widget = {}

function Widget:Show() end
function Widget:Hide() end

---@generic K: keyof self
---@param method K
function Widget:Dispatch(method)
    self[method](self)
end

---@type Widget
local w = {}

w:Dispatch("Show")   -- ok
w:Dispatch("Hide")   -- ok
w:Dispatch("Nope")   -- generic-constraint-mismatch: "Nope" is not a method of Widget
```

For subclasses, the receiver's full surface (own + inherited methods) satisfies the constraint, and completions, references, and rename all see the resolved set. `keyof self` only fires for method calls (`:` syntax) — a direct function call where `self` is passed explicitly won't enforce the constraint.

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

### Projections in inline `fun()` return types

You can also use `params<F>` and `returns<F>` inside inline `fun()` type expressions. This is useful for function wrappers that transform signatures:

```lua
---@generic F
---@param func F
---@return fun(...: params<F>): string
local function wrapToString(func)
    return function(...)
        func(...)
        return ""
    end
end

---@param a number
---@param b string
---@return boolean
local function original(a, b) return true end

local wrapped = wrapToString(original)
-- wrapped is: fun(a: number, b: string): string
```

```lua
---@generic F
---@param func F
---@return fun(key: string): returns<F>
local function wrapKeyed(func)
    return function(key) return func(key) end
end
```

## Variadic generics {#variadic-generics}

A variadic generic parameter collects any number of excess arguments into an intersection type. Declare one with `...` prefix:

```lua
---@generic T, ...M
---@param object T
---@param ... any
---@return T & ...M
function Mixin(object, ...) end
```

The first argument binds `T`. All remaining arguments bind `...M` as an intersection:

```lua
---@class Draggable
---@class Resizable
---@class Scrollable

---@type Frame
local f = {}

local result = Mixin(f, Draggable, Resizable, Scrollable)
-- result: Frame & Draggable & Resizable & Scrollable
```

There's no limit on the number of arguments — they all flow into the intersection.

A variadic generic can also be the only generic parameter:

```lua
---@generic ...M
---@param ... any
---@return ...M
function CreateFromMixins(...) end

local obj = CreateFromMixins(Draggable, Resizable)
-- obj: Draggable & Resizable
```

When no excess arguments are provided, the variadic generic stays unbound and is filtered out, leaving just the non-variadic parts of the return type.

## How inference works

The LS infers generic bindings from multiple sources:

1. **Direct argument types** — if `@param x T`, and you pass a `string`, T = string
2. **Structural matching** — `T[]` extracts T from an array's element type; `table<K,V>` extracts from map types
3. **Backtick resolution** — `` `T` `` resolves a string literal as a class name
4. **Function-type extraction** — `fun(): T` extracts T from a callback's return type
5. **Receiver type_args** — for `@class Foo<T>`, calling a method on `---@type Foo<string>` binds T from the receiver

Inference runs per-call and doesn't persist — each call site resolves its own bindings independently.