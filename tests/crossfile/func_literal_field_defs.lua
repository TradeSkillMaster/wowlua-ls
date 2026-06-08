-- Cross-file test: function literal directly assigned to addon namespace field
---@diagnostic disable: unused-local, unused-function
local private = select(2, ...)

-- Function literal with annotated inner function returned
private.Test = function()
    ---@param a string
    local function MyFunction(a)
    end
    return MyFunction
end

-- Function literal with @param/@return annotations on itself
---@param x number
---@param y number
---@return number
private.Add = function(x, y)
    return x + y
end

-- Function literal with no annotations (body-derived return)
private.MakeGreeting = function(name)
    return "Hello, " .. name
end

-- Function returning a function that itself has a typed return — used to test
-- that calling a FunctionSig resolves to the correct return type cross-file.
private.GetGreeter = function()
    ---@param name string
    ---@return string
    local function Greeter(name)
        return "Hello, " .. name
    end
    return Greeter
end

-- Function literal with bare return on some paths (should NOT resolve
-- the return type as the inner function since it can also return nil)
private.MaybeFunc = function(flag)
    if flag then return end
    ---@param x number
    local function Inner(x)
    end
    return Inner
end
