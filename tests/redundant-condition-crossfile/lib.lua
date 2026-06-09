---@diagnostic disable: unused-local, unused-function
-- Library file: class with bare self-field assignments (no ---@type).

---@class LazyCache
local LazyCache = {}

function LazyCache:__init()
    self._cache = nil
end

function LazyCache:Populate()
    self._cache = {}
end
