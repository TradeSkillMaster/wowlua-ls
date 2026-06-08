# All Annotations

Quick reference for every annotation wowlua-ls supports. For detailed usage and examples, see the [guide](/guide/basic-annotations).

## Type annotations

| Annotation | Description | Guide |
|---|---|---|
| `@param name type` | Parameter type. `name?` for optional. | [Basic Annotations](/guide/basic-annotations) |
| `@return type [name]` | Return type. Multiple lines for multi-return. | [Basic Annotations](/guide/basic-annotations) |
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
| `@correlated f1, f2, ...` | Fields or locals that are always nil/non-nil together. | [Nil Safety](/guide/nil-safety) |

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
| `@narrows-arg N` | Bare call narrows the Nth argument's type to the return type. | [Type Guards](/guide/type-guards#narrows-arg) |
| `@flavor-narrows flavor` | Flavor guard function or boolean. | [Flavor Filtering](/guide/flavor-filtering) |

## Metadata annotations

| Annotation | Description |
|---|---|
| `@alias Name type` | Type alias. Supports parameters: `@alias Name<K,V> V[]`. Use `@alias (opaque) Name type` for a nominally distinct type (see below). |
| `@deprecated` | Mark as deprecated. |
| `@nodiscard` | Warn if return value is ignored. |
| `@meta` | Declaration-only file (suppresses all diagnostics). |
| `@diagnostic disable:code` | Suppress a diagnostic inline. |
| `@see symbol` | Cross-reference shown in hover. |
| `@constructor` | Mark a method as the class constructor. |
| `@accessor name [visibility]` | Set visibility for methods defined through a sub-table accessor. [Guide](/guide/classes#accessor-visibility-accessor) |

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
| `T?` | Optional (`T \| nil`) |
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
