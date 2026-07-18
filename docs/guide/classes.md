# Classes and Inheritance

Lua doesn't have classes, but WoW addons use them everywhere - via metatables, factory libraries, or just convention. wowlua-ls gives you a way to tell it about your class structures so it can provide completion, type checking, and cross-file intelligence.

## Defining a class

Use `@class` to declare a named type with fields:

```lua
---@class AuctionEntry
---@field itemId number
---@field buyout number
---@field seller string
---@field duration number?
```

This creates a type called `AuctionEntry` that you can reference anywhere:

```lua
---@param entry AuctionEntry
function displayEntry(entry)
    print(entry.seller)   -- completion works, type is string
    print(entry.duration) -- number | nil (the ? makes it optional)
end
```

### Attaching to a variable

Usually you'll attach the class to a local that serves as the class table:

```lua
---@class AuctionEntry
---@field itemId number
---@field buyout number
---@field seller string
local AuctionEntry = {}
```

Now `AuctionEntry` is both a value (the table) and a type. Methods defined on it with colon syntax are automatically part of the class:

```lua
function AuctionEntry:GetDisplayPrice()
    return self.buyout -- self is typed as AuctionEntry
end
```

### Fields from assignments

You don't have to declare every field up front. When you assign to `self.field` inside a method, the LS discovers it:

```lua
function AuctionEntry:Init(data)
    self.itemId = data.itemId
    self.buyout = data.buyout
    self.seller = data.seller
end
```

The LS picks up `itemId`, `buyout`, and `seller` as fields. But explicit `@field` annotations are better because they:
- Document the type (the LS might infer `any` from an ambiguous RHS)
- Show up in completion before you've called `Init`
- Enable diagnostics like `undefined-field` and `missing-fields`

::: tip Start with `@field`, fill in as you go
You don't need to annotate everything at once. Add `@field` for your core data, and let the LS discover the rest from assignments. Over time, promote discovered fields to explicit `@field` declarations as your type coverage improves.
:::

## Field visibility

Fields have three visibility levels:

```lua
---@class PlayerCache
---@field name string                -- public (default)
---@field protected _entries table   -- protected: class + subclasses
---@field private _lock boolean      -- private: this class only
```

- **public**: accessible from anywhere (the default)
- **protected**: accessible from the class and its subclasses
- **private**: accessible only within the class itself

### Implicit protected for `_` prefixes

If your project follows the `_`-prefix convention for internal fields, you can opt in to implicit protected visibility:

```json
{
  "inference": {
    "implicitProtectedPrefix": true
  }
}
```

With this enabled, data fields starting with `_` are **implicitly protected** without needing the keyword:

```lua
---@class PlayerCache
---@field _entries table   -- implicitly protected (starts with _)
---@field public _id number -- explicit public overrides the convention
```

This only applies to data fields discovered at runtime (assignments, constructor fields), not to explicit `@field` declarations without a visibility keyword; those default to public since the author had the chance to write `protected`.

Methods are **not** affected by the `_` convention. A method named `_helper` stays public. Use `@private` or `@protected` explicitly for methods.

### Accessor visibility (`@accessor`)

Some addons group methods under a sub-table to signal visibility - for example, `function MyClass.__p:DoSomething()` where `__p` is a private accessor. The `@accessor` annotation tells the LS that methods defined through that sub-table should inherit its visibility:

```lua
---@class MyClass
---@accessor __p private
---@accessor __pt protected
local MyClass = {}

function MyClass.__p:InternalUpdate()
    -- This method is private (from __p's visibility)
end

function MyClass.__pt:SharedHelper()
    -- This method is protected (from __pt's visibility)
end

function MyClass:PublicMethod()
    -- This method is public (no accessor)
end
```

The accessor name (`__p`, `__pt`) is transparent: the methods are placed directly on the class, not on a sub-table. The accessor only controls visibility. Calling `obj:InternalUpdate()` from outside the class triggers an `access-private` diagnostic.

## Inheritance

Classes can extend other classes:

```lua
---@class Animal
---@field name string
---@field sound string

---@class Dog : Animal
---@field breed string
```

`Dog` inherits all of `Animal`'s fields. You can access `name` and `sound` on any `Dog`:

```lua
---@param dog Dog
function describe(dog)
    print(dog.name)   -- string (inherited from Animal)
    print(dog.breed)  -- string (own field)
    print(dog.sound)  -- string (inherited from Animal)
end
```

### Multiple parents {#multiple-parents}

A class can inherit from multiple parents. Use commas or `&`; both are equivalent:

```lua
---@class CellMixin
---@field cellWidth number

---@class TooltipMixin
---@field tooltipText string

---@class MyCellTemplate : CellMixin, TooltipMixin
---@field label string
```

