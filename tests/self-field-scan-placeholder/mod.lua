-- Regression: a `self.field = <call>` write that the workspace self-field scan
-- cannot type is parked as a bare `table` placeholder. `select(N, UnitClass(...))`
-- is one such case — the argument nests another call, so the scan treats the
-- assignment as unresolvable and infers a bare `table`. That placeholder is a
-- "type unknown" marker, not an author annotation, so the real scalar/nil write
-- must NOT report `field-type-mismatch` against it. (No `diag:` markers on the
-- placeholder writes: the harness already fails on any unexpected diagnostic.)
---@diagnostic disable: unused-local, unused-function

local _, NS = ...

-- Addon-namespace field the scan can't type: the addon-ns branch parks it as a
-- `Table(Some(idx))` whose synthesized sub-table is flagged `placeholder: true`
-- (the `SPModule` self-fields below instead use the `Table(None)` arm). The
-- per-file scalar resolution of the same write must not mismatch the placeholder.
NS.classId = select(3, UnitClass("player"))

---@class SPModule
local Module = {}

function Module:OnInit()
    self.classId = select(3, UnitClass("player"))
    self.classTag = select(2, UnitClass("player"))
    self.cached = nil
end

function Module:Refresh()
    -- Same field written again with a scalar — both this and the `nil` write
    -- above used to mismatch the bare `table` placeholder.
    self.cached = select(3, UnitClass("player"))
end

function Module:Read()
    return self.classId, self.classTag, self.cached
end

-- Negative control: an author-declared `@field data table` is authoritative
-- (not `from_scan`), so a genuine scalar write to it still fires the diagnostic.
---@class SPTyped
---@field data table
local Typed = {}

function Typed:Set()
    self.data = 5
    --   ^ diag: field-type-mismatch
end
