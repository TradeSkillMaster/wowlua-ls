---@meta _
-- Annotate EnumUtil.MakeEnum with @returns-enum. The function returns
-- `tInvert({...})`, so `EnumUtil.MakeEnum("Idle", "Running")` is the table
-- `{ Idle = 1, Running = 2 }`. With the annotation, each call whose arguments are
-- string literals is typed as that table, so its members resolve, complete, and
-- are checked, and `---@enum Name` above the call names it. Without it, the result
-- is the generated stub's bare `table`.

---@param ... string
---@return table
---@returns-enum
function EnumUtil.MakeEnum(...) end