Or with `&`, which reads naturally when the parents are mixins:

```lua
---@class MyCellTemplate : CellMixin & TooltipMixin
---@field label string
```

`MyCellTemplate` inherits `cellWidth` from `CellMixin` and `tooltipText` from `TooltipMixin`:

```lua
---@type MyCellTemplate
local cell = {}
cell.cellWidth    -- number (from CellMixin)
cell.tooltipText  -- string (from TooltipMixin)
cell.label        -- string (own field)
```

You can mix the two syntaxes and combine with `table<K,V>`:

```lua
---@class TaggedMixin : CellMixin & TooltipMixin, table<string, number>
```

### Dictionary classes (`table<K,V>` parent)

A class can inherit from `table<K,V>` to combine a named class type with dictionary key/value types. This gives `pairs()` loops typed keys and values:

```lua
---@class ColorMap : table<string, string>
---@field default string

---@type ColorMap
local colors = { Red = "#FF0000", Blue = "#0000FF" }

for name, hex in pairs(colors) do
    -- name: string, hex: string
end

local d = colors.default  -- string (named field)
```

You can combine this with regular class inheritance:

```lua
---@class Base
---@field id number

---@class TaggedScores : Base, table<string, number>
```

`TaggedScores` inherits `id` from `Base` and has typed `string` keys / `number` values.

### Deep inheritance

Inheritance chains work to arbitrary depth:

```lua
---@class Base
---@field id number

---@class Middle : Base
---@field category string

---@class Leaf : Middle
---@field value number
```

`Leaf` has `id`, `category`, and `value`. The LS resolves the full chain.

### Protected access in subclasses

Protected fields are accessible in subclasses:

```lua
---@class Base
---@field protected _data table

---@class Child : Base

function Child:Process()
    self._data = {} -- OK, Child extends Base
end
```

But not from outside the hierarchy:

```lua
---@param base Base
function external(base)
    base._data = {} -- warning: access-protected
end
```

## Mixins and templates {#mixins-and-templates}

WoW uses mixins extensively. `Mixin()` copies fields from one or more tables onto a target, and frame templates apply mixin behaviors to frames. In the type system, this is an **intersection type** (`A & B`): a value that has the fields and methods of both types.

### Frame templates

When you call `CreateFrame` with a template, the return type is automatically an intersection of the frame type and the template mixin:

```lua
local frame = CreateFrame("Frame", nil, nil, "BackdropTemplate")
-- frame: Frame & BackdropTemplate

frame:SetPoint("CENTER")  -- Frame method
frame:SetBackdrop({})     -- BackdropTemplate method
```

No annotation needed; the `CreateFrame` stub handles this.

### Mixin(), CreateFromMixins(), CreateAndInitFromMixin()

WoW's mixin functions are fully typed. They use variadic generics to support any number of mixins:

```lua
local frame = Mixin(CreateFrame("Frame"), DraggableMixin, TooltipMixin, ScrollableMixin)
-- frame: Frame & DraggableMixin & TooltipMixin & ScrollableMixin
```

`Mixin()` also supports bare calls via `@narrows-arg`: the first argument's type is updated in-place:

```lua
---@type Frame
local frame = CreateFrame("Frame")
Mixin(frame, DraggableMixin)
-- frame is now Frame & DraggableMixin
frame:StartDragging() -- works
```

`CreateFromMixins()` creates a new object from mixins:

```lua
local obj = CreateFromMixins(DraggableMixin, TooltipMixin)
-- obj: DraggableMixin & TooltipMixin
```

### Annotating mixin parameters

When a function expects a frame with a specific mixin applied, use `&`:

```lua
---@param frame Frame & BackdropTemplate
function configureBackdrop(frame)
    frame:SetBackdrop({ bgFile = "Interface\\Tooltips\\UI-Tooltip-Background" })
    frame:SetBackdropColor(0, 0, 0, 0.8)
end
```

This also works with multiple mixins:

```lua
---@param frame Frame & BackdropTemplate & UIDropDownMenuTemplate
function setupDropdown(frame) end
```

`&` binds tighter than `|`, so `Frame | Button & BackdropTemplate` means `Frame | (Button & BackdropTemplate)`.

### Defining mixin types

If your addon defines its own mixins, declare them as `@class` types:

```lua
---@class DraggableMixin
---@field isDragging boolean

---@return nil
function DraggableMixin:StartDragging() end

---@return nil
function DraggableMixin:StopDragging() end
```

Then reference them in intersections wherever the mixin is applied:

