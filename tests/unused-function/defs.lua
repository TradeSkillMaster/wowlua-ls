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

-- Dynamic dispatch via a `keyof`-constrained generic parameter. A method named
-- only by a string literal passed to CallMethod is still a genuine reference.
---@class Dispatcher
Dispatcher = {}

---@generic Obj, K: keyof Obj
---@param obj Obj
---@param method K
function Dispatcher:CallMethod(obj, method)
end

---@class Dispatched
Dispatched = {}

-- Referenced only via `disp:CallMethod(d, "DynamicMethod")` in user.lua.
-- Should NOT be flagged as unused.
function Dispatched:DynamicMethod()
    return 50
end

-- Genuinely unused method on the same class — proves dynamic-dispatch tracking
-- doesn't blanket-suppress real unused methods.
function Dispatched:UnusedDynamicMethod()
    return 51
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

-- Register-by-name event system: the registrar takes the handler owner as an
-- ARGUMENT typed `keyof T` (not `self`), and the handler by string name. A method
-- named only by such a string is a genuine reference (issue #58). This exercises
-- the direct `keyof X` parameter reference path (distinct from the `K: keyof Obj`
-- generic-constraint path covered by Dispatcher:CallMethod above).
---@event RegistrarEvent
---| "ThingHappened" -> value: number

---@class EventRegistrar
EventRegistrar = {}

---@generic T
---@generic E: RegistrarEvent
---@overload fun(owner: T, event: E, handler: keyof T)
---@param owner T
---@param event E
---@param handler fun(...params<E>)
function EventRegistrar.Register(owner, event, handler) end

-- Static functions sharing a name across unrelated modules. A static function
-- is called through its module's path, so the name match is a coincidence,
-- not a duck-typed interface: the unused one must still be flagged.
---@class QueryModuleA
QueryModuleA = {}

function QueryModuleA.CreateQuery()
    return 60
end

---@class QueryModuleB
QueryModuleB = {}

function QueryModuleB.CreateQuery()
    return 61
end

-- Methods taking `self` that share a name across unrelated classes, reached
-- only through an untyped receiver in user.lua. Interface detection keeps both
-- (colon-defined and dot-defined with an explicit `self`).
---@class PollerA
PollerA = {}

function PollerA:IsReady()
    return true
end

function PollerA.Describe(self)
    return self
end

---@class PollerB
PollerB = {}

function PollerB:IsReady()
    return false
end

function PollerB.Describe(self)
    return self
end

-- Referenced only from this file: a same-file call resolves to the file-local
-- copy of the method, which must still count as a reference.
function NS.CalledInDefiningFile()
    return 70
end

function NS.StoredInDefiningFile()
    return 71
end

local function useOwnMethods()
    local handlers = { run = NS.StoredInDefiningFile }
    return NS.CalledInDefiningFile(), handlers
end
useOwnMethods()

-- A string-key write defines the method; the write itself is not a reference.
NS["BracketDefinedUnused"] = function()
    return 72
end

-- Also defined in user.lua (e.g. a flavor-specific file), which calls its own
-- copy: that call is still a reference to the method.
function NS:DefinedInTwoFiles()
    return 73
end

-- A static function and a `self` method sharing a name on unrelated tables:
-- the static one doesn't make the name an interface, so both are flagged.
---@class SettingsStore
SettingsStore = {}

function SettingsStore.Reset()
    return 90
end

---@class ResetWidget
ResetWidget = {}

function ResetWidget:Reset()
    return 91
end
