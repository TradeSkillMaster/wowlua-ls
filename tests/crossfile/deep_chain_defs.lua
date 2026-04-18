-- Deep cross-file test: defines 4+ part chains rooted at the addon namespace
local _, ns = ...

-- 4-part field chain: intermediate sub-tables auto-created
ns.Data.Config.version = 1
ns.Data.Config.title = "Deep"

-- 5-part field chain: 3 intermediates auto-created
ns.Deep.Nested.Inner.Leaf.count = 7

-- 4-part method chain: method on auto-created sub-table
---@return string
function ns.Engine.Core:Start()
    return "started"
end

-- 5-part method chain
---@return number
function ns.Engine.Core.Parser:Parse()
    return 0
end

-- Type conflict: a deep write whose first path segment collides with an
-- existing non-table field must NOT overwrite the original field.
ns.Conflict = "hello"
ns.Conflict.shouldNotExist = 42

-- Deep methods on a local table that is later aliased to the addon ns.
-- All depths of the buffered `function LocalDb.*:Foo()` definitions must be
-- flushed onto ns.Db (the addon alias), with the buffered intermediates
-- prepended under the alias.
local LocalDb = {}
---@return string
function LocalDb:Direct()
    return "d"
end
---@return string
function LocalDb.Sub:OneDeep()
    return "1"
end
---@return string
function LocalDb.Sub.Inner:TwoDeep()
    return "2"
end
ns.Db = LocalDb
