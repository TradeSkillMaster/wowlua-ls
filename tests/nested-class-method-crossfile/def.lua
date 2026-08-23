-- Regression: a method defined via `function <@class-typed-local>:Method()` INSIDE a
-- function body must join the class's cross-file method set. `collect_statements_recursive`
-- (which drives the coarse scan) does not descend into function bodies, so before the fix
-- such methods were harvested only onto the per-file live table (build_ir), never the
-- external class table a `table<K, V>` / class-field receiver resolves to — producing
-- `undefined-field` false positives at such call sites (same-file here; cross-file in
-- use.lua).
---@diagnostic disable: unused-local, unused-function, redefined-local, shadowed-local

--- @class NCM_Manager
local Manager = {}

--- @type table<Frame, NCM_Widget>
Manager.widgets = {}

function Manager:Process(frame)
    -- `self.widgets[frame]` is NCM_Widget via the class-field `table<K, V>`; its method
    -- resolves to the definition harvested from inside `InitWidget` below (no undefined-field).
    self.widgets[frame]:Configure(1)
    --                  ^ def: external
    --                  ^ hover: (method) function NCM_Widget:Configure(configId)
    local id = self.widgets[frame]:GetConfiguredId()
end

--- @param widget NCM_Widget
function Manager:InitWidget(widget)
    --- @class NCM_Widget : Frame
    local widget = widget
    function widget:Configure(configId)
        self.configId = configId
    end
    function widget:GetConfiguredId()
        return self.configId
    end
end
