-- Cross-file half of the descendants-pass `has_prefix_expr_base` regression.
-- The fields written through a parenthesized prefix in defs.lua must NOT leak
-- as globals, so these bare reads stay `undefined-global`. Without the guard
-- in the scan_globals.rs descendants pass, defs.lua registers them as phantom
-- existence-only globals and both diagnostics silently disappear.
local function read()
    local a = inFuncField
    --        ^ diag: undefined-global
    local b = multiTargetField
    --        ^ diag: undefined-global
    return a, b
end
return read
