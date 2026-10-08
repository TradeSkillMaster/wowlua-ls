-- A workspace `CreateFromMixins` class is an open mixin, assignable to any table.
-- As a method receiver it must still only replace a callback's declared `self`
-- type when it nominally is that class or derives from it.
---@diagnostic disable: create-global, unused-local

---@class CallbackOwner

---@class HubBaseMixin
HubBaseMixin = {}
---@overload fun(self: HubBaseMixin, kind: "Changed", cb: fun(self: CallbackOwner))
---@overload fun(self: HubBaseMixin, kind: "Ready", cb: fun(self: HubBaseMixin))
---@param kind string
---@param cb function
function HubBaseMixin:Register(kind, cb) end

MyHubMixin = CreateFromMixins(HubBaseMixin)
function MyHubMixin:Init() end

local hub = CreateAndInitFromMixin(MyHubMixin)
hub:Register("Changed", function(owner)
    local o = owner
--        ^ hover: (local) o: CallbackOwner
end)
hub:Register("Ready", function(h)
    local r = h
--        ^ hover: (local) r: MyHubMixin
end)
