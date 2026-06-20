---@diagnostic disable: unused-local, unused-function, missing-return, create-global

-- Library: defines a module base with @protected lifecycle methods and a
-- global @defclass factory that creates modules inheriting that base. This
-- mirrors a real addon's module-loader pattern, where each module file calls
-- the factory at file scope and registers its own protected load handlers.

---@class ProtoModuleBase
local MODULE_METHODS = {}

---Registers the function to be called when the module is loaded.
---@protected
---@param func fun() The function to call
function MODULE_METHODS:OnModuleLoad(func) end

---Registers the function to be called when the module is unloaded.
---@protected
---@param func fun() The function to call
function MODULE_METHODS:OnModuleUnload(func) end

---Creates a new module that inherits the base's protected lifecycle methods.
---@generic T: ProtoModuleBase
---@defclass T
---@param path `T` The module path
---@return T
function NewProtoModule(path)
    return MODULE_METHODS
end
