---@return (number speciesId, number level, number quality)
---      | (nil, nil, nil)
function CrossFileGetInfo()
    if math.random() > 0.5 then
        return 1, 2, 3
    end
    return nil, nil, nil
end
