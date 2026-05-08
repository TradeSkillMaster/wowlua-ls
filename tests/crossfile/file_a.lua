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
---@return table<string,string>
function Locale.GetTable()
    return {}
end
---@class MyComponent
local MyComponent = {}
ns.MyComponent = MyComponent
ns.MyComponent.active = true
-- Method chain: first_string_arg should be "ChainApp" not "MyLib"
ns.ChainApp = ns.Lib.NewComponent("ChainApp"):AddDependency("MyLib")
-- Void method (implicit nil return)
function ns:Reset()
    print("reset")
end
ns.ChainApp.Locale = ns.Locale
