---@diagnostic disable: unused-local, create-global
-- Two flavor files define the methods of one mixin, each writing its fields: each
-- reads a field as the union of what both write there, as other files do.
LedgerScanMixin = {}

function LedgerScanMixin:Start()
    self.results = {}
    self.current = nil
end

---@param index integer
function LedgerScanMixin:Record(index)
    self.results[index] = { id = index }
    self.current = "modern"
end

function LedgerScanMixin:Read()
    local current = self.current
    --    ^ hover: (local) current: nil | string | number
end
