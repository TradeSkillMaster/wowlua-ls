-- Test: undefined-global diagnostic (requires stubs)
local function _consume(...) end

-- Should warn: typo in WoW API name
_consume(CretaeFrame)
--       ^ diag: undefined-global

-- Should NOT warn: real WoW API global
_consume(CreateFrame)
--       ^ diag: none

-- Should warn: non-existent global
_consume(nonExistentGlobal123)
--       ^ diag: undefined-global

-- Should NOT warn: real WoW global (FrameXML stub)
_consume(WOW_PROJECT_ID)
--       ^ diag: none

-- Should NOT warn: _G is a built-in Lua global
_consume(_G)
--       ^ diag: none

-- Should NOT warn: suppressed
---@diagnostic disable-next-line: undefined-global
_consume(totallyFakeGlobal)
-- ^ diag: none

-- Should NOT warn: field access on grouped expression (not a global)
local t1 = { hex = "red" }
local t2 = { hex = "blue" }
local _color = (t1 or t2).hex
--                         ^ diag: none

-- Should NOT warn: bare local declaration without assignment
local subject
if true then
    subject = "hello"
end
_consume(subject)
--       ^ hover: (global) subject: string  diag: none

-- Writing to `_G.<known-global>` is a plain global assignment, not field
-- injection on the `_G` class.
_G.print = function() end
-- ^ diag: none
_G.CreateFrame = nil
-- ^ diag: none

-- Writing to `_G` via a local alias (common FrameXML-override idiom) also
-- bypasses inject-field when the name is a known global.
local gAlias = _G
gAlias.ChatFrame_OnEvent = function() end
--     ^ diag: none

-- Genuinely unknown names still trigger inject-field on `_G`.
gAlias.ThisIsNotARealGlobal = 1
--     ^ diag: inject-field

-- Workspace-defined globals (declared in the same file or another file) are
-- also recognized — matching `undefined-global`'s scope walk.
---@diagnostic disable-next-line: create-global
function MyAddonGlobalFn() end
---@diagnostic disable-next-line: create-global
MyAddonGlobalVar = 1
_G.MyAddonGlobalFn = function() end
-- ^ diag: none
gAlias.MyAddonGlobalVar = 2
--     ^ diag: none
