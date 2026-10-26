---@class NsListsAddon
local addonTable = select(2, ...)

function addonTable.ResetPending()
    addonTable.Pending = nil
end

-- Adding to a list another file builds stores an element: the value isn't
-- checked against the elements that file stores.
---@param name string
function addonTable.AddName(name)
    tinsert(addonTable.Entries, name)
    --                          ^ hover: (param) name: string
end
