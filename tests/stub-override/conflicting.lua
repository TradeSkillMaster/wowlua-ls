-- A NON-@meta file that also re-declares ColorMixin:GenerateHexColor, named to
-- sort before override.lua so it is scanned first. Because it is not `@meta` it
-- must NOT override the built-in stub, and — critically — it must NOT prevent the
-- `@meta` override in override.lua from winning just by being scanned first
-- (build_on_stubs prioritizes `@meta` method overrides ahead of the `seen_methods`
-- dedup). Its wrong `@return number` is the tell: if it claimed the method, the
-- hover in user.lua would be number, not the `@meta` boolean.
---@class ColorMixin
local ColorMixin = {}

---@return number wrong
function ColorMixin:GenerateHexColor() end
