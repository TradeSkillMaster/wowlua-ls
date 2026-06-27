---@meta _
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-addon-3-0)

--- The library object returned by `LibStub("AceAddon-3.0")`. It carries the
--- addon-creation methods (`NewAddon`/`GetAddon`/…) directly, and inherits the
--- embeddable prototype (`NewModule`, `GetModule`, `Enable`, …) from `AceAddon`
--- so the common convention `---@class MyAddon : AceAddon-3.0` resolves those
--- methods on the addon object.
---@class AceAddon-3.0 : AceAddon
local AceAddonLib = {}

---@generic T: AceAddon
---@defclass T : AceAddon
---@overload fun(self, object: table, name: `T`, ...: string): T
---@param name `T`
---@param ... string @ Ace library names to embed
---@return T
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-addon-3-0#title-2)
function AceAddonLib:NewAddon(name, ...) end

---@generic T: AceAddon
---@overload fun(self, name: `T`, silent: boolean): T?
---@param name `T`
---@return T
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-addon-3-0#title-3)
function AceAddonLib:GetAddon(name) end

---@return fun(): string, AceAddon
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-addon-3-0#title-4)
function AceAddonLib:IterateAddons() end

---@return fun(): string, table
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-addon-3-0#title-5)
function AceAddonLib:IterateAddonStatus() end

---@param addon AceAddon
---@return fun(): string, table
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-addon-3-0#title-6)
function AceAddonLib:IterateEmbedsOnAddon(addon) end

---@param addon AceAddon
---@return fun(): string, AceAddon
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-addon-3-0#title-7)
function AceAddonLib:IterateModulesOfAddon(addon) end

---@class AceAddon
---@field name string
---@field moduleName? string
---@field modules table<string, AceAddon>
---@field orderedModules AceAddon[]
---@field defaultModuleLibraries string[]
---@field enabledState boolean
---@field baseName? string
local AceAddon = {}

---@generic T: AceAddon
---@defclass T : AceAddon
---@param name `T`
---@param prototype? table
---@param ... string @ Ace library names to embed
---@return T
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-addon-3-0#title-8)
function AceAddon:NewModule(name, prototype, ...) end

---@generic T: AceAddon
---@overload fun(self, name: `T`, silent: boolean): T?
---@param name `T`
---@return T
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-addon-3-0#title-9)
function AceAddon:GetModule(name) end

---@return fun(): string, AceAddon
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-addon-3-0#title-10)
function AceAddon:IterateModules() end

---@return string
function AceAddon:GetName() end

function AceAddon:Enable() end

function AceAddon:Disable() end

---@param name string
---@return boolean
function AceAddon:EnableModule(name) end

---@param name string
---@return boolean
function AceAddon:DisableModule(name) end

---@return boolean
function AceAddon:IsEnabled() end

---@return boolean
function AceAddon:IsModule() end

---@param state boolean
function AceAddon:SetEnabledState(state) end

---@param state boolean
function AceAddon:SetDefaultModuleState(state) end

---@param prototype table
function AceAddon:SetDefaultModulePrototype(prototype) end

---@param ... string @ Ace library names
function AceAddon:SetDefaultModuleLibraries(...) end
