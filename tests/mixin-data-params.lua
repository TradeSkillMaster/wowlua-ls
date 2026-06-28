---@diagnostic disable: unused-local, unused-function

-- A parameter typed with a mixin's *data* type (and WoW C APIs, whose params are
-- retyped to the data type) accepts either a plain data table or a real instance.
-- A parameter typed with the *methods* type requires a real instance: a plain
-- table literal can't be one, since its methods would be nil at runtime.

-- C APIs read only data fields, so a plain data table is accepted.
local code = C_ColorUtil.GenerateTextColorCode({ r = 1, g = 0, b = 0 })
local locked = C_Item.IsLocked({ bagID = 0, slotIndex = 1 })

-- A wrong-keyed / typo'd table is still flagged: it shares no field with either
-- member of the `Data | Object` param union, so it can't be the intended type
-- (covers a color typo, and ItemLocation whose data fields are all-optional).
local typo = C_ColorUtil.GenerateTextColorCode({ red = 1, green = 0, blue = 0 })
--           ^ diag: missing-fields
local junk = C_Item.IsLocked({ nope = 1 })
--           ^ diag: missing-fields

-- A real color object (carrying the mixin methods) is accepted too.
local rc = CreateColor(1, 0, 0)
local code2 = C_ColorUtil.GenerateTextColorCode(rc)

-- A user function whose parameter is the methods type rejects a plain table:
-- it would call methods the literal does not have.
---@param color colorRGBA
local function paint(color)
    return color:GenerateHexColor()
end

paint({ r = 1, g = 0, b = 0 })
--    ^ diag: missing-fields

paint(rc)

-- A string-keyed config dict is a data form (never a methods instance), so it is
-- accepted by a methods-typed param: its keys cover the class's data fields, and
-- methods are not required of a dict.
---@param color ColorMixin
local function tint(color) return (color.r or 0) + (color.g or 0) + (color.b or 0) end
---@type table<"r" | "g" | "b" | "a", number>
local cfg = {}
tint(cfg)

-- An unannotated parameter passed to a data-reading C API keeps its methods: the
-- C-API param is an `ItemLocationData | ItemLocation` union, so backward inference
-- retains the object member and a method call on the param still resolves.
local function describe(loc)
    if C_Item.IsBound(loc) then
        return loc:IsValid()
    end
    return false
end
local _ = describe
