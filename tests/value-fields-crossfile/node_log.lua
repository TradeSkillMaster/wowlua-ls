-- A `---@class` local's field cleared to `nil` keeps the type another write in
-- its file gives it.
---@class NodeReport
---@field x number

local function NewLog()
    ---@class NodeLog
    local log = { n = 1 }
    log.last = nil
    return log
end

---@param holder NodeHolder
---@param report NodeReport
local function Finish(holder, report)
    holder.record.last = report
end

---@param holder NodeHolder
local function Last(holder)
    local last = holder.record.last
    --    ^ hover: (local) last: NodeReport {
    return last
end

return NewLog, Finish, Last
