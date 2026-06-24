---@diagnostic disable: undefined-global
-- Test: structural subtyping — table literals assignable to @class types

---@class ContentLine
---@field label string
---@field content string

---@class Point
---@field x number
---@field y number

---@class OptionalFields
---@field name string
---@field tag? string

---@class ParentShape
---@field color string

---@class ChildShape : ParentShape
---@field sides number

---@param line ContentLine
local function useLine(line)
    return line.label
end

---@param pt Point
local function usePoint(pt)
    return pt.x + pt.y
end

---@param o OptionalFields
local function useOptional(o)
    return o.name
end

---@param s ChildShape
local function useChild(s)
    return s.sides
end

-- Should NOT warn: table literal has all required fields with matching types
useLine({ label = "hello", content = "world" })

-- Should NOT warn: table literal has matching fields
usePoint({ x = 1, y = 2 })

-- Should HINT: extra field not in class definition
useLine({ label = "hello", content = "world", extra = true })
--       ^ diag: inject-field

-- Should NOT warn: optional field omitted
useOptional({ name = "test" })

-- Should NOT warn: optional field provided
useOptional({ name = "test", tag = "v1" })

-- Should WARN: missing required field 'content'. A non-empty literal whose only
-- problem is omitting required fields is owned by the dedicated `missing-fields`
-- diagnostic; the redundant `type-mismatch` is suppressed.
useLine({ label = "hello" })
--       ^ diag: missing-fields ~missing required field 'content'

-- Should WARN: missing required field 'x' (missing-fields, not type-mismatch)
usePoint({ y = 2 })
--        ^ diag: missing-fields ~missing required field 'x'

-- Should WARN: wrong field type (number instead of string). A wrong-typed field
-- is not covered by `missing-fields`, so `type-mismatch` still fires.
useLine({ label = 42, content = "world" })
--       ^ diag: type-mismatch ~wrong type for field: 'label' (expected `string`, got `number`)

-- Should WARN: empty table has no fields to match. `missing-fields` skips empty
-- literals (they read as deferred placeholders), so `type-mismatch` stays the
-- sole signal here and still lists the missing fields.
useLine({})
--      ^ diag: type-mismatch ~missing fields: 'content', 'label'

-- Should WARN: table literal does not have parent's required field 'color'
-- (missing-fields, not type-mismatch)
useChild({ sides = 4 })
--        ^ diag: missing-fields ~missing required field 'color'

-- Should NOT warn: inherited field satisfied, no excess
useChild({ sides = 4, color = "red" })

-- Should HINT: excess field on child class
useChild({ sides = 4, color = "red", weight = 10 })
--        ^ diag: inject-field

-- Excess fields via @type assignment context
---@type ContentLine
local assigned = { label = "hello", content = "world", bonus = 1 }
--               ^ diag: inject-field
useLine(assigned)

-- No excess in @type assignment
---@type ContentLine
local clean = { label = "hello", content = "world" }
useLine(clean)

-- Regression: tinsert with typed array of @class
---@type ContentLine[]
local lines = {}
tinsert(lines, { label = "hello", content = "world" })

-- Nil-valued fields in constructors are placeholders, not type errors
---@class InitContext
---@field path string
---@field ready boolean
---@field callback fun()?

---@type InitContext
local ctx = { path = "test", ready = nil, callback = nil }
_consume(ctx)

-- But non-nil mismatched types should still be caught
---@type InitContext
local ctx2 = { path = "test", ready = "wrong", callback = nil }
--           ^ diag: assign-type-mismatch ~wrong type for field: 'ready' (expected `boolean`, got `string`)
_consume(ctx2)

-- Optional class parameter (`p?`) receiving a partial table literal: the
-- expected type is `Options?` (a `Class | nil` union). A correctly-typed subset
-- still reports `missing-fields` against the class member, and the redundant
-- `type-mismatch` (which previously fired with no explanatory detail because the
-- expected type was a union) is suppressed. Mirrors `LibDBIcon:Register`'s
-- `db?` parameter receiving `{ hide = false }`.
---@class Options
---@field hide boolean
---@field lock boolean

---@param opts? Options
local function useOptions(opts) end

