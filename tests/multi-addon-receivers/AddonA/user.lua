-- A method defined through the namespace has this addon's namespace as its
-- `self`, never another addon's fields.
local _, ns = ...
local greet = ns.Greet
--    ^ hover: (local) function greet(self: {Greet: fun(self: table, name): any, value: number}, name)\n-> any
local function call(x)
    greet(x, "y")
    return x.other
    --       ^ hover: (field) other: ?
end
call(ns)
