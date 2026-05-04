local _, ns = ...

-- Cross-file params<F> projection: returned function should carry F's param names
local wrapped = ns.wrapper(function(arg1, arg2)
    return 0
end)

wrapped(1, 2)
--^ hover: (local) function wrapped(arg1, arg2)\n-> string

-- Cross-file returns<F> projection: returned function should carry F's return type
---@return boolean
local function producer() return true end
local lifted = ns.lift(producer)

lifted(42)
--^ hover: (local) function lifted(x: number)\n-> boolean

_G.useParamsProjUser = { wrapped, lifted }