useOptions({ hide = false })
--          ^ diag: missing-fields ~missing required field 'lock'

-- A wrong-typed field on the optional-union parameter still surfaces a
-- `type-mismatch` (missing-fields cannot report wrong types).
useOptions({ hide = 1, lock = true })
--          ^ diag: type-mismatch ~wrong type for field: 'hide' (expected `boolean`, got `number`)

-- Hash-map + array mixed table: table<K,V> with non-number keys is compatible
-- with array parameters because Lua tables have both hash and array parts.
---@class MixedEntry
---@field id number

---@param list MixedEntry[]
local function useArray(list)
    return list[1]
end

---@type table<string,number>|MixedEntry[]
local mixed = {}
useArray(mixed)

-- Standalone table<K,V> without an array member in the union should still warn
---@type table<string,number>
local hashOnly = {}
useArray(hashOnly)
-- ^ diag: type-mismatch

-- table<number,V> should NOT be silently compatible (number keys overlap with array)
---@type table<number,string>
local numKeyed = {}
useArray(numKeyed)
-- ^ diag: type-mismatch

-- Number-keyed hash in a union WITH an array member should still warn
-- (number keys alias array indices even when sibling array type matches)
---@type table<number,string>|MixedEntry[]
local numKeyedUnion = {}
useArray(numKeyedUnion)
-- ^ diag: type-mismatch

-- Array member in union doesn't match expected — both members fail
---@class OtherEntry
---@field name string

---@type table<string,number>|OtherEntry[]
local wrongArray = {}
useArray(wrongArray)
-- ^ diag: type-mismatch

-- Hash value type unrelated to array element type — should still be tolerated
-- (hash entries don't interfere with array access)
---@type table<string,boolean>|MixedEntry[]
local unrelatedHash = {}
useArray(unrelatedHash)

-- Hash-map exemption only applies when expected is array-shaped, not any type
---@param s string
local function useString(s)
    return s
end

---@type table<string,number>|string[]
local mixedStr = {}
useString(mixedStr)
-- ^ diag: type-mismatch

-- Structural match: table<string_literal_keys, V> → @class
-- A dict annotated as table<"x"|"y", number> covers all fields of Point
---@type table<"x"|"y", number>
local dictPoint = {}
usePoint(dictPoint)

-- Should work with a single literal key matching a single-field class
---@class SingleField
---@field x number
---@param sf SingleField
local function useSingle(sf) return sf.x end

---@type table<"x", number>
local single = {}
useSingle(single)

-- Should fail: key set missing required field 'y'
---@type table<"x", number>
local missingY = {}
usePoint(missingY)
-- ^ diag: type-mismatch

-- Should fail: extra keys are fine but value type incompatible (string vs number)
---@type table<"x"|"y", string>
local wrongValType = {}
usePoint(wrongValType)
-- ^ diag: type-mismatch

-- Should work: optional field absent from key set is OK
---@class RGBA
---@field r number
---@field g number
---@field b number
---@field a? number
---@param color RGBA
local function useRGBA(color) return color.r end

---@type table<"r"|"g"|"b", number>
local noAlpha = {}
useRGBA(noAlpha)

-- Should work: all four keys present
---@type table<"r"|"g"|"b"|"a", number>
local fullRGBA = {}
useRGBA(fullRGBA)

-- Non-literal key type should NOT match (no guarantee of named fields)
---@type table<string, number>
local anyStr = {}
usePoint(anyStr)
-- ^ diag: type-mismatch

-- Should fail: inherited required field missing from key set
---@class ColorBase
---@field r number
---@class ColorChild : ColorBase
---@field g number
---@field b number
---@param c ColorChild
local function useColorChild(c) return c.r end

---@type table<"g"|"b", number>
local missingInherited = {}
useColorChild(missingInherited)
-- ^ diag: type-mismatch

-- Should work: key set covers both own and inherited required fields
---@type table<"r"|"g"|"b", number>
local fullInherited = {}
useColorChild(fullInherited)

-- Should match vacuously: class with no fields at all
---@class EmptyClass
---@param e EmptyClass
local function useEmpty(e) end

---@type table<"x", number>
local anyDict = {}
useEmpty(anyDict)
