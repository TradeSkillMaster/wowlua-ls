-- `@noreturn` functions other files call: a global, a namespace table's
-- function and a class method. The tag is the author's claim; the bodies
-- don't decide it.

local _, ns = ...

---@param msg string
---@noreturn
function NoReturnCrossfile_Fail(msg)
end

ns.Errors = {}

---@param msg string
---@noreturn
function ns.Errors.Fail(msg)
end

---@class NoReturnCrossfileLogger
NoReturnCrossfileLogger = {}

---@param msg string
---@noreturn
function NoReturnCrossfileLogger:Fatal(msg)
end

-- Errors in practice, but isn't tagged.
---@param msg string
function NoReturnCrossfile_Warn(msg)
end
