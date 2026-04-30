---@meta basic_wow

---Returns a string representation of the current calling stack.
---[Wiki](https://warcraft.wiki.gg/wiki/API_debugstack)
---@param coroutine thread
---@param start? number
---@param count1? number
---@param count2? number
---@return string
---@overload fun(start?: number, count1?: number, count2?: number): string
---@nodiscard
function debugstack(coroutine, start, count1, count2) end
