-- Cross-file addon namespace alias test: factory accessed through local alias
---@class NsAliasFactory
local AliasedFactory = {}

---@class NsAliasWidget
---@field label string

---@return NsAliasWidget
function AliasedFactory:CreateWidget()
    return {}
end

---@class NsAliasSubModule
local SubModule = {}

---@class NsAliasResult
---@field id number

---@return NsAliasResult
function SubModule:Run()
    return {}
end

AliasedFactory.Sub = SubModule

---@class NsAliasHost
local Host = {}

local addonName, ns = ...
ns.AliasedFactory = AliasedFactory
ns.NsAliasHost = Host
