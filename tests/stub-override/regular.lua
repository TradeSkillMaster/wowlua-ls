-- A normal workspace file (NOT `---@meta`). Its annotated method collides with a
-- built-in stub method, but must NOT override it: only `@meta` declaration files
-- override stubs, so ordinary addon code that happens to reuse a stub method name
-- never silently clobbers the stub's type. WrapTextInColorCode's real stub
-- returns string; the wrong `@return number` here is ignored. Asserted in user.lua.
---@class ColorMixin
local ColorMixin = {}

---@return number wrong
function ColorMixin:WrapTextInColorCode(text) end
