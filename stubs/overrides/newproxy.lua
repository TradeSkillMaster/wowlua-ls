---@meta basic_wow

---Creates a userdata proxy object. If useMt is true, the proxy has its own
---metatable that can be retrieved and modified with getmetatable/setmetatable.
---@param useMt? boolean
---@return userdata
---@nodiscard
function newproxy(useMt) end
