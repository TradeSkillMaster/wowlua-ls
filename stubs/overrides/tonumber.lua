---@meta basic_lua

---When called with no base, attempts to convert its argument to a number.
---Returns `nil` if it cannot be converted.
---When called with `base`, the first argument is interpreted as an
---integer numeral in that base; may still return `nil` if the value
---is not a valid numeral in the given base.
---[View documents](https://www.lua.org/manual/5.1/manual.html#pdf-tonumber)
---@param e any
---@return number?
---@overload fun(e: any, base: integer): integer?
---@nodiscard
function tonumber(e) end