```lua
---@param frame Frame & DraggableMixin
function makeDraggable(frame)
    frame:StartDragging() -- mixin method
    frame:SetMovable(true) -- Frame method
end
```

## Partial classes

The `(partial)` and `(exact)` modifiers are accepted by the parser for compatibility, but are currently ignored: they have no effect on diagnostics. This means code using `@class (partial)` won't cause parse errors, but the class is still treated as exact.

```lua
---@class (partial) AddonState  -- parsed, but treated the same as @class AddonState
---@field version number
```

## Constructors and `missing-fields`

When you construct a class instance via a table literal, the LS checks that all required fields are present:

```lua
---@class Config
---@field name string
---@field debug boolean
---@field timeout number?

---@type Config
local cfg = {
    name = "MyAddon",
    -- warning: missing-fields: 'debug' is required
}
```

Optional fields (those with `?` or `nil` in their type) don't trigger the warning.

## Enum types (`@enum`)

Use `@enum` instead of `@class` to declare an enum type: a named table whose values are bidirectionally compatible with their value type (`number` or `string`):

```lua
---@enum Priority
local Priority = {
    Low = 1,
    Medium = 2,
    High = 3,
}

---@param p Priority
function setPriority(p) end

setPriority(Priority.High) -- OK
setPriority(2)             -- OK, enums accept plain numbers
setPriority("high")        -- warning: type-mismatch
```

String-valued enums work the same way: values are interchangeable with `string`:

```lua
---@enum Status
local Status = {
    Active = "active",
    Inactive = "inactive",
    Pending = "pending",
}

---@param s Status
function setStatus(s) end

setStatus(Status.Active) -- OK
setStatus("custom")      -- OK, string enums accept plain strings
setStatus(42)            -- warning: type-mismatch
```

The enum's value type is inferred automatically from the field values. All values must be the same type; mixing numbers and strings in the same enum produces a `mixed-enum-values` warning.

### Key-based enums (`@enum (key)`)

By default, `@enum` creates a type from the table's **values**. Use `@enum (key)` to create an enum from the table's **keys** instead, useful when a table's keys represent a fixed set of valid string identifiers:

```lua
---@enum (key) Settings
local DEFAULTS = { showTooltip = true, maxRetries = 5, prefix = "My" }

---@param setting Settings
---@return any
function getSetting(setting)
    return DEFAULTS[setting]
end

getSetting("showTooltip") -- OK
getSetting("unknown")     -- OK (string-compatible, like all string enums)
getSetting(42)            -- warning: type-mismatch
```

Key enums are always string enums (since Lua table constructor keys are identifiers). The `mixed-enum-values` diagnostic does not apply to key enums; their values can be any type.

WoW's built-in `Enum.*` types (like `Enum.PowerType`, `Enum.UnitSex`) are automatically treated as number enums, so `UnitPower("player", 0)` doesn't produce a type-mismatch warning.

## `@class` with metatable patterns

The most common WoW addon class pattern combines `@class` with metatables:

```lua
---@class Tooltip
---@field lines string[]
---@field maxWidth number
local Tooltip = {}
Tooltip.__index = Tooltip

---@return Tooltip
function Tooltip:New()
    return setmetatable({
        lines = {},
        maxWidth = 200,
    }, self)
end

function Tooltip:AddLine(text)
    table.insert(self.lines, text)
end

function Tooltip:Show()
    -- self.lines, self.maxWidth are typed
end
```

The LS understands that `setmetatable({}, self)` creates an instance of `Tooltip` through the `__index` chain. The `@return Tooltip` on `New` makes it explicit for callers:

```lua
local tip = Tooltip:New()
tip:AddLine("Hello")  -- completion works
tip:Show()            -- type checked
tip.maxWidth          -- number
```

## Class factory pattern (`@defclass`)

Many WoW addons use a factory function to create classes:

```lua
local Dog = MyLib:NewClass("Dog")
function Dog:Bark() end
```

The `@defclass` annotation tells the LS that a function creates classes:

```lua
---@generic T: BaseClass
---@defclass T
---@param name `T`
---@return T
function MyLib:NewClass(name) return {} end
```

Now every call to `NewClass` creates a properly typed class that inherits from `BaseClass`. The backtick `` `T` `` means "resolve the string argument as a class name."

With parameterized parents for deep hierarchies:

```lua
---@class BaseClass<S>
---@field __super S

---@generic T: BaseClass<P>
---@generic P: BaseClass
---@defclass T : P
---@param name `T`
---@param parent? P
---@return T
function MyLib:NewClass(name, parent) return {} end

local Animal = MyLib:NewClass("Animal")
local Dog = MyLib:NewClass("Dog", Animal)
Dog.__super -- typed as Animal
```