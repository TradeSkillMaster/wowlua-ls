-- A local table a file exports to its namespace: the fields its methods write
-- through `self` are the namespace field's.
local _, ns = ...
local Viewer = {}
ns.Viewer = Viewer

---@param id number
function Viewer:Show(id)
    self.currentId = id
end
