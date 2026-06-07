---@diagnostic disable: unused-local, create-global
-- Dynamic global prefix test: definition file
-- Exercises _G["PREFIX"..k] = v pattern that exports computed globals.

local currentLocale = {
    NONE = "None",
    OK = "OK",
    CANCEL = "Cancel",
}

-- Export constants into global scope via computed _G write
for key, value in pairs(currentLocale) do
    _G["MYADDON_L_" .. key] = value
end

-- Suffix pattern (less common but valid)
local handlers = { Click = true, Enter = true }
for name, flag in pairs(handlers) do
    _G[name .. "_HANDLER"] = flag
end
