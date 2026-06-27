---@diagnostic disable: unused-local, unused-function

---@class Module
local M = {}

-- A workspace @class reusing the built-in stub class name `CVarInfo` with its own
-- explicit @field contract (a fresh record that collides with a built-in). The
-- warning fires here.
---@class CVarInfo
-- ^ diag: class-shadows-builtin
---@field label string
---@field quality string

-- Field-on-a-@class-table use of the colliding class. `@type table<K,V>` on a
-- class-table field resolves `CVarInfo` against the EXTERNAL class set (not the
-- per-file local @class), where the stub's fields are additively merged in. So
-- this exercises the missing-fields contract scoping: the constructors below must
-- be checked against the workspace's declared fields {label, quality}, NOT the
-- stub CVarInfo's required fields — otherwise `missing-fields` false-fires.
---@type table<string, CVarInfo>
M.CVARS = {
  shadowQuality = { label = "Shadow Quality", quality = "3" },
  liquidDetail = { label = "Liquid Detail", quality = "5" },
}

return M
