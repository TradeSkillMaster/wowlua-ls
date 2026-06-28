---@diagnostic disable: unused-local
-- Cross-file self-field test: deep-chain methods should attribute self-fields
-- to the sub-table's class, not the root table's class.

-- Fields should be on DeepSub, not DeepRoot
---@type DeepSub
local sub = {}

local l = sub.label
--            ^ hover: (field) label: string  def: external
local m = sub.manager
--            ^ hover: (field) manager: any  def: external
local r = sub.ready
--            ^ hover: (field) ready: boolean  def: external

-- DeepRoot should NOT have the sub-table's self-fields
---@type DeepRoot
local root = {}

local rl = root.label
--              ^ diag: undefined-field
local rm = root.manager
--              ^ diag: undefined-field
local rr = root.ready
--              ^ diag: undefined-field
