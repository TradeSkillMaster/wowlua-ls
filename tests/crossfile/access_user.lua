-- Cross-file access modifier test: accesses private/protected fields from outside

---@type AccessWidget
local w = {}

-- Public field: OK
local n = w.name
--    ^ hover: (global) n: string  def: local

-- Public method call should produce no access diagnostic
_ = w:GetName()
-- ^ diag: none

-- Private field from outside: should error
local s = w._secret
--          ^ diag: access-private

-- Protected field from outside: should error
local i = w._internal
--          ^ diag: access-protected

-- Subclass type — but access is still from external file code, not from a method body
---@class AccessSubWidget : AccessWidget
---@field extra boolean

---@type AccessSubWidget
local sw = {}

-- Protected inherited field: still protected when accessed from outside the class
local si = sw._internal
--            ^ diag: access-protected

-- Private inherited field: inaccessible
local ss = sw._secret
--            ^ diag: access-private
