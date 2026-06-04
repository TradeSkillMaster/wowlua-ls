---@diagnostic disable: unused-local, empty-block
-- Cross-file test: bracket writes don't override field type to boolean
local _, ns = ...

-- Field should be table (initialized with {}), not boolean
local idx = ns.currIndexes
--    ^ hover: (local) idx: table

-- pairs() should work without generic-constraint-mismatch
for k, v in pairs(ns.currIndexes) do
end
