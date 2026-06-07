-- Cross-file correlated return "cases": a no-@return method with two correlated
-- return paths. The coarse workspace scan types the synthesized return-only
-- overloads ("cases") as `any`; the lazy whole-file resolver should recover the
-- precise tuple types for cross-file callers (matching the definition site).
---@class CorrSource
local CorrSource = {}

-- No @return: the engine infers correlated cases `(number, number)` (arithmetic)
-- and `(nil, nil)` (bare early return).
function CorrSource:Range(key)
    if not key then
        return
    end
    return key + 1, key + 2
end
