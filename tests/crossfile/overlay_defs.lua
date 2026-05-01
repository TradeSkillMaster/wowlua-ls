-- Cross-file overlay test: defines fields on class variables and globals
---@class MyAddon
local MyAddon = {}
MyAddon.version = 1
MyAddon.name = "TestAddon"
function MyAddon:Run()
    return true
end

-- Field assigned from a function call result (scanner can't resolve callee return type)
local function makeWidget() return {} end
MyAddon.widget = makeWidget()

-- Global table field assignment
GLOBAL_REGISTRY = {}
GLOBAL_REGISTRY.count = 42
GLOBAL_REGISTRY.label = "test"
function GLOBAL_REGISTRY:Reset()
    return 0
end
