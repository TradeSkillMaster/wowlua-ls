---@diagnostic disable: unused-local

-- `EnumUtil.MakeEnum` is `@returns-enum` in stubs/overrides/EnumUtil.lua: a call
-- is typed as its constructor `{ Idle = 1, Running = 2, Done = 3 }`.
local State = EnumUtil.MakeEnum("Idle", "Running", "Done")
--    ^ hover: (local) State: {\nDone: number,\nIdle: number,\nRunning: number\n}
local running = State.Running
--                    ^ hover: (field) Running: number  def: local 5:41  comp: Done, Idle, Running
local typo = State.Runing
--                 ^ diag: undefined-field

-- A non-literal argument: the declared `table`.
local names = { "Idle" }
local dynamic = EnumUtil.MakeEnum(unpack(names))
--    ^ hover: (local) dynamic: table

-- `@enum` names it, with the members' values in hover.
---@enum Pace
local Pace = EnumUtil.MakeEnum("Fast", "Slow")
--    ^ hover: (local) Pace: Pace {\nFast = 1,\nSlow = 2\n}
local slow = Pace.Slow
--                ^ hover: (field) Slow: number = 2  def: local 19:40

---@param pace Pace
local function setPace(pace) end
setPace(Pace.Fast)
setPace("fast")
--      ^ diag: type-mismatch

-- Blizzard's own `MakeEnum` enums carry their members in the stubs.
local right = MinimalSliderWithSteppersMixin.Label.Right
--    ^ hover: (local) right: number
local mapTag = MapPinTags.Event
--    ^ hover: (local) mapTag: number

-- Through a local alias the scan can't match the declared name, so the call's own
-- resolution fills the `@enum`.
local EU = EnumUtil
---@enum Speed
local Speed = EU.MakeEnum("Slow", "Fast")
local aliasSlow = Speed.Slow
--                      ^ hover: (field) Slow: number = 1  def: local 40:27
local aliasTypo = Speed.Slw
--                      ^ diag: undefined-field
