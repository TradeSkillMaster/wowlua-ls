---@diagnostic disable: unused-local
-- Defines global functions, some used and some not.

function UsedGlobal()
    return 1
end

function UnusedGlobal()
    return 2
end

function _IgnoredGlobal()
    return 3
end

UnusedAssignFunc = function()
    return 4
end

UsedAssignFunc = function()
    return 5
end

-- Self-recursive function: locally referenced, should NOT be flagged.
function RecursiveGlobal()
    RecursiveGlobal()
end

-- Method functions on a namespace table.
---@class NS
NS = {}

function NS.UsedMethod()
    return 10
end

function NS.UnusedMethod()
    return 11
end

function NS:UsedColonMethod()
    return 12
end

function NS:UnusedColonMethod()
    return 13
end

function NS._IgnoredMethod()
    return 14
end
