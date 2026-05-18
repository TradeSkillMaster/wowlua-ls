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
--       ^ hover: (local) subject: string  diag: none

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

-- Genuinely unknown names on `_G` via a local alias — `_G` is not a @class
-- table, so inject-field does not fire.
gAlias.ThisIsNotARealGlobal = 1
--     ^ diag: none

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

-- Global assignment inside nested blocks (do, if, while, for) should be
-- visible at file scope and produce create-global, not undefined-global.
do
    ---@diagnostic disable-next-line: create-global
    NestedDoGlobal = "test"
    --  ^ hover: (global) NestedDoGlobal: string = "test"
end
_consume(NestedDoGlobal)
--       ^ hover: (global) NestedDoGlobal: string  diag: none

if true then
    ---@diagnostic disable-next-line: create-global
    NestedIfGlobal = 42
    --  ^ hover: (global) NestedIfGlobal: number = 42
end
_consume(NestedIfGlobal)
--       ^ hover: (global) NestedIfGlobal: number  diag: none

-- Without suppression, assignment should produce create-global
do
    NestedDoGlobalWarn = "warn"
    --  ^ diag: create-global
end

-- Global function inside do-block
do
    ---@diagnostic disable-next-line: create-global
    function NestedDoFunc() return 1 end
    --       ^ hover: (global) function NestedDoFunc()
end
_consume(NestedDoFunc)
--       ^ hover: (global) function NestedDoFunc()  diag: none

-- Explicit global creation via _G should NOT produce create-global
_G.ExplicitNewGlobal = "test"
-- ^ diag: none
_consume(ExplicitNewGlobal)
--       ^ hover: (global) ExplicitNewGlobal: string = "test"  diag: none

_G["BracketNewGlobal"] = 99
-- ^ diag: none
_consume(BracketNewGlobal)
--       ^ hover: (global) BracketNewGlobal: number = 99  diag: none

---@param x number
---@return string
_G.ExplicitNewFunc = function(x) return tostring(x) end
-- ^ diag: none
_consume(ExplicitNewFunc)
--       ^ hover: (global) function ExplicitNewFunc(x: number)  diag: none
