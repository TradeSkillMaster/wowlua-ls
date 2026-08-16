-- Assigns `ns.FNF_Owner`'s fields via *funcall* self-fields, WITHOUT a local
-- `@class FNF_Owner` declaration and WITHOUT any typed/bare field for it. The coarse
-- scan routes `self.x = SomeCall()` through the funcall chain, recording only a
-- TableField *global* (not a `field_paths` entry) — so this file enters `FNF_Owner`'s
-- assigning-file index only through the `ws_globals` TableField pass in
-- `build_on_stubs::finish`. Without that pass the fields would stay a coarse placeholder
-- cross-file. The callee is a file-local `@return`-annotated function, which the coarse
-- cross-file scan doesn't follow but the on-demand whole-file harvest resolves.

local addonName, ns = ...

---@return FNF_Widget
local function MakeWidget()
    return ns.FNF_Widget
end

local function ComputeCount()
    return 10 + 5
end

function ns.FNF_Owner:Build()
    self.widget = MakeWidget()   -- funcall -> coarse placeholder -> FNF_Widget
    self.count = ComputeCount()  -- funcall -> coarse placeholder -> number
end
