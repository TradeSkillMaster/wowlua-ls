---@diagnostic disable: unused-local, unused-function
-- Cross-file regression: lazy-init guard on a bare self-field must not
-- be flagged as redundant-condition. The field type is inferred from the
-- `self._cache = {}` assignment in lib.lua (no ---@type), so it may be
-- nil at runtime before that assignment runs.

---@class LazyCacheChild : LazyCache
local Child = {}

function Child:EnsureCache()
    if self._cache then
--              ^ hover: (field) _cache: table  def: external
        return
    end
    self._cache = {}
end
