---@meta basic_lua

---If `index` is a number, returns all arguments after argument number `index`;
---a negative number indexes from the end (`-1` is the last argument).
---Otherwise, `index` must be the string `"#"`, and `select` returns the total
---number of extra arguments it received.
---[View documents](https://www.lua.org/manual/5.1/manual.html#pdf-select)
---@generic F
---@param index integer
---@param ... returns<F>
---@return returns<F, index>
---@overload fun(index: "#", ...: any): integer
---@nodiscard
function select(index, ...) end
