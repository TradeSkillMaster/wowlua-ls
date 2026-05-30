-- Cross-file test: bare name of a local function from another file must be undefined
local private = select(2, ...)

-- The local function should NOT be visible as a bare global.
local x = FormatTexture("test")
--        ^ diag: undefined-global

-- But accessing through the namespace field should still work.
local _ = private.FormatTexture("test")
--                ^ hover: (field) function FormatTexture(name: string)\n  -> any  def: external
