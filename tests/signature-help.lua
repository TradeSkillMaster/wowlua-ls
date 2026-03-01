-- Test: signature help

---@param x number
---@param y string
---@return boolean
local function foo(x, y)
    return true
end

foo(1, "hello")

-- Test overloads
---@overload fun(a: string): string
---@overload fun(a: number, b: number): number
---@param a string
---@param b string
---@param c string
---@return boolean
local function bar(a, b, c)
    return true
end

bar("hello")
bar(1, 2)
bar("a", "b", "c")

-- Test method calls
---@type Button
local btn = nil
btn:GetText()

-- Test global function
local t = {}
table.insert(t, "hello")
