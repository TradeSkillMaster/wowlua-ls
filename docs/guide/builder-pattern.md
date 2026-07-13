# Builder Pattern

Some WoW addons define data schemas through method-chaining builders: each call adds a typed field, and a final call returns the accumulated type. wowlua-ls tracks these chains and gives you full completion and type checking on the result.

## Basic builder

Three annotations power the builder pattern:

- `@builds-field <param_idx> <type>`: this method adds a field whose name is the string at parameter position `param_idx`
- `@return self`: the method returns the same receiver (for chaining)
- `@return built`: returns the accumulated type with all added fields

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

---@return built
function Schema:Build() return {} end
```

Now chain them:

```lua
local inst = Schema:AddString("label"):AddNumber("count"):Build()
inst.label -- string
inst.count -- number?
```

Each `@builds-field` call adds a field. `@return self` propagates the growing type through the chain. `@return built` returns the final result with all accumulated fields.

## Naming built types (`@built-name`)

By default, the built type inherits the schema's class name. Use `@built-name` to give it a custom name from a string argument:

```lua
---@built-name 1
---@return self
function Schema.Create(name) return Schema end
```

```lua
local MY_SCHEMA = Schema.Create("PlayerState")
    :AddString("name")
    :AddNumber("level")

local state = MY_SCHEMA:Build()
-- state has type PlayerState { name: string, level: number? }
```

The name `PlayerState` is registered globally. You can reference it in `@param` and `@type` annotations across files:

```lua
---@param state PlayerState
function process(state)
    print(state.name) -- completion works
end
```

## Extending schemas (`@built-extends`)

Use `@built-extends` with `@built-name` to create a new type that inherits from the receiver's current built type:

```lua
---@param name string
---@built-name 1
---@built-extends
---@return self
function Schema:Extend(name) return self end
```

```lua
local BASE = Schema.Create("BaseState")
    :AddString("name")
    :AddNumber("level")

local CHILD = BASE:Extend("ChildState")
    :AddString("childField")

local inst = CHILD:Build()
inst.childField -- string (own field)
inst.name       -- string (inherited from BaseState)
inst.level      -- number (inherited from BaseState)
```

Multi-level extension works. Grandchild inherits from child and base.

## Lateinit fields (`T!`)

Builder fields support the `!` suffix for lateinit semantics:

```lua
---@generic T
---@param name string
---@param class T|`T`
---@builds-field 1 T!
---@return self
function Schema:AddDeferred(name, class) return self end
```

Fields created with `T!` allow nil assignment without `field-type-mismatch` but hover as non-nil. This is useful for object pools or lazy initialization patterns.

## `@class` overlays

A `@class` declaration with the same name as a `@built-name` type merges its `@field` annotations with the builder fields. Overlay fields take precedence:

```lua
---@class PlayerState
---@field name string
---@field readonly computed string
```

If `PlayerState` is also created via `@built-name`, the `@field` declarations overlay the built fields. This lets you declare computed or virtual fields that aren't part of the builder chain.

## Inheriting from a parent class

`@return built : ParentClass` adds a parent class to the built type:

```lua
---@class State
---@field GetValue fun(self, key: string): any

---@return built : State
function Schema:CreateState() return {} end

local state = Schema:AddString("name"):CreateState()
state.name       -- string (from builder)
state:GetValue() -- inherited from State
```