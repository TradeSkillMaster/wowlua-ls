---@diagnostic disable: unused-local

-- A table constructor replacing an opened entry keeps the methods defined
-- through it (`build_on_stubs`; the cold build's table writes carry no entries).
local resetVersion = DeepMethodReset.API.version
--    ^ hover: (local) resetVersion: number
local resetGet = DeepMethodReset.API.v1.Get()
--    ^ hover: (local) resetGet: number

-- A constructor entry the method is defined on and the call replacing it, as the
-- defining file reads the field (only the workspace build harvests it).
local slotFmt = DeepMethodSlot.Fmt
--    ^ hover: (local) slotFmt: { Extra: fun(self: any): number } | DeepMethodFmt
