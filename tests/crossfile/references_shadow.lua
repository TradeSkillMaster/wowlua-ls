-- Cross-file references test: this file declares a file-local variable that
-- happens to share a name with the global `GlobalRefFn` defined in references_defs.lua.
-- find-references on the global should (permissively) include this token so the user sees
-- the name collision; rename, which passes `strict_shadow`, must NOT rewrite it.

local GlobalRefFn = 5
return GlobalRefFn
