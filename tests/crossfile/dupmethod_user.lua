-- Cross-file duplicate method test: calling the varargs overload should not
-- produce a false positive type-mismatch on the chatframe parameter.

---@class DupMethodUser : DupMethodMixin
local DupMethodUser = {}

DupMethodUser:Print("hello", "world")
-- ^ diag: none
