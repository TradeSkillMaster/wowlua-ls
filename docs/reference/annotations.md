# All Annotations

Quick reference for every annotation wowlua-ls supports. For detailed usage and examples, see the [guide](/guide/basic-annotations).

## Type annotations

| Annotation | Description | Guide |
|---|---|---|
| `@param name type` | Parameter type. `name?` for optional. | [Basic Annotations](/guide/basic-annotations) |
| `@return type [name]` | Return type. Use multiple lines or one comma-separated line (`@return A, B`) for multi-return. | [Basic Annotations](/guide/basic-annotations) |
| `@return (A, B) \| (C, D)` | Tuple-union return with correlated narrowing. | [Multi-Return](/guide/multi-return) |
| `@return ...T` | Variadic return — fills remaining positions with T. | [Multi-Return](/guide/multi-return) |
| `@type type` | Variable type annotation. | [Basic Annotations](/guide/basic-annotations) |
| `@as type` | Inline expression type assertion (`--[[@as T]]`). | [Basic Annotations](/guide/basic-annotations) |
| `@cast var [+\|-]type` | Change variable type: replace, add (`+`), remove (`-`). | [Basic Annotations](/guide/basic-annotations) |

## Class and type annotations

| Annotation | Description | Guide |
|---|---|---|
| `@class Name` | Define a named class type. | [Classes](/guide/classes) |
| `@class Name : Parent` | Class with inheritance. | [Classes](/guide/classes) |
| `@class Name : A, B` | Multiple parent classes (comma-separated). | [Classes](/guide/classes#multiple-parents) |
| `@class Name : A & B` | Multiple parent classes (intersection syntax). | [Classes](/guide/classes#multiple-parents) |
| `@class Name : table<K, V>` | Class with dictionary key/value types. | [Classes](/guide/classes) |
| `@class (partial) Name` | Accepted for compatibility (currently ignored). | [Classes](/guide/classes#partial-classes) |
| `@class Name<T>` | Parameterized class. | [Generics](/guide/generics) |
| `@class Name<T: Constraint>` | Parameterized class with type constraint. | [Generics](/guide/generics) |
| `@enum Name` | Enum type — bidirectionally compatible with `number` or `string` (inferred from values). | [Classes](/guide/classes#enum-types-enum) |
| `@enum (key) Name` | Key-based enum — creates a string enum from table keys instead of values. | [Classes](/guide/classes#key-based-enums-enum-key) |
| `@event TypeName "EVENT_NAME"` | Declare an event with typed payload (hover + handler param narrowing). | [Events](/guide/events) |
| `@event TypeName` + `---\|` | Batch event declarations with inline params. | [Events](/guide/events#batch-declarations-with) |
| `@field name type` | Class field declaration. | [Classes](/guide/classes) |
| `@field [K] V` | Bracket-index field. | [Generics](/guide/generics) |
| `@field private name type` | Private field. | [Classes](/guide/classes) |
| `@field protected name type` | Protected field. | [Classes](/guide/classes) |
| `@shape {fields}` | Plain-table form(s) accepted where this class is expected (userdata/mixin escape). | [#shape](#shape-accept-plain-table-forms) |
| `@correlated f1, f2, ...` | Fields or locals that are always nil/non-nil together. | [Nil Safety](/guide/nil-safety) |

### `@shape` — accept plain-table forms

Some classes are runtime objects (userdata, mixins) with both data fields **and**
methods, yet are routinely created by passing a plain table that carries only the
data fields. World of Warcraft's `ItemLocation` is constructed with
`ItemLocation:CreateFromBagAndSlot(bag, slot)`, but addons just pass
`C_Item.IsLocked({ bagID = 0, slotIndex = 1 })`; likewise a `ColorMixin` parameter
commonly receives `{ r = 1, g = 0, b = 0, a = 1 }`. The strict structural check
would reject these for the missing mixin methods.

`@shape` declares the plain-table form(s) a class accepts. A table matching any
declared shape is assignable to the class even though it lacks the methods:

```lua
---@class ItemLocation
---@shape { bagID: number, slotIndex: number } | { equipmentSlotIndex: number }
---@field bagID number
---@field slotIndex number
---@field IsValid fun(self: ItemLocation): boolean
```

- The shape is a normal [type expression](#type-syntax). Use a **union of table
  literals** for mutually-exclusive construction variants (bag+slot *or* equipment
  slot, above). Optional fields use `field?: type`.
- Methods on the class are **not** required by the shape — that's the point.
- Once a class declares `@shape`, the shapes are the **complete** plain-table
  input spec: it is matched against plain tables *only* by its shapes, never by
  the generic structural-field check. A table that matches **no** shape still
  mismatches, so unrelated tables and wrong-typed fields are caught (no hole).
- A string-keyed dict (`table<"r"|"g"|"b", number>`) is accepted when its keys
  cover the shape's required fields — handy for config-derived colors.

The class keeps its full field/method set for member access, hover, and
completion; `@shape` only widens what is *assignable* to it.

**The shape drives read-side nilability.** A field named in the shape but not
required in *every* member is conditionally present, so it reads as nilable on a
value typed as the class — e.g. an `ItemLocation` built from bag+slot has
`equipmentSlotIndex == nil`, so `loc.equipmentSlotIndex` is `number?`. This keeps
the field types honest without re-declaring the (often vendor-authored) `@field`s.

**Standalone form (additive).** `@shape <ClassName> <type>` attaches a shape to an
existing class by name, without re-declaring `@class`. This is how API stubs add a
shape to a generated class without replacing it:

```lua
---@meta _
---@shape ItemLocation { bagID: number, slotIndex: number } | { equipmentSlotIndex: number }
---@shape ColorMixin   { r: number, g: number, b: number, a?: number }
```

## Generic annotations

| Annotation | Description | Guide |
|---|---|---|
| `@generic T` | Generic type parameter on a function. | [Generics](/guide/generics) |
| `@generic T: Class` | Constrained generic. | [Generics](/guide/generics) |
| `@generic T, K: keyof T` | Key-constrained generic — K must be a field name of T. | [Generics](/guide/generics#keyof-constraints) |
| `@generic K: keyof self` | Method receiver key constraint — K must be a field name of the call's receiver. | [Generics](/guide/generics#keyof-constraints) |
| `@generic T, ...M` | Variadic generic — collects excess arguments into an intersection. | [Generics](/guide/generics#variadic-generics) |
| `@requires T: Constraint` | Method is only callable when the receiver's class type parameter `T` satisfies the constraint. | [Generics](/guide/generics) |
| `` @param name `T` `` | Resolve string argument as a class name. | [Generics](/guide/generics) |
| `@overload fun(...)` | Function overload signature. | [Generics](/guide/generics) |

## Factory and builder annotations

| Annotation | Description | Guide |
|---|---|---|
| `@defclass T` | Class factory function. | [Classes](/guide/classes) |
| `@defclass T : P` | Class factory with parent parameter. | [Classes](/guide/classes) |
| `@builds-field idx type` | Builder method adds a field. | [Builder Pattern](/guide/builder-pattern) |
| `@return built` | Return the accumulated built type. | [Builder Pattern](/guide/builder-pattern) |
| `@return built : Parent` | Built type with parent class. | [Builder Pattern](/guide/builder-pattern) |
| `@built-name idx` | Name the built type from a string argument. | [Builder Pattern](/guide/builder-pattern) |
| `@built-extends` | Built type inherits from receiver's built type. | [Builder Pattern](/guide/builder-pattern) |
| `@return self` | Method returns the receiver (for chaining). | [Builder Pattern](/guide/builder-pattern) |
| `@return self<X>` | Method returns the receiver re-parameterized with type argument `X`. | [Builder Pattern](/guide/builder-pattern) |

## Narrowing and guard annotations

| Annotation | Description | Guide |
|---|---|---|
| `@type-narrows target class` | Type guard function (index-based). | [Type Guards](/guide/type-guards) |
| `@type-narrows ClassName` | Type guard method (narrows self). | [Type Guards](/guide/type-guards) |
| `@returns-class-name` | Method whose string return value names the receiver's class; `recv:m() == "Class"` narrows `recv` to `Class`. | [Type Guards](/guide/type-guards#returns-class-name) |
| `@narrows-arg N` | Bare call narrows the Nth argument's type to the return type. | [Type Guards](/guide/type-guards#narrows-arg) |
| `@flavor-narrows flavor` | Flavor guard function or boolean. | [Flavor Filtering](/guide/flavor-filtering) |

## Metadata annotations

| Annotation | Description |
|---|---|
| `@alias Name type` | Type alias. Supports parameters: `@alias Name<K,V> V[]`, including constrained parameters: `@alias Box<T: Frame> { value: T }`. Use `@alias (opaque) Name type` for a nominally distinct type (see below). |
| `@deprecated` | Mark as deprecated. |
| `@nodiscard` | Warn if return value is ignored. |
| `@meta` | Declaration-only file (suppresses all diagnostics). |
| `@diagnostic disable:code` | Suppress a diagnostic inline. |
| `@see symbol` | Cross-reference shown in hover. |
| `@constructor` | Mark a method as the class constructor. |
| `@accessor name [visibility]` | Set visibility for methods defined through a sub-table accessor. [Guide](/guide/classes#accessor-visibility-accessor) |
| `@creates-global N` | Calling this function with a string literal at param `N` creates a named global. The global's type is taken from the call's return type. |
| `@generates-events N [Field]` | Calling this method with an array table at param `N` synthesizes an enum-like `Field` table (default `Event`) on the receiver class, one member per array entry. |
| `@callback-event-arg N` | Marks a callback-registry consumer method (`RegisterCallback`/`TriggerEvent`/…) whose argument `N` is an event name — enables event-name completion and the `unknown-callback-event` diagnostic. |

### `@creates-global N`

Some functions create a global as a side effect of being called — for example
World of Warcraft's `CreateFrame("Frame", "MyFrame")` defines `_G.MyFrame`. Mark
such a function so that reading the created name in another file does not produce
a false [`undefined-global`](/reference/diagnostics) diagnostic:

- `N` (1-based) is the parameter whose **string-literal** argument names the
  created global.

The created global's **type is the call's actual return type** — you don't
specify it. This means a call carrying a template/mixin gets the full type: a
`CreateFrame("Frame", "MyFrame", parent, "MyTemplate")` global is typed
`Frame & MyTemplate`, not a bare `Frame`.

```lua
---@param frameType FrameType
---@param name? string
---@return T frame
---@creates-global 2   -- param 2 names the global; its type is the return type
function CreateFrame(frameType, name, ...) end

---@param name string
---@return Font
---@creates-global 1   -- param 1 names the global; it is a Font (from @return)
function CreateFont(name) end
```

Only string-literal arguments are detected; dynamic names (e.g.
`CreateFrame("Frame", varName)`) are not registered.

::: info
Currently, only functions defined in API stubs are detected as `@creates-global`
sources. The annotation is parsed on workspace-defined functions but their calls
are not yet scanned for created globals.
:::

### `@generates-events N [Field]`

Some methods populate an enum-like table on their receiver as a side effect of
being called. World of Warcraft's
`CallbackRegistryMixin:GenerateCallbackEvents({ "OnFoo", ... })` builds
`self.Event = { OnFoo = "OnFoo", ... }`, which addons later reference as
`Mixin.Event.OnFoo`. Mark such a method so those accesses resolve instead of
producing a false [`undefined-field`](/reference/diagnostics):

- `N` (1-based) is the call argument that holds the **array table** of event
  names.
- `Field` (optional, default `Event`) is the table field synthesized on the
  receiver class.

```lua
---@generates-events 1 Event   -- arg 1 is the event array; build `self.Event`
---@param events string[]
function CallbackRegistryMixin:GenerateCallbackEvents(events) end
```

The receiver must be a single-name class (e.g. `ScrollBoxListMixin`, not a dotted
chain). Each array entry contributes one `string` member: string literals use
their value, and field references (`SomeEvents.OnFoo`) use the leaf name `OnFoo`,
matching the value-equals-name convention. Accessing an event that isn't in the
array still resolves leniently — the synthesized table is not closed.

```lua
ScrollBoxListMixin:GenerateCallbackEvents({
    BaseScrollBoxEvents.OnScroll,        -- → ScrollBoxListMixin.Event.OnScroll
    "OnDataProviderReassigned",          -- → ScrollBoxListMixin.Event.OnDataProviderReassigned
})

local e = ScrollBoxListMixin.Event.OnDataProviderReassigned  -- string, no diagnostic
```

### `@callback-event-arg N`

Addons that mix in `CallbackRegistryMixin` usually register and fire events by
string-literal name rather than through the `.Event` table:

```lua
addonTable.CallbackRegistry:RegisterCallback("SettingChanged", handler)
addonTable.CallbackRegistry:TriggerEvent("SettingChanged", value)
```

`@callback-event-arg N` marks a consumer method whose `N`-th argument is such an
event name. Combined with the registry's declared events (from
`GenerateCallbackEvents`, including an `addonTable.Constants.Events`-style string
array resolved across files), the language server then:

- **completes** the registered event names inside the string argument, and
- flags an unregistered name with
  [`unknown-callback-event`](/reference/diagnostics) (off by default).

```lua
---@callback-event-arg 1   -- arg 1 is the event name
---@param event string
function CallbackRegistryMixin:RegisterCallback(event, func, owner, ...) end
```

The registry receiver is matched by name (a global, a class, or an
`addonTable.X` namespace field — the addon-namespace alias is normalized so the
declaration and the call sites agree across files, and scoped by addon so separate
addons in one workspace don't share an event set). When a registry's event set
can't be fully determined, validation is suppressed for it so no false positives
are reported.

## Opaque aliases

`@alias (opaque)` creates a nominally distinct type that prevents accidental mixing of values that share the same underlying type:

```lua
---@alias (opaque) PlayerID number
---@alias (opaque) ItemID number

---@param id PlayerID
local function lookupPlayer(id) end

lookupPlayer(42)            -- OK: number literal matches inner type
lookupPlayer(getItemID())   -- ERROR: ItemID is not PlayerID
```

**Rules:**
- Literal values and base-type values are accepted where an opaque alias is expected (e.g. `42` passes as `PlayerID`)
- An opaque alias flows out to its base type freely (e.g. `PlayerID` passes where `number` is expected)
- Different opaque aliases with the same inner type are **not** interchangeable (`ItemID` cannot be used as `PlayerID`)
- Arithmetic and other operators unwrap to the inner type; results decay to the base type (`PlayerID + 1` produces `number`)
- Hover displays the alias name, not the inner type

Works with any inner type including string literal unions:

```lua
---@alias (opaque) Answer "YES"|"NO"
---@alias (opaque) Toggle "YES"|"NO"

---@param a Answer
local function process(a) end

process("YES")          -- OK
process(getToggle())    -- ERROR: Toggle is not Answer
```

## Type syntax

| Syntax | Meaning |
|---|---|
| `string`, `number`, `boolean`, `nil`, `any` | Primitives |
| `integer` | Integer subtype of number |
| `table` | Any table |
| `function` | Any function |
| `A \| B` | Union |
| `A & B` | Intersection |
| `T[]` | Array |
| `T[K]` | Indexed access — field type of K on T |
| `[T1, T2]` | Tuple — fixed-shape table (`{ [1]: T1, [2]: T2 }`) |
| `T?` | Optional (`T \| nil`) |
| `?T` | Optional, prefix form — same as `T?` |
| `T!` | Non-nil / lateinit |
| `table<K, V>` | Map type |
| `fun(a: T): R` | Function type |
| `{f: T, g?: U}` | Anonymous table shape |
| `"literal"` | String literal type |
| `true`, `false` | Boolean literal types |
| `0`, `-1`, `0xFF` | Number literal types (e.g. a `\| (0, nil, nil)` tuple-union case) |
| `params<F>` | Function parameter projection (vararg only) |
| `params<EventType>` | Event payload projection — types varargs per-event |
| `returns<F>` | Function return type projection |
| `expression<C>` | Expression string type — fields of class C become variables |
| `expression<C, R>` | Expression string with return type constraint R |
| `expression<C, R>` (R is `@generic`) | Result type R inferred from the expression and propagated to the return |
| `expression<C & F>` | Expression string with additional functions/fields from F |
| `expression<C & F, R>` | Expression with extra environment and return constraint |
