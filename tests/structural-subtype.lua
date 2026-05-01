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
--       ^ diag: none

-- Should NOT warn: table literal has matching fields
usePoint({ x = 1, y = 2 })
--        ^ diag: none

-- Should HINT: extra field not in class definition
useLine({ label = "hello", content = "world", extra = true })
--       ^ diag: inject-field

-- Should NOT warn: optional field omitted
useOptional({ name = "test" })
--           ^ diag: none

-- Should NOT warn: optional field provided
useOptional({ name = "test", tag = "v1" })
--           ^ diag: none

-- Should WARN: missing required field 'content'
useLine({ label = "hello" })
--       ^ diag: type-mismatch ~missing field: 'content'

-- Should WARN: missing required field 'x'
usePoint({ y = 2 })
--        ^ diag: type-mismatch ~missing field: 'x'

-- Should WARN: wrong field type (number instead of string)
useLine({ label = 42, content = "world" })
--       ^ diag: type-mismatch ~wrong type for field: 'label' (expected `string`, got `number`)

-- Should WARN: empty table has no fields to match — still lists missing fields
useLine({})
--      ^ diag: type-mismatch ~missing fields: 'content', 'label'

-- Should WARN: table literal does not have parent's required field 'color'
useChild({ sides = 4 })
--        ^ diag: type-mismatch ~missing field: 'color'

-- Should NOT warn: inherited field satisfied, no excess
useChild({ sides = 4, color = "red" })
--        ^ diag: none

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
--            ^ diag: none
useLine(clean)

-- Regression: tinsert with typed array of @class
---@type ContentLine[]
local lines = {}
tinsert(lines, { label = "hello", content = "world" })
-- ^ diag: none

-- Nil-valued fields in constructors are placeholders, not type errors
---@class InitContext
---@field path string
---@field ready boolean
---@field callback fun()?

---@type InitContext
local ctx = { path = "test", ready = nil, callback = nil }
--          ^ diag: none
_consume(ctx)

-- But non-nil mismatched types should still be caught
---@type InitContext
local ctx2 = { path = "test", ready = "wrong", callback = nil }
--           ^ diag: assign-type-mismatch ~wrong type for field: 'ready' (expected `boolean`, got `string`)
_consume(ctx2)

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
-- ^ diag: none

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
-- ^ diag: none

-- Hash-map exemption only applies when expected is array-shaped, not any type
---@param s string
local function useString(s)
    return s
end

---@type table<string,number>|string[]
local mixedStr = {}
useString(mixedStr)
-- ^ diag: type-mismatch
