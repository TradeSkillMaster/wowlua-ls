local function make_counter(start)
    local count = start or 0
    return function()
        count = count + 1
        return count
    end
end

local counter = make_counter(10)
local a = counter()
local b = counter()
