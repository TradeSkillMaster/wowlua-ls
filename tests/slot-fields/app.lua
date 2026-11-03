---@diagnostic disable: create-global
-- A plain global table and the addon namespace, filled in initializers.
local _, ns = ...

---@class LedgerStore
---@field entries string[]

---@return LedgerStore
function CreateLedgerStore()
    return { entries = {} }
end

LedgerApp = {}
LedgerApp.State = {}

function ns.Initialize(panel)
    ns.Store = CreateLedgerStore()
    ns.Options = { scale = 1, label = "main" }
    ns.Limit = 10
    ns.Pending = nil
    LedgerApp.State.Count = 0
    LedgerApp.State.Panel = panel
end

function ns.Load()
    ns.Pending = "ready"
end

-- One file writing alternative records, and a list.
function ns.SetMode(advanced)
    if advanced then
        ns.Mode = { scale = 1, extra = true }
    else
        ns.Mode = { label = "basic" }
    end
end

function ns.InitOrder()
    ns.Order = { "first", "second" }
end
