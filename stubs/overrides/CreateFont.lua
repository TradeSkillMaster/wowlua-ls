---@meta _
-- Override CreateFont / CreateFontFamily to mark them as creating a named global.
-- Both take the new font object's name as their first argument; calling them
-- registers that name as a global Font object (e.g. `CreateFont("MyFont")` makes
-- `_G.MyFont`). The `@creates-global 1` annotation drives this generally so no
-- function names are hard-coded in the language server: param 1's string literal
-- names the global, and its type is harvested from the call's `@return Font`.
-- Signatures mirror the generated Blizzard stubs; keep them in sync if upstream
-- adds parameters.

---[Documentation](https://warcraft.wiki.gg/wiki/API_CreateFont)
---@param name string
---@return Font fontObject
---@creates-global 1
function CreateFont(name) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CreateFontFamily)
---@param name string
---@param members CreateFontFamilyMemberInfo[]
---@return Font fontFamily
---@creates-global 1
function CreateFontFamily(name, members) end
