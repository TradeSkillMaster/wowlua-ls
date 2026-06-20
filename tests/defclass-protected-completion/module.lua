---@diagnostic disable: unused-local, unused-function

-- A module file: the defclass-created instance lives at file scope, so the
-- module file *is* the class's own implementation. Completing a colon access
-- must therefore offer the inherited @protected lifecycle methods, matching the
-- access.rs allowance that lets these calls compile without an access error.

local Vendor = NewProtoModule("VendorModule")

---A public method defined on the module via dot syntax.
function Vendor.DoStuff() end

-- Colon completion offers the inherited @protected methods at file scope.
Vendor:OnModuleLoad(function() end)
--             ^ comp: OnModuleLoad, OnModuleUnload

-- A discriminating prefix narrows to just the load handler.
Vendor:OnModuleLoad(function() end)
--              ^ comp: OnModuleLoad
