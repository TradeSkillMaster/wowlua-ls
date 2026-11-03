---@diagnostic disable: unused-local
-- A table this file exports, read here through its local: a field another file
-- writes through the export (`settings.lua`) is that file's type here too.
local _, ns = ...

local Shared = { count = 0 }
ns.Shared = Shared

local function isMuted()
    return Shared.muted
--                ^ hover: (field) muted: boolean
end
