---@diagnostic disable: unused-local

local _, ns = ...

local steps = ns.state.steps
--    ^ hover: (local) steps: TypedInstStep[]
local running = ns.state.running
--    ^ hover: (local) running: boolean
local extra = ns.state.inner.extra
--    ^ hover: (local) extra: boolean
local value = ns.state.inner.value
--    ^ hover: (local) value: number
local cached = ns.state.cached
--    ^ hover: (local) cached: TypedInstInner?
local cachedExtra = ns.state.cached.extra
--    ^ hover: (local) cachedExtra: boolean
local panelTitle = ns.Panel.title
--    ^ hover: (local) panelTitle: string
local statePanel = ns.state.Panel
--    ^ hover: (local) statePanel: TypedInstWidget?
local stepsAlias = ns.stepsAlias
--    ^ hover: (local) stepsAlias: TypedInstStep[]
local globalAlias = TypedInstStepsAlias
--    ^ hover: (local) globalAlias: TypedInstStep[]

---@param scan TypedInstScan
local function Read(scan)
    local scanSteps = scan.state.steps
    --    ^ hover: (local) scanSteps: TypedInstStep[]
    local index = scan.state.index
    --    ^ hover: (local) index: number
    local scanExtra = scan.state.inner.extra
    --    ^ hover: (local) scanExtra: any
end
