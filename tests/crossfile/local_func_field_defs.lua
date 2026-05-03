-- Cross-file test: local function assigned to addon namespace field by name
local private = select(2, ...)

local function SortByMapName(a, b)
    return a < b
end

private.SortByMapName = SortByMapName

-- Also test local variable assigned a function expression
local FormatLabel = function(text)
    return "[" .. text .. "]"
end

private.FormatLabel = FormatLabel
