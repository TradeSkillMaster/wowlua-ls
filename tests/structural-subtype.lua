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

-- Should NOT warn: extra fields are allowed (structural superset)
useLine({ label = "hello", content = "world", extra = true })
--       ^ diag: none

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

-- Regression: tinsert with typed array of @class
---@type ContentLine[]
local lines = {}
tinsert(lines, { label = "hello", content = "world" })
-- ^ diag: none
