---@diagnostic disable: create-global, unused-local
-- A field written calls in several methods (or branches) reads in other files
-- (frames_user.lua) as this file reads it: the calls' type when they agree, the
-- callee's when they don't.
---@class PanelHost
PanelHost = {}

function PanelHost:Init(compact)
    self.frame = CreateFrame("Frame", nil, UIParent, "BackdropTemplate")
    self.pair = CreateFrame("Frame", nil, UIParent, "BackdropTemplate")
    if compact then
        self.alt = CreateFrame("Frame", nil, UIParent, "BackdropTemplate")
    else
        self.alt = CreateFrame("Frame", nil, UIParent, "UIPanelDialogTemplate")
    end
end

function PanelHost:Reset()
    self.frame = CreateFrame("Frame", nil, UIParent, "UIPanelDialogTemplate")
    self.pair = CreateFrame("Frame", nil, UIParent, "BackdropTemplate")
end

function PanelHost:Use()
    local frame = self.frame
    --    ^ hover: (local) frame: Frame {
    local pair = self.pair
    --    ^ hover: (local) pair: BackdropTemplate {
    local alt = self.alt
    --    ^ hover: (local) alt: Frame {
end
