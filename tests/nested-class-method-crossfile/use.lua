-- Cross-file half: NCM_Widget's methods are defined in def.lua inside a function body.
-- A consumer reaching an instance through a `table<K, V>` class-field must still resolve
-- them — they live on the external class table now (no undefined-field).
---@diagnostic disable: unused-local, unused-function

--- @class NCM_Consumer
local C = {}

--- @type table<Frame, NCM_Widget>
C.widgets = {}

function C:Use(frame)
    self.widgets[frame]:Configure(2)
    --                  ^ def: external
    --                  ^ hover: (method) function NCM_Widget:Configure(configId)
    self.widgets[frame]:GetConfiguredId()
end
