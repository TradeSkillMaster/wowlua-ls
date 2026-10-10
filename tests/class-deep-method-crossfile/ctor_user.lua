---@diagnostic disable: unused-local

-- A table constructor replacing an opened entry keeps the methods defined
-- through it (`build_on_stubs`; the cold build's table writes carry no entries).
local resetVersion = DeepMethodReset.API.version
--    ^ hover: (local) resetVersion: number
local resetGet = DeepMethodReset.API.v1.Get()
--    ^ hover: (local) resetGet: number
