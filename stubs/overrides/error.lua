---@meta basic_lua

---Terminates the last protected function called and returns message as the error object.
---
---Usually, `error` adds some information about the error position at the beginning of the message, if the message is a string.
---[View documents](https://www.lua.org/manual/5.1/manual.html#pdf-error)
---@param message any
---@param level? integer
---@noreturn
---@secret-args untainted level
function error(message, level) end
