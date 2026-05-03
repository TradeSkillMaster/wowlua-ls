-- Cross-file test: or-chaining on addon namespace fields (defensive init)
local _, ns = ...

-- Defensive initialization pattern: `x = x or function()`
-- The `or` right operand should determine the field's type.
ns.CompareByName = ns.CompareByName or function(a, b)
    return a < b
end

-- Or-table pattern
ns.ItemCache = ns.ItemCache or {}
