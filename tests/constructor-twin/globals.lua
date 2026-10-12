-- Constructors `user.lua` reads cross-file. A class-annotated constructor's
-- entries belong to the shared class, so a call entry must not retype the
-- class's declared field; nor may an entry that declares its own type.
---@type Config
NS.config = { handler = MakeThing() }

NS.DECLARED = {
    h = MakeThing(), ---@type any
    ---@type Widget
    ---@diagnostic disable-next-line: missing-fields
    w = {},
}

-- Entries `write.lua` replaces with a typed write.
NS.REPLACED = { A = MakeThing() }
G_REPLACED = { A = MakeThing() }

-- A global table's own constructor entries, top-level and nested.
G_ENTRIES = { A = MakeThing(), sub = { B = MakeThing() } }

-- A generic callee's generics are bound from the arguments the scan names.
NS.COPIES = { A = CopyOf(Factory) }
