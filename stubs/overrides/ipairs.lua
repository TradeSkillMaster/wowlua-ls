---@meta basic_lua

---Returns three values (an iterator function, the table `t`, and `0`) so that
---the construction
---```lua
---for i, v in ipairs(t) do body end
---```
---will iterate over the key–value pairs `(1,t[1])`, `(2,t[2])`, ..., up to the
---first absent index.
---[View documents](https://www.lua.org/manual/5.1/manual.html#pdf-ipairs)
---@generic V
---@param list V[]
---@return fun(table: V[], i?: integer): integer, V!
---@return V[]
---@return integer
---@nodiscard
function ipairs(list) end
