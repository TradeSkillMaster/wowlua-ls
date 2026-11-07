-- A class's only Lua file writes members onto a field whose constructor entry
-- only declares it, onto one a statement initializes to nil, onto one the constructor
-- gives a value, and (another class) onto one through each of two declarations of the
-- class: the file reads each as other files do.
---@class ClassWriterPeerPanel
local Panel = { header = nil, footer = CreateFrame("Frame") }
_G.ClassWriterPeerPanel = Panel
Panel.body = nil

function Panel:Init()
    local header = CreateFrame("Frame")
    header.extra = 5
    self.header = header
    local body = CreateFrame("Frame")
    body.rows = 2
    self.body = body
    local footer = CreateFrame("Frame")
    footer.note = "x"
    self.footer = footer
end

function Panel:Footer()
    local footer = self.footer
    --    ^ hover: (local) footer: Frame & { note: string } {
    return footer.note
end

---@class ClassWriterPeerSheet
local Sheet = {}
_G.ClassWriterPeerSheet = Sheet

function Sheet:Init()
    local title = CreateFrame("Frame")
    title.size = 12
    self.title = title
end

---@class ClassWriterPeerSheet
local Builder = {}

function Builder:Build()
    local title = CreateFrame("Frame")
    title.text = "x"
    self.title = title
end

function Builder:Title()
    local title = self.title
    --    ^ hover: (local) title: Frame & { size: number, text: string } {
    return title.size
end
