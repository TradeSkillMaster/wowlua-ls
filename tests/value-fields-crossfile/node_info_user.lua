---@param node NodeRecord
local function Show(node)
    return node.rank, node.title, node.marked
    --          ^ hover: (field) rank: number
    --                     ^ hover: (field) title: string
    --                                 ^ hover: (field) marked: boolean
end

return Show
