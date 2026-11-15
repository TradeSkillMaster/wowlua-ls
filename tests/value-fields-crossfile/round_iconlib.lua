---@diagnostic disable: unused-local
-- A library made through a local holding its name: its class is the library's own
-- table, not one a locale file's call names after it (round_iconlib_locale.lua).
local MAJOR_VERSION = "RoundIconLib-1.0"
local lib = LibStub:NewLibrary(MAJOR_VERSION, 1)
if not lib then return end

function lib:Debug(...)
end
local self_lib = lib
--    ^ hover: (local) self_lib: RoundIconLib-1.0 {\n  Debug: fun(self: RoundIconLib-1.0, ...)\n}
