---@diagnostic disable: unused-local, create-global
-- Writes through a field typed as a class (`---@type`) don't retype the members
-- the class declares: in a function body, where the write only registers that
-- the field exists, or at file level; on a local `@class` and on the addon
-- namespace. A member the class declares as a class keeps the fields written
-- through it (`inner.extra`), and references to a member (`ns.stepsAlias`) take
-- the declared type. A table another field holds stays that field's (`ns.Panel`,
-- which the scan gives a same-named member written in a function).

local _, ns = ...

---@class TypedInstStep
---@field id number

---@class TypedInstInner
---@field value number

---@class TypedInstWidget
---@field shown boolean

---@class TypedInstBase
---@field running boolean
---@field inner TypedInstInner

---@class TypedInstState : TypedInstBase
---@field steps TypedInstStep[]
---@field index number
---@field cached? TypedInstInner
---@field Panel? TypedInstWidget

---@class TypedInstScan
local Scan = {}

---@type TypedInstState
Scan.state = {
    steps = {},
    index = 1,
    running = false,
    inner = { value = 0 },
}

---@return TypedInstStep[]
function Scan:BuildSteps()
    return {}
end

function Scan:Start()
    Scan.state.steps = self:BuildSteps()
    Scan.state.index = 1
    Scan.state.running = true
    Scan.state.inner.extra = true
end

local step = Scan.state.steps[Scan.state.index]
--    ^ hover: (local) step: TypedInstStep {
local running = Scan.state.running
--    ^ hover: (local) running: boolean
local index = Scan.state.index
--    ^ hover: (local) index: number
local scanValue = Scan.state.inner.value
--    ^ hover: (local) scanValue: number

---@type TypedInstState
ns.state = {
    steps = {},
    index = 1,
    running = false,
    inner = { value = 0 },
}
ns.state.steps = {}

---@return table
function ns.NewSteps()
    return {}
end

function ns.Reset()
    ns.state.steps = ns.NewSteps()
    ns.state.running = false
    ns.state.inner.extra = true
    if ns.state.cached then
        ns.state.cached.extra = true
    end
end

ns.Panel = {}
ns.Panel.title = "x"

function ns.Show(widget)
    ns.state.Panel = widget
end

ns.stepsAlias = ns.state.steps
TypedInstStepsAlias = ns.state.steps

local nsSteps = ns.state.steps
--    ^ hover: (local) nsSteps: TypedInstStep[]
local nsRunning = ns.state.running
--    ^ hover: (local) nsRunning: boolean
local nsValue = ns.state.inner.value
--    ^ hover: (local) nsValue: number
