local _, ns = ...

---@generic F
---@param func F
---@return fun(...params<F>): string
function ns.wrapper(func)
    return function(...)
        return tostring(func(...))
    end
end

---@generic F
---@param func F
---@return fun(x: number): returns<F>
function ns.lift(func)
    return function(x)
        return func(x, x)
    end
end

_G.useParamsProjLib = true
