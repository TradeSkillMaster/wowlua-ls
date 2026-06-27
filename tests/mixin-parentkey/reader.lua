---@diagnostic disable: unused-local
-- Cross-file reader: a different file references the derived mixin global and a
-- frame built from its template. The derived mixin's inheritance and parentKey
-- fields must resolve here too (the fix is in the cross-file build, not just the
-- defining file's per-file analysis).

local view = DerivedViewMixin
--    ^ hover: (local) view: DerivedViewMixin {

-- Inherited base method is visible cross-file.
view:Refresh()

-- parentKey child landed on the mixin class and resolves cross-file.
local c = view.Container
--    ^ hover: (local) c: Frame
