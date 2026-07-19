-- A workspace "types.lua" declaration file (`---@meta`). An *annotated* method
-- here overrides the colliding built-in WoW API stub method, so the workspace
-- signature wins for hover/signature/completion (go-to-definition still offers
-- both sites). A *bare* (unannotated) method adds no type info and leaves the
-- richer stub in place. Assertions live in user.lua.
---@meta

---@class ColorMixin
local ColorMixin = {}

--- Overrides the built-in ColorMixin:GetRGB (stub returns r, g, b numbers).
---@return string hex
function ColorMixin:GetRGB() end

-- Overrides the built-in ColorMixin:GenerateHexColor (stub returns string). A
-- NON-@meta file (conflicting.lua) also re-declares this method and is scanned
-- first (alphabetically before override.lua); the override must still win, so the
-- result must NOT depend on scan order.
---@return boolean fromMeta
function ColorMixin:GenerateHexColor() end

-- Bare re-declaration in a @meta file: no type info, so the richer stub GetHSL
-- (four numbers) is kept, not overridden.
function ColorMixin:GetHSL() end
