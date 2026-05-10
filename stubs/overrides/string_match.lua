---@meta string_match_override
--- Override: string.match returns strings, not any

---Looks for the first match of `pattern` in the string.
---
---[View documents](command:extension.lua.doc?["en-us/51/manual.html/pdf-string.match"])
---
---@param s string | number
---@param pattern string | number
---@param init? integer
---@return (...string) | (nil)
---@nodiscard
function string.match(s, pattern, init) end
