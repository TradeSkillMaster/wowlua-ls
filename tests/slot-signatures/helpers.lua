---@diagnostic disable: unused-local
-- Methods defined through another file's class: this file reads them as it
-- defines them, as other files do.
---@class SigHelpers
SigApp.Helpers = {
    Mail = {},
}

function SigApp.Helpers.Mail:SetRecipient(name)
    print(name .. "")
end

function SigApp.Helpers.Mail:Compose(text)
    return text .. "!"
end

local compose = SigApp.Helpers.Mail.Compose
--    ^ hover: (local) compose: fun(self: table, text: string | number): string
