-- Test: cross-file table-escape analysis for missing-param/return-annotation.
-- A method on a *local* table is only flagged when that table escapes the file
-- (assigned to a global, declared a @class, or attached to the addon
-- namespace). Purely file-private tables are never flagged — this reuses the
-- workspace global scan, so "API surface" matches what the LS resolves
-- cross-file.

---@diagnostic disable: create-global, unused-local, unused-function, redundant-return, undefined-global, inject-field, missing-return

local _, ns = ...

-- ── File-private local table: NOT flagged ──────────────────────────────────

-- Internal dispatch table that never leaves the file.
local Internal = {}
function Internal.handle(event)
    return event
end
Internal.handle("x")

-- ── Global table: flagged ──────────────────────────────────────────────────

GlobalApi = {}
function GlobalApi.Run(opts)
--                    ^ diag: missing-param-annotation
--       ^ diag: missing-return-annotation
    return opts
end

-- ── Local table attached to the addon namespace: flagged ───────────────────

local Module = {}
function Module.Process(data)
--                     ^ diag: missing-param-annotation
--       ^ diag: missing-return-annotation
    return data
end
ns.Module = Module

-- ── @class local table: flagged ────────────────────────────────────────────

---@class Service
local Service = {}
function Service:Start(config)
--                    ^ diag: missing-param-annotation
    self.config = config
end
