-- A field whose only writes are locals nothing types reads the scan's `any`, and
-- so does what read it while those locals were still pending.
---@class UntypedHost
local Host = {}

---@param s string
local function Label(s) return s end

function Host:Use()
    local v = self.value or 0
    --    ^ hover: (local) v: any
    local alias = v
    --    ^ hover: (local) alias: any
    local got = self:Get()
    --    ^ hover: (local) got: any
    return Label(alias), Label(got), Label(self.value or 0), Label(self.label or 0), Label(self.extra or 0)
end

function Host:Get()
    return self.value or 0
end

---@param opts table
function Host:Init(opts)
    local value = opts.make()
    self.value = value
end

function Host:SetLabel(cfg)
    local label = cfg.label
    self.label = label
end

function Host:SetExtra(...)
    local extra = select(2, ...)
    self.extra = extra
end

-- A write that never resolves doesn't widen the ones that do.
function Host:Count()
    local n = self.count
    --    ^ hover: (local) n: number
    return n
end

---@param opts table
function Host:Load(opts)
    local count = opts.read()
    self.count = count
end

function Host:Reset()
    self.count = 0
end

return Host
