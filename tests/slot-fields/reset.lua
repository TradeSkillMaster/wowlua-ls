-- A write through a string key (`ns["Limit"]`) is one of the field's writes.
local _, ns = ...

function ns.Reset()
    ns["Limit"] = 20
end
