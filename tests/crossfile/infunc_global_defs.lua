-- Cross-file test: a global created via a plain `X = ...` assignment *inside a
-- function body* must be registered so reads elsewhere don't false-positive as
-- undefined-global. Mirrors the saved-variable pattern where a global is only
-- ever initialized inside an event/initializer function.
local private = select(2, ...)

local function InitializeData()
  if MY_SAVED_DATA == nil then
    MY_SAVED_DATA = { version = 1 }
  end

  -- A function-scoped local reassigned inside the body must NOT leak as a
  -- global: the coarse scan recognizes `scratch` as a local declared in this
  -- file and skips the bare reassignment.
  local scratch = 0
  for i = 1, 3 do
    scratch = scratch + i
  end

  -- An explicit `_G.EXPLICIT_GLOBAL = ...` write must register the global even
  -- though `EXPLICIT_GLOBAL` is also declared as a local elsewhere in this file.
  -- This tests the `was_g_redirect` bypass of the local-name check.
  _G.EXPLICIT_GLOBAL = "from-init"

  return scratch
end

local EXPLICIT_GLOBAL = "local-only" ---@diagnostic disable-line: unused-local

private.Init = InitializeData
