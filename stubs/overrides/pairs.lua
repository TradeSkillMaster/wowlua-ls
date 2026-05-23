---@meta basic_lua

---Returns an iterator function, the table `t`, and `nil`, so that the construction
---```lua
---for k, v in pairs(t) do body end
---```
---will iterate over all key–value pairs of table `t`.
---[View documents](https://www.lua.org/manual/5.1/manual.html#pdf-pairs)
---@generic K, V
---@param t table<K, V>
---@return fun(table: table<K, V>, index?: K): K!, V!
---@return table<K, V>
---@nodiscard
function pairs(t) end
