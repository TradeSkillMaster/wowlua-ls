---@diagnostic disable: unused-local

---@type XFieldOptions
local opts = {}
local optValue = opts.option.value
--    ^ hover: (local) optValue: boolean
--                    ^ hover: (field) option: XFieldValue<boolean>
--                           ^ hover: (field) value: boolean
local optItem = opts.option.items[1]
--    ^ hover: (local) optItem: boolean
local maybeValue = opts.maybe and opts.maybe.value
--    ^ hover: (local) maybeValue: number?

---@type XFieldValue<boolean>
local direct = {}
local directValue = direct.value
--    ^ hover: (local) directValue: boolean

---@type XFieldFixed
local fixed = {}
local fixedValue = fixed.value
--    ^ hover: (local) fixedValue: string
