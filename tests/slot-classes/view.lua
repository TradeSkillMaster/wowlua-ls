-- A plain mixin table an XML template names (`mixin="LedgerViewMixin"`): `self` in
-- its methods is the file's own table, which reads the template's `parentKey`
-- children off the class, and what its methods write there is read alike in the
-- files of mixins built from it.
LedgerViewMixin = {}

function LedgerViewMixin:OnLoad()
    self.buttonsWidth = 0
    self.selected = nil
end

function LedgerViewMixin:Layout(width)
    self.buttonsWidth = width + 10
    local header = self.Header
    --    ^ hover: (local) header: FontString & GameFontHighlight
    header:SetText("Ledger")
end

function LedgerViewMixin:Select(entry)
    self.selected = "entry"
    return entry
end
