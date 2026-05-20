-- Test: signature help

---@param x number
---@param y string
---@return boolean
local function foo(x, y)
    return true
end

foo(1, "hello")
--  ^ sig: fun(x: number, y: string): boolean

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
--  ^ sig: fun(a: string): string
bar(1, 2)
--  ^ sig: fun(a: number, b: number): number
bar("a", "b", "c")
--  ^ sig: fun(a: string, b: string, c: string): boolean

-- Test method calls
---@type Button
local btn = nil
btn:GetText()
--          ^ sig: fun(): string

-- Test global function
local t = {}
table.insert(t, "hello")
--           ^ sig: fun(list: T[], value: T)
