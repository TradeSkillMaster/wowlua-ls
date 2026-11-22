---@diagnostic disable: unused-local, unused-function

-- A hover's clickable types: the classes and aliases its signature names in
-- type position, each once, in order.

---A record naming types in each position a hover shows them.
---@class HlRecord : HlBase-1.0
---@field range HlRange
---@field ranges HlRange[]? An HlRange per frame.
---@field parent HlRecord?
---@field cb fun(r: HlRange, mode: HlMode): HlRecord
local HlRecord = {}

-- A type's own hover leaves the type out.
---@type HlRecord
--        ^ links: HlRange, HlMode
local rec = HlRecord
--    ^ links: HlRecord, HlRange, HlMode

---@type HlMode
--        ^ links: none
local mode = "fast"

---@type HlBase-1.0
local base = nil
--    ^ links: HlBase-1.0

-- Field and parameter names, descriptions, string literals and number literals
-- that spell a type's name don't link to it.
---@class HlLabels
---@field HlRange number The HlRange this counts.
---@field kind "HlMode"
---@field count 0|1
local labels = {}
--    ^ links: HlLabels

---@param HlRange number
---@param r HlRange
---@param map table<string, HlBase-1.0>
---@return HlRecord?
local function Make(HlRange, r, map) end
--             ^ links: HlRange, HlBase-1.0, HlRecord
--                               ^ links: HlBase-1.0

-- A function or method name doesn't either; the class a method is on does.
---@class HlWidget
local HlWidget = {}
function HlWidget:HlRange() end
--                ^ links: HlWidget

local function HlBase() end
--             ^ links: none

-- Nor does an untyped parameter named like a type, or an enum member.
local function Paint(HlRange, size) end
--             ^ links: none

---@enum HlKind
local HlKind = { HlRange = 1, HlMode = 2 }
--    ^ hover: (local) HlKind: HlKind {\n  HlRange = 1,\n  HlMode = 2\n}  links: HlKind

-- Built-in Lua types don't link.
---@type table<string, number>
local counts = {}
--    ^ links: none
