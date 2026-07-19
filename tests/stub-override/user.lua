---@diagnostic disable: unused-local

---@type ColorMixin
local c

-- (1) An annotated method in a @meta file (override.lua) overrides the built-in
-- stub method of the same name: hover shows the workspace return type, not the
-- stub's r, g, b numbers.
local hex = c:GetRGB()
--    ^ hover: (local) hex: string

-- Go-to-definition still offers BOTH the workspace override and the displaced
-- stub site (like any multiply-defined method).
c:GetRGB()
--^ defs: 2

-- (2) A bare (unannotated) method in a @meta file does NOT override; the richer
-- stub GetHSL (numbers) is preserved.
local h = c:GetHSL()
--    ^ hover: (local) h: number

-- (3) An annotated method in a NON-@meta file (regular.lua) does NOT override:
-- the built-in WrapTextInColorCode stub (string) is preserved despite the wrong
-- `@return number` declared there — only @meta files override stubs.
local s = c:WrapTextInColorCode("hi")
--    ^ hover: (local) s: string

-- (4) Order-independence: a non-@meta file (conflicting.lua) also declares
-- GenerateHexColor and is scanned first, but the @meta override still wins —
-- whether the override applies must not depend on scan/file order.
local g = c:GenerateHexColor()
--    ^ hover: (local) g: boolean
