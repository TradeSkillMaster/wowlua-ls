---@diagnostic disable: unused-local, inject-field
-- Base mixin defined the WoW way: a bare table with methods + runtime self-fields.
-- It is NOT a `@class` and is NOT used as an XML mixin, so the cross-file scan
-- registers it as a plain (non-class) table — the regression case for
-- `apply_mixin_parent_inheritance` resolving a non-class-table parent.
BaseViewMixin = {}

function BaseViewMixin:OnLoad()
  self.lastCharacter = "player"
  self.isLive = true
end

function BaseViewMixin:Refresh()
  return self.lastCharacter, self.isLive
end

-- A second base for the multi-mixin (`CreateFromMixins(A, B)`) case.
ExtraMixin = {}

function ExtraMixin:Extend()
  self.extended = true
end
