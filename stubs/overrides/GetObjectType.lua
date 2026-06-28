---@meta _

---@class FrameScriptObject
local FrameScriptObject = {}

-- `GetObjectType` returns the widget's class name (e.g. "Button", "FontString").
-- `@returns-class-name` lets the language server narrow the receiver on an
-- equality comparison: `region:GetObjectType() == "FontString"` narrows `region`
-- to `FontString` in the then-branch (and the `~=` / early-exit complements),
-- mirroring the `@type-narrows` narrowing on `IsObjectType`.
---@returns-class-name
---@return string objectType
function FrameScriptObject:GetObjectType() end
