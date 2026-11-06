---@diagnostic disable: unused-local
LedgerDerivedViewMixin = CreateFromMixins(LedgerViewMixin)

function LedgerDerivedViewMixin:Refresh()
    local width = self.buttonsWidth
    --    ^ hover: (local) width: number
    local header = self.Header
    --    ^ hover: (local) header: FontString & GameFontHighlight
    local selected = self.selected
    --    ^ hover: (local) selected: string
end
