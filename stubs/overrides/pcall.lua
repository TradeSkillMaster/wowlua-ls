---@meta basic_lua

---Calls the function `f` with the given arguments in protected mode.
---If `f` raises an error, `pcall` catches it and returns `false` plus the
---error object.  Otherwise it returns `true` plus all values returned by `f`.
---[View documents](https://www.lua.org/manual/5.1/manual.html#pdf-pcall)
---@generic F
---@param f F
---@param ... params<F>
---@return (true, returns<F>) | (false, string)
function pcall(f, ...) end
