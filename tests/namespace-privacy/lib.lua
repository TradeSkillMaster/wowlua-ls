---@diagnostic disable: create-global, unused-local
-- Namespace-global privacy: a global @class table may initialize and use its
-- own private/protected fields at file scope in the file that DECLARES the
-- class. A local @type handle to the same class stays strict.

---@class PrivLib
---@field private handlers table<string, fun()>
---@field protected count number
PrivLib = PrivLib or {}

-- file-scope init idiom + file-scope read of the private/protected fields: clean
PrivLib.handlers = PrivLib.handlers or {}
PrivLib.count = PrivLib.count or 0
local _seed = PrivLib.count

-- a field whose visibility is declared INLINE on the assignment (the sugar),
-- not in the @class block: same behavior, still clean in the declaring file
---@private
PrivLib.token = PrivLib.token or ""
local _tok = PrivLib.token

-- same-class access inside a method: clean (already worked before this feature)
function PrivLib:Bump()
    self.count = self.count + 1
    self.handlers["x"] = function() end
end

-- a LOCAL @type handle is a consumer-style reference, even in this file → strict
---@type PrivLib
local h = {}
local _a = h.handlers
--           ^ diag: access-private
local _b = h.count
--           ^ diag: access-protected
local _c = h.token
--           ^ diag: access-private

-- A DIFFERENT class reached through a namespace global's field is not the root's
-- own class, so its private fields stay strict even in the declaring file.
---@class PrivInner
---@field private innerSecret number

---@class PrivOuter
---@field inner PrivInner
PrivOuter = PrivOuter or {}
local _in = PrivOuter.inner.innerSecret
--                          ^ diag: access-private

-- A GLOBAL handle whose name differs from the class it resolves to is a
-- consumer, even in the declaring file → its fields stay strict.
GlobalHandle = PrivLib
local _gh = GlobalHandle.count
--                       ^ diag: access-protected
