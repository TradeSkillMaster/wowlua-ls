---@diagnostic disable: unused-local
-- Defines global functions, some used and some not.

function UsedGlobal()
    return 1
end

function UnusedGlobal()
    return 2
end

function _IgnoredGlobal()
    return 3
end

UnusedAssignFunc = function()
    return 4
end

UsedAssignFunc = function()
    return 5
end

-- Self-recursive function: locally referenced, should NOT be flagged.
function RecursiveGlobal()
    RecursiveGlobal()
end

-- Method functions on a namespace table.
---@class NS
NS = {}

function NS.UsedMethod()
    return 10
end

function NS.UnusedMethod()
    return 11
end

function NS:UsedColonMethod()
    return 12
end

function NS:UnusedColonMethod()
    return 13
end

function NS._IgnoredMethod()
    return 14
end

-- Two workspace classes with a shared method name, called via a union-typed
-- receiver. Neither should be flagged as unused. This case is covered by
-- interface detection (2+ workspace tables defining the same method name).
---@class AlphaWidget
AlphaWidget = {}

function AlphaWidget:Process()
    return 20
end

---@class BetaWidget
BetaWidget = {}

function BetaWidget:Process()
    return 21
end

-- Workspace class sharing a method name (AddDoubleLine) with a STUB class
-- (GameTooltip), called via a union-typed receiver `GameTooltip|CustomTip`.
-- Interface detection does NOT count stub methods, so without union-receiver
-- reference tracking the stub method wins the call resolution and
-- CustomTip:AddDoubleLine looks unreferenced — a false-positive unused-function.
---@class CustomTip
CustomTip = {}

function CustomTip:AddDoubleLine(left, right)
    return left, right
end

-- Genuinely unused method on the same class — proves the class's methods CAN
-- still be flagged, so the AddDoubleLine non-flag is meaningful.
function CustomTip:UnusedTipMethod()
    return 22
end

-- Read as a function value (local assignment) in user.lua.
-- Should NOT be flagged as unused.
function NS.FuncAsValueMethod()
    return 15
end

-- Passed as an argument to another function in user.lua (the original TSM pattern).
-- Should NOT be flagged as unused.
function NS.FuncAsArgMethod()
    return 16
end

-- Stored as a value inside a table constructor in user.lua.
-- Should NOT be flagged as unused.
function NS.FuncInTableMethod()
    return 17
end

-- Class with methods, used via a local variable returned from a function.
-- This mirrors the pattern where a factory returns a class instance and
-- the caller invokes methods on the returned value.
---@class Worker
Worker = {}

function Worker:Run()
    return 30
end

function Worker:UnusedWorkerMethod()
    return 31
end

---@return Worker
function CreateWorker()
    return Worker
end

-- Method called on a narrowed return value from a local function.
---@class Processor
local Processor = {}

function Processor:IsValid()
    return true
end

function Processor:Execute()
    return 40
end

function Processor:UnusedProcessorMethod()
    return 41
end
