-- Test: framexml: false config disables FrameXML globals
local function _consume(...) end

-- FrameXML global should be undefined when framexml is disabled
_consume(MouseIsOver)
--       ^ diag: undefined-global

-- Core WoW API globals (non-FrameXML) should still be available
_consume(CreateFrame)
--       ^ diag: none
