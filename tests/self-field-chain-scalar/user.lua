---@diagnostic disable: unused-local
-- Consumer re-declares @class ScalarHost (the addon-module re-declaration idiom).
-- The chained self-field `baseHeight` from lib.lua resolves here as `any`, so
-- copying it into a local and passing that local to a `number` parameter is
-- clean — no `type-mismatch`. A `table` placeholder would have produced
-- `got table` (the reported false positive).

---@class ScalarHost
local Host = {}

function Host:Apply(width)
    local height = self.baseHeight
    --                  ^ hover: (field) baseHeight: any
    C_NamePlate.SetNamePlateSize(width, height)
    return height
end
