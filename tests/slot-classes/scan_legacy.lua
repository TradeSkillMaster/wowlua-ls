---@diagnostic disable: unused-local, create-global
LedgerScanMixin = {}

function LedgerScanMixin:Start()
    self.results = {}
    self.current = nil
end

---@param index integer
function LedgerScanMixin:Record(index)
    table.insert(self.results, { name = tostring(index) })
    self.current = 1
end

function LedgerScanMixin:Read()
    local current = self.current
    --    ^ hover: (local) current: nil | number | string
end
