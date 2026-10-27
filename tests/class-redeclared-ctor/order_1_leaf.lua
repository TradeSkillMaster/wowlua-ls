-- Three-level chains over RedeclBase, whose `type` each level's constructor
-- writes: this leaf's file is read before its middle class's (order_2_mid.lua),
-- order_3_mid.lua's before order_4_leaf.lua's. reader.lua reads both leaves.

local TYPE = "LeafFirst"

local function Constructor()
    --- @class LeafFirst : MidSecond
    local widget = { type = TYPE }
    return widget
end

return Constructor
