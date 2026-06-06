-- Cross-file self-field test: deep-chain methods should attribute self-fields
-- to the sub-table's class, not the root table's class.
-- Regression test for fields "bumping up" to the parent table.

---@class DeepRoot
local DR = {}

---@class DeepSub
DR.Sub = {}

---@param mgr table
function DR.Sub:Init(mgr)
    ---@type string
    self.label = "hello"
    self.manager = mgr
    self.ready = true
end
