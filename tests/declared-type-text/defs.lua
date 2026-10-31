-- A field write's own `---@type` shows as written in other files, on both
-- workspace builds.
---@diagnostic disable: create-global

---@alias DeclSlot "left" | "right"

DeclMod = {}

---@type table<DeclSlot, string>
DeclMod.SLOTS = {
    left = "l",
    right = "r",
}
