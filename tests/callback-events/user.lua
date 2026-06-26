---@diagnostic disable: unused-local

-- Cross-file access: the synthesized `Event` table flows to other files via the
-- ScrollBoxListMixin class, matching how addons reference callback events.
local function register(self)
    self:RegisterCallback(ScrollBoxListMixin.Event.OnDataProviderReassigned, self.OnChange, self)
    --                                              ^ hover: (field) OnDataProviderReassigned: string

    local evt = ScrollBoxListMixin.Event
    --                             ^ hover: (field) Event: {
end
