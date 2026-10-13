-- A name argument that is neither a literal nor the addon's `...` leaves the class
-- to the call: the library name after it is a mixin the table-first overload's
-- name position mistakes for the class, so the fields its methods write don't
-- land on that shared library class.
local NAME = "MixinNamedAddon"
local Mixed = LibStub("AceAddon-3.0"):NewAddon(NAME, "AceEvent-3.0")

function Mixed:OnInitialize()
    self.secretList = {}
end
