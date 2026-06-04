---@diagnostic disable: unused-local
-- Regression: file-level return { ... } should lower expressions
-- so that function parameters, table constructors, and nested
-- scopes are all properly registered in the IR.

---@param x number
---@return number
local function helper(x) return x + 1 end

return {
    name = "test",
    ---@param ctx string
    run = function(ctx)
        local y = ctx
--            ^ hover: (local) y: string  def: local
--                ^ hover: (param) ctx: string  def: local
    end,
    compute = function(a, b)
        local sum = a + b
--              ^ hover: (local) sum: ?  def: local
        return sum
    end,
    nested = {
        inner = function()
            local z = helper(1)
--                ^ hover: (local) z: number  def: local
        end,
    },
}
