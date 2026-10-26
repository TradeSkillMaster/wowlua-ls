-- A list built on a global table another file defines (globals.lua).
NsListsRegistry.recent = {}

---@param id number
function NsListsRegistry.Remember(id)
    tinsert(NsListsRegistry.recent, id)
end
