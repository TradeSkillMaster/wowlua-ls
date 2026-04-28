---@meta basic_wow

---Returns the elements from the given `list`. This function is equivalent to
---```lua
---return list[i], list[i+1], ···, list[j]
---```
---[View documents](https://www.lua.org/manual/5.1/manual.html#pdf-unpack), [Wiki](https://warcraft.wiki.gg/wiki/API_unpack)
---@generic T
---@param list T[]
---@param i? integer
---@param j? integer
---@return ...T
---@nodiscard
function unpack(list, i, j) end
