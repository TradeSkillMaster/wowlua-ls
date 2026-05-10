-- Regression: file-level return with ---@type should produce
-- assign-type-mismatch and missing-fields diagnostics.

---@class FileRetTestPlugin
---@field code string
---@field run fun(ctx: string)

return { ---@type FileRetTestPlugin
-- ^ diag: missing-fields  diag: assign-type-mismatch
    codee = "test",
    run = function(ctx) end,
}
