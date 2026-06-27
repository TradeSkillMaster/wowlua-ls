-- Calls a cross-file workspace `@deprecated` global from another file. Even
-- under a Classic Era project, a workspace deprecation (not a WoW stub) must
-- still warn — `is_stub_function` keeps the flavor-aware suppression scoped to
-- WoW API stubs.
local _v = GlobalOldHelper()
--         ^ diag: deprecated
