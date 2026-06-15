---@diagnostic disable: unused-local, unused-function
-- File that reads named globals created by CreateFrame/CreateFont/CreateFontFamily
-- in another file. These should NOT produce undefined-global.

-- Frame created via CreateFrame (local assignment) in creator.lua
MyAddonFrame:Show()
--           ^ hover: (method) function Frame:Show()

-- Button created via CreateFrame (bare statement) in creator.lua
MyAddonButton:Enable()
--            ^ hover: (method) function Button:Enable()

-- Frame created via CreateFrame (global assignment) in creator.lua
MyAddonPanel:Hide()
--           ^ hover: (method) function Frame:Hide()

-- Font created via CreateFont in creator.lua
local gameFont = MyAddonGameFont
--    ^ hover: (local) gameFont: Font

-- Font created via CreateFontFamily in creator.lua
local fontFamily = MyAddonFontFamily
--    ^ hover: (local) fontFamily: Font

-- Dynamic name was not detected, so it remains undefined
local bad = DynamicFrame
--          ^ diag: undefined-global

-- Frame created inside a function body in creator.lua (detected anywhere, not
-- just at top level).
local nested = MyAddonNestedFrame
--    ^ hover: (local) nested: Frame

-- Button created as a nested call argument in creator.lua.
local wrapped = MyAddonWrappedButton
--    ^ hover: (local) wrapped: Button

-- Frame created with a template mixin in creator.lua. The type is harvested from
-- the call's resolved return, so it carries the template intersection and the
-- template method resolves cross-file (no false undefined-field).
MyAddonTemplatedFrame:SetRowData()
--                    ^ hover: (method) function MyAddonRowTemplate:SetRowData()

-- ── @creates-global annotation validation (LSP feature parity) ──

-- Valid use on a function: NOT flagged malformed-annotation or doc-func-no-function
-- (verified by the harness's exhaustive diagnostic check — no diagnostic expected).
---@param kind string
---@param objName string
---@creates-global 2
local function makeNamedObject(kind, objName) end

-- Malformed: a non-numeric parameter index is reported.
---@creates-global notANumber
--  ^ diag: malformed-annotation
local function badCreatesGlobal() end

-- Malformed: zero is invalid (1-based index).
---@creates-global 0
--  ^ diag: malformed-annotation
local function zeroIndexCreatesGlobal() end

-- An extra token after the index is ignored (the type is harvested from the
-- call), so a legacy `@creates-global 2 1` is accepted without a diagnostic.
---@param kind string
---@param objName string
---@creates-global 2 1
local function legacyCreatesGlobal(kind, objName) end

-- Function-level annotation not attached to a function is reported.
---@creates-global 1
--  ^ diag: doc-func-no-function
local notAFunction = 5
