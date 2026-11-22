-- A class declared here, read in user.lua: its hover there lists the fields in
-- the order this file declares them, then the members other files add.
---@class XfOrderBase
---@field zulu number
---@field alpha string

---@class XfOrder : XfOrderBase
---@field name string
---@field count number
---@field active boolean
XfOrder = {}

function XfOrder:Run() end

-- Members this file adds to a class user.lua declares.
function XfOrderHere:Remote() end

-- A parent user.lua assigns a function to, read through a subclass.
---@class XfOrderIA
---@field handler function
XfOrderIA = {}

---@class XfOrderIB : XfOrderIA
XfOrderIB = {}
