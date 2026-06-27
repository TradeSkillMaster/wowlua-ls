---@meta _
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-locale-3-0)

--- AceLocale-3.0 manages localization for addons: locale files register their
--- strings against an application name, and consumers fetch the active locale
--- table back. Accessed via `LibStub("AceLocale-3.0")`.
---@class AceLocale-3.0
local AceLocale = {}

--- Register a new locale (or extend an existing one) for your addon.
---
--- When `isDefault` is `true` this registers the base/fallback locale and the
--- call **always returns a non-nil table** — this is the universal pattern in a
--- default-locale file (`local L = ...:NewLocale(addon, "enUS", true)` followed
--- by `L["key"] = true`). When `isDefault` is omitted or false, the call returns
--- the locale table only if `locale` matches the player's client locale and
--- `nil` otherwise, so non-default locale files must nil-check the result.
---@overload fun(self, application: string, locale: string, isDefault: true, silent?: boolean): table
---@param application string @ Unique name of your addon (usually the addon folder name)
---@param locale string @ Locale code this table provides (e.g. "enUS", "deDE")
---@param isDefault? boolean @ Whether this is the default (fallback) locale
---@param silent? boolean @ If true, suppress errors on access to missing entries
---@return table? locale @ The locale table to populate, or nil when `locale` is not the active one
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-locale-3-0#title-2)
function AceLocale:NewLocale(application, locale, isDefault, silent) end

--- Get the localization table for your addon. Errors if no locale was registered,
--- unless `silent` is true.
---@param application string @ Unique name of your addon (usually the addon folder name)
---@param silent? boolean @ If true, return the table even when entries are missing instead of erroring
---@return table<string, string> locale @ The locale table for the active locale
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-locale-3-0#title-3)
function AceLocale:GetLocale(application, silent) end
