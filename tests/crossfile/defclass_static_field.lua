---@diagnostic disable: shadowed-local, unused-local
-- Cross-file test: builder chain assigned to a defclass class field (static field)
-- Tests that overlay field expressions on external tables get resolved by the fixpoint
-- loop, and that top-level class field assignments don't fire inject-field.

local Component = DefineClass("ChainTestComponent")
local Schema = Component:Include("ChainSchema")
local Element = DefineClass("StaticFieldElement")

-- Builder chain assigned to class field — must resolve (not ?)
Element._SCHEMA = Schema:AddTypedString("label"):AddTypedNumber("count"):AddTypedBool("active")

-- No inject-field on the static field assignment
Element._ACTION_LIST = {}

-- Built type should be accessible via CreateInstance()
local inst = Element._SCHEMA:CreateInstance()
local lbl = inst.label
--    ^ hover: (local) lbl: string

local cnt = inst.count
--    ^ hover: (local) cnt: number?

local act = inst.active
--    ^ hover: (local) act: boolean

-- ── Constructor field assignment (local table field expr) ──────────────────
-- Tests that builder chain expressions inside constructors get resolved by
-- the fixpoint loop.

-- @return built : Parent — test parent class inheritance on built types
local inst2 = Element._SCHEMA:CreateInstanceWithParent()
local gv_top = inst2.GetValue
--    ^ hover: (local) function gv_top(self: any, key: string)

function Element:__init()
    local inst = Element._SCHEMA:CreateInstance()
    local lbl2 = inst.label
    --    ^ hover: (local) lbl2: string
end
