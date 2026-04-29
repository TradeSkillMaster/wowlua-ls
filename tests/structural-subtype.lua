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
--       ^ diag: type-mismatch

-- Should WARN: missing required field 'x'
usePoint({ y = 2 })
--        ^ diag: type-mismatch

-- Should WARN: wrong field type (number instead of string)
useLine({ label = 42, content = "world" })
--       ^ diag: type-mismatch

-- Should WARN: empty table has no fields to match
useLine({})
--      ^ diag: type-mismatch

-- Should WARN: table literal does not have parent's required field 'color'
useChild({ sides = 4 })
--        ^ diag: type-mismatch

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
--           ^ diag: assign-type-mismatch
_consume(ctx2)
