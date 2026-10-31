---@diagnostic disable: undefined-global
-- Fields a base class writes in its own methods: other files read them as this
-- file does, a subclass's methods included.
local InputBase = DefineClass("InputBase")

function InputBase:__init()
    self._target = nil
    self._key = nil
end

---@param tbl table
---@param key string
function InputBase:Bind(tbl, key)
    self._target = tbl
    self._key = key
end

local LOCKED_MT = { __newindex = function() end }
local Store = DefineClass("InputStore")

function Store:__init(data, profile)
    self._data = data
    setmetatable(data, LOCKED_MT)
    -- A field also written a value no file can type reads `any` in its file, so
    -- it doesn't read the `table` its other write gives it elsewhere.
    self._options = UnknownProfiles[profile]
end

function Store:Reset(profile)
    self._options = UnknownProfiles[profile] or {}
end
