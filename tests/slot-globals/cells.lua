---@diagnostic disable: create-global
-- A mixin global this file assigns another global's table: that table, not this
-- global's own.
---@class JournalBaseCellMixin
JournalBaseCellMixin = {}

function JournalBaseCellMixin:Reset() end

JournalCellMixin = {}

function JournalCellMixin:Init() end

if JournalModern then
    JournalCellMixin = JournalBaseCellMixin
end
