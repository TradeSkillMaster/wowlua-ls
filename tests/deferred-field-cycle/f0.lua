-- Regression fixture for the multi-file class-field harvest cycle guarded in
-- resolve_deferred_class_field_type (deferred.rs — see that comment for the mechanism).
-- `Obj` is a @class assigned across many sibling files that all read its coarse fields;
-- exercised by the terminates-regression test in wowlua_lsp main_loop tests.
---@class Obj
Obj = {}
