-- Calls a cross-file workspace `@deprecated` global from another file. Even
-- under a Classic Era project, a workspace deprecation (not a WoW stub) must
-- still warn — `is_stub_function` keeps the flavor-aware suppression scoped to
-- WoW API stubs.
local _v = GlobalOldHelper()
--         ^ diag: deprecated

-- The `@deprecated <message>` guidance survives cross-file into both the
-- caller's hover doc and the diagnostic message.
local _w = GlobalOldHelperMsg()
--         ^ diag: deprecated ~is deprecated: Use NewHelper instead
--         ^ doc: **Deprecated.** Use NewHelper instead
