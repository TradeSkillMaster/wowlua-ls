---@meta basic_lua

---Loads a chunk from the given string.
---Returns the compiled chunk as a function, or `nil` plus an error message
---if the string cannot be compiled.
---[View documents](https://www.lua.org/manual/5.1/manual.html#pdf-loadstring)
---@param text string
---@param chunkname? string
---@return (fun(...): ...any) | (nil, string)
function loadstring(text, chunkname) end
