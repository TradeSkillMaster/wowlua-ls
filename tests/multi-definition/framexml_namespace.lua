-- A workspace file that redefines a method on a stub *namespace* table
-- (`Settings` — a plain scope-0 global table, NOT a @class, present in the
-- built-in stubs). This is the `library` overlap case for namespace tables:
-- unless the workspace method merges onto the real stub table it lands on an
-- orphaned shadow table that nothing resolves the name to, and go-to-definition
-- on `Settings.RegisterVerticalLayoutCategory` would only ever offer the stub
-- site. The assertion lives in user.lua.

function Settings.RegisterVerticalLayoutCategory(name)
    return name
end
