---@meta basic_lua

---Allows a program to traverse all fields of a table. Its first argument is a
---table and its second argument is an index in this table. A call to `next`
---returns the next index of the table and its associated value. When called with
---`nil` as its second argument, `next` returns an initial index and its
---associated value. When called with the last index, or with `nil` in an empty
---table, `next` returns `nil`. If the second argument is absent, then it is
---interpreted as `nil`.
---[View documents](https://www.lua.org/manual/5.1/manual.html#pdf-next)
---@generic K, V
---@param t table<K, V>
---@param k? K
---@return K?
---@return V!
---@nodiscard
function next(t, k) end
