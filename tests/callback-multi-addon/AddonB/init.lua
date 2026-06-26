---@diagnostic disable: unused-local, create-global
local _, addonTable = ...
addonTable.CallbackRegistry = CreateFromMixins(CallbackRegistryMixin)
addonTable.CallbackRegistry:GenerateCallbackEvents({ "BetaEvent" })
