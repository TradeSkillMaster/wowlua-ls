-- A workspace global function the addon marks `@deprecated`. Registered as a
-- cross-file (external-space) workspace function, NOT a WoW API stub, so the
-- flavor-aware suppression must not apply to it.

---@deprecated
---@return number
function GlobalOldHelper()
  return 1
end
