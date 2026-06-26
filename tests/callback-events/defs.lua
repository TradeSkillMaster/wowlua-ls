---@diagnostic disable: unused-local

-- CallbackRegistryMixin:GenerateCallbackEvents (annotated @generates-events in
-- stubs/overrides/CallbackRegistryMixin.lua) synthesizes an `Event` enum table on
-- the receiver class, one `string` member per array entry. Entries may be string
-- literals or field references (whose leaf name is used).

local BaseScrollBoxEvents = { OnScroll = "OnScroll" }

ScrollBoxListMixin = CreateFromMixins(CallbackRegistryMixin);--- @class ScrollBoxListMixin : CallbackRegistryMixin

ScrollBoxListMixin:GenerateCallbackEvents(
    {
        BaseScrollBoxEvents.OnScroll,
        "OnDataProviderReassigned",
        "OnUpdate",
    }
);

-- Same-file access resolves to the synthesized members.
local a = ScrollBoxListMixin.Event.OnDataProviderReassigned
--                                 ^ hover: (field) OnDataProviderReassigned: string
local b = ScrollBoxListMixin.Event.OnScroll
--                                 ^ hover: (field) OnScroll: string
