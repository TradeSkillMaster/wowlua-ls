-- Cross-file test: file A defines globals on the addon namespace
local addonName, ns = ...
ns.version = 1
ns.title = "MyAddon"
ns.DB = {}
---@class MyLib
---@field enabled boolean
local MyLib = {}
function MyLib:GetName()
    return "lib"
end
ns.Lib = MyLib
function ns:Init()
    return true
end
function ns.DB:Start()
    return 0
end
local Locale = {}
ns.Locale = Locale
---@class MyComponent
local MyComponent = {}
ns.MyComponent = MyComponent
ns.MyComponent.active = true
