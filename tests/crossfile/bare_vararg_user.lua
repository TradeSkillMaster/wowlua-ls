-- The inherited bare-vararg method must hover with a plain `...` trailing param.

---@class BareVarargUser : BareVarargMixin
local BareVarargUser = {}

BareVarargUser:Collect("x")
--             ^ hover: (method) function BareVarargUser:Collect(first, ...)
