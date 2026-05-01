-- Cross-file overlay test: defines methods on a @type-annotated local
-- whose variable name matches the class name (mirrors the common pattern:
-- ---@type MyClass \n local MyClass = ns.MyClass \n function MyClass:Method() end)
---@type MyAddon
local MyAddon = {}
function MyAddon:ExtraMethod()
    return "extra"
end
MyAddon.extraField = true
