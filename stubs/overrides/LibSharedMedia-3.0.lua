---@meta _
-- Full replacement of Ketho's vendored LibSharedMedia-3.0 annotations. The only
-- substantive change vs upstream: `mediatype` is `string` (not the closed
-- `LibSharedMediaTypes` enum) on every method, and `:Register`'s `data` is `any`.
-- `:Register` is the API for *adding new* media types and the query methods must
-- look those custom types back up, so a closed enum is wrong there; custom types
-- also store arbitrary `data` (only the built-ins use a path string / FileID).
-- Same-stem override → replaces the vendor file 1:1 (no extra class overlay).
---[Documentation](https://www.wowace.com/projects/libsharedmedia-3-0/pages/api-documentation)
---@class LibSharedMedia-3.0
local LibSharedMedia = {}

LibSharedMedia.LOCALE_BIT_koKR = 1
LibSharedMedia.LOCALE_BIT_ruRU = 2
LibSharedMedia.LOCALE_BIT_zhCN = 4
LibSharedMedia.LOCALE_BIT_zhTW = 8
LibSharedMedia.LOCALE_BIT_western = 128

LibSharedMedia.MediaType = {
	BACKGROUND = "background",
	BORDER = "border",
	FONT = "font",
	STATUSBAR = "statusbar",
	SOUND = "sound",
}

-- Retained for reference (the built-in media types); the methods deliberately
-- take a plain `string` so custom types register and resolve cleanly.
---@alias LibSharedMediaTypes
---| "background" # Backgrounds
---| "border" # Borders
---| "font" # Fonts
---| "sound" # Sounds
---| "statusbar" # Statusbars

---@param mediatype string
---@param key string
---@param data any the data to associate with the handle; a filename, FileID, or custom value
---@param langmask? number
---[Documentation](https://www.wowace.com/projects/libsharedmedia-3-0/pages/api-documentation/)
function LibSharedMedia:Register(mediatype, key, data, langmask) end

---@param mediatype string
---@param key string
---@param noDefault? boolean
---@return string?
---[Documentation](https://www.wowace.com/projects/libsharedmedia-3-0/pages/api-documentation/)
function LibSharedMedia:Fetch(mediatype, key, noDefault) end

---@param mediatype string
---@param key? string
---[Documentation](https://www.wowace.com/projects/libsharedmedia-3-0/pages/api-documentation/)
function LibSharedMedia:IsValid(mediatype, key) end

---@param mediatype string
---@return table<string, string>
---[Documentation](https://www.wowace.com/projects/libsharedmedia-3-0/pages/api-documentation/)
function LibSharedMedia:HashTable(mediatype) end

---@param mediatype string
---@return string[]
---[Documentation](https://www.wowace.com/projects/libsharedmedia-3-0/pages/api-documentation/)
function LibSharedMedia:List(mediatype) end

---@param mediatype string
---@return string?
---[Documentation](https://www.wowace.com/projects/libsharedmedia-3-0/pages/api-documentation/)
function LibSharedMedia:GetGlobal(mediatype) end

---@param mediatype string
---@param key? string
---[Documentation](https://www.wowace.com/projects/libsharedmedia-3-0/pages/api-documentation/)
function LibSharedMedia:SetGlobal(mediatype, key) end

---@param mediatype string
---@return string?
---[Documentation](https://www.wowace.com/projects/libsharedmedia-3-0/pages/api-documentation/)
function LibSharedMedia:GetDefault(mediatype) end

---@param type string
---@param handle string
---[Documentation](https://www.wowace.com/projects/libsharedmedia-3-0/pages/api-documentation/)
function LibSharedMedia:SetDefault(type, handle) end
