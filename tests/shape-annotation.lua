---@diagnostic disable: unused-local, unused-function
-- `@shape` — a class declares plain-table forms accepted where it is expected
-- (the userdata/mixin escape hatch). The class keeps its methods/fields for
-- member access; a plain table matching a shape is accepted even though it lacks
-- the methods. An unrelated table still mismatches (no blanket hole), and a
-- wrong-typed shape field is still a genuine mismatch.

-- Two mutually-exclusive construction variants (bag+slot OR equipment slot).
---@class Loc
---@shape { bagID: number, slotIndex: number } | { equipmentSlotIndex: number }
---@field bagID number
---@field slotIndex number
---@field equipmentSlotIndex number
---@field IsValid fun(self: Loc): boolean

---@param loc Loc
local function useLoc(loc) return loc.bagID end

-- Accepted: matches shape member 1 (bag + slot), despite missing equipmentSlotIndex
-- and the IsValid method.
useLoc({ bagID = 0, slotIndex = 1 })

-- Accepted: matches shape member 2 (equipment slot).
useLoc({ equipmentSlotIndex = 5 })

-- Accepted: extra field tolerated alongside a matching shape.
useLoc({ bagID = 0, slotIndex = 1, tag = "x" })

-- Rejected: an unrelated table matches no shape (no hole). A shape-bearing class
-- is matched solely by its shapes, so a non-matching table is a type-mismatch.
useLoc({ foo = 1 })
--      ^ diag: type-mismatch

-- Single-form shape: r/g/b required, alpha optional.
---@class Color
---@shape { r: number, g: number, b: number, a?: number }
---@field r number
---@field g number
---@field b number
---@field a number
---@field GenerateHex fun(self: Color): string

---@param c Color
local function useColor(c) return c.r end

-- Accepted: a full {r,g,b,a} literal.
useColor({ r = 1, g = 1, b = 1, a = 1 })

-- Accepted: {r,g,b} (alpha optional in the shape).
useColor({ r = 1, g = 1, b = 1 })

-- Accepted: a string-keyed dict whose keys cover r/g/b/a matches the shape
-- (the ColorMixin pattern: config colors typed as `table<keys, number>`).
---@type table<"r"|"g"|"b"|"a", number>
local rgba = {}
useColor(rgba)

-- Rejected: a dict whose keys do not cover r/g/b.
---@type table<"x"|"y", number>
local xy = {}
useColor(xy)
--       ^ diag: type-mismatch

-- The shape drives read-side nilability. equipmentSlotIndex is absent from the
-- {bagID, slotIndex} member, so on a Loc value it is conditionally present:
---@type Loc
local probe = nil
local _eq = probe.equipmentSlotIndex
--    ^ hover: (local) _eq: number?

-- r/g/b are required in ColorMixin's single shape member, so they stay non-nil;
-- alpha is optional in the shape, so it is nilable.
---@type Color
local cprobe = nil
local _r = cprobe.r
--    ^ hover: (local) _r: number
local _a = cprobe.a
--    ^ hover: (local) _a: number?

-- Regression: a `@shape` that names another @class must terminate, not recurse
-- forever. A cyclic pair (CycA's shape is CycB, CycB's shape is CycA) plus a
-- non-matching table must not overflow the stack. A class is not a plain-table
-- form, so the table matches no shape and is a type-mismatch.
---@class CycA
---@shape CycB
---@field run fun(self: CycA)

---@class CycB
---@shape CycA
---@field run fun(self: CycB)

---@param x CycA
local function useCyc(x) x:run() end

useCyc({ z = 1 })
--      ^ diag: type-mismatch
