---@meta _

--- Hooks a secure function so that your hook is called with the same arguments
--- whenever the original function is called.
---@generic F: function
---@overload fun(name: `F`, hook: F)
---@param tbl table
---@param name string
---@param hook function
function hooksecurefunc(tbl, name, hook) end
