-- A workspace file that re-annotates a mixin method already present in the
-- built-in stubs (here CallbackRegistryMixin, from
-- stubs/overrides/CallbackRegistryMixin.lua). This is the "library" overlap
-- case: a workspace/`library` file redefines a stubbed method, so
-- go-to-definition on the method should list both the stub site and this
-- workspace site. The assertion lives in user.lua.

---@class CallbackRegistryMixin
CallbackRegistryMixin = {}

---@param events string[]
function CallbackRegistryMixin:GenerateCallbackEvents(events) end
