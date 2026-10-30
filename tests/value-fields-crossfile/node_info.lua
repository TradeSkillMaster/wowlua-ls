---@diagnostic disable: create-global
-- A local or parameter a function body binds under `---@class`: what the body
-- writes through it are the class's fields, after a rebinding in a branch too.
---@class NodeRecord
---@field id number

---@return table
local function FetchNode(id) return { id = id } end

NodeViewer = {}

function NodeViewer:Get(id)
    ---@class NodeRecord
    local info = FetchNode(id)
    if not info.id then
        ---@class NodeRecord
        info = FetchNode(0)
    end
    info.rank = 1
    info.title = "x"
    return info
end

---@param node NodeRecord
function NodeViewer:Mark(node)
    ---@class NodeRecord
    node = node
    node.marked = true
end
