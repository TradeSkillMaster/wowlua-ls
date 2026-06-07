---@diagnostic disable: unused-local
-- Dynamic global prefix test: usage file
-- Reads from globals created by _G["PREFIX"..k] in defs.lua.
-- The workspace scanner detects the prefix pattern and allows these reads.

-- Prefix pattern reads — should NOT produce undefined-global
local none = MYADDON_L_NONE
--    ^ hover: (local) none: ?  def: local
local ok_val = MYADDON_L_OK
--    ^ hover: (local) ok_val: ?  def: local

-- Suffix pattern reads — should NOT produce undefined-global
local h1 = Click_HANDLER
--    ^ hover: (local) h1: ?  def: local
