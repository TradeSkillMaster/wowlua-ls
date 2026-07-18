---@diagnostic disable: unused-local
-- AddonU: `@class`-annotated namespace whose `Util` field is a differently-typed
-- `@class` local than AddonV's same-named `Util` field. The field type must not
-- leak across addon roots.
---@class U_NS
local _, ns = ...
---@class U_Util
local Util = {}
-- Assigning the class-typed local to the namespace field must NOT flag
-- field-type-mismatch (it would if a sibling addon's `V_Util` leaked in as the
-- expected type of `U_NS.Util`). This write is diagnostic-checked by the
-- `..._shared_u` test that targets this file.
ns.Util = Util
-- ^ hover: (field) Util: U_Util
function Util:UMethod() end

---@class U_Thing
local UThing = {}

-- A namespace *method* carrying a `@return`. Its field type is a function; the
-- per-addon type-isolation pass must NOT re-type the field to its RETURN type
-- (`U_Thing`) — `Method`-kind globals carry the method's return type in
-- `returns`, not the field type, so they are excluded from that pass.
---@return U_Thing
function ns:GetThing() return UThing end
