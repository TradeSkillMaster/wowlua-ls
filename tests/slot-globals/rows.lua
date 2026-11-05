---@diagnostic disable: create-global
-- A mixin global one file builds and another assigns another mixin, as two
-- flavor files do.
---@class JournalBaseRowMixin
JournalBaseRowMixin = {}

function JournalBaseRowMixin:Reset() end

JournalRowMixin = {}

function JournalRowMixin:Init() end
