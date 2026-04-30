-- Cross-file alias test: uses aliases defined in alias_defs.lua

-- Union alias in @type
---@type XfResult
local res = "hello"
--    ^ hover: (local) res: string | number  def: local

-- String literal union alias (order matches alias declaration)
---@type XfStatus
local st = "ok"
--    ^ hover: (local) st: "ok" | "error" | "pending"  def: local

-- Cross-file function using alias param type
local val = RunCallback(function(x) return true end)
--    ^ hover: (local) val: string | number  def: local

-- Cross-file function using alias param type
local ok = CheckStatus("ok")
--     ^ hover: (local) ok: boolean  def: local

-- Alias used locally in @param
---@param cb XfCallback
---@return boolean
local function runIt(cb)
    return cb(5)
end

-- Function-type alias in @type is propagated through assignments: `h` inherits
-- the full fun(...) signature from `handler`, not a collapsed `function` type.
---@type XfCallback
local handler
local h = handler
--    ^ hover: (local) function h(x: number)\n-> boolean  def: local
