---@diagnostic disable: create-global
---@diagnostic disable: unused-local
-- A library embedded in the namespace, else loaded on its own: written through a
-- local bound to the namespace field in one branch.
local _, ns = ...

---@return table
function LedgerCopy(t) return t end

-- The library loaded on its own, absent here.
function LedgerLoadLib(name) end

local lib
if ns.Lib then
    lib = ns.Lib
else
    lib = LedgerLoadLib("Lib")
end

lib.internal = {}
local internal = lib.internal
--                   ^ hover: (field) internal: {
function internal:Trigger(callback) end

-- A table other files fill, written a value saying no more than `table` here.
lib.SettingType = LedgerCopy({ Dropdown = 1 })
lib.SettingType.ColorPicker = 10
--              ^ hover: (field) ColorPicker: number
