---@diagnostic disable: unused-local, create-global

-- Declare the registry and its events. The receiver `addonTable.CallbackRegistry`
-- and the `addonTable.Constants.Events` reference are canonicalized so the consumer
-- sites in user.lua match cross-file.
local _, addonTable = ...

addonTable.CallbackRegistry = CreateFromMixins(CallbackRegistryMixin)
addonTable.CallbackRegistry:GenerateCallbackEvents(addonTable.Constants.Events)
