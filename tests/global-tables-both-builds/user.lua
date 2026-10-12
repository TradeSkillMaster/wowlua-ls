---@diagnostic disable: unused-local

local wrap = PanelConstants.WrapText
--                          ^ hover: (field) WrapText: boolean  def: external
local padding = PanelConstants.Padding
--    ^ hover: (local) padding: number
local left = PanelConstants.Insets.Left
--    ^ hover: (local) left: number
local pending = PanelConstants.Pending
--    ^ hover: (local) pending: string
local formatter = PanelDefaults.Formatter
--    ^ hover: (local) formatter: PanelFormatter
local mode = PanelKeys.Mode
--    ^ hover: (local) mode: number
---@param key string
local function readKey(key)
    return PanelKeys[key]
end
local keyed = readKey("Label")
--    ^ hover: (local) keyed: any
local factionIndex = PanelFactions.Horde
--    ^ hover: (local) factionIndex: number
local factionName = PanelFactions[0]
--    ^ hover: (local) factionName: any
---@param names string[]
local function takeNames(names) return names end
takeNames(PanelOrder)
local order = PanelOrder
--    ^ hover: (local) order: table

local minInterval = PanelMixin.TimeFormatter:GetMinInterval(5)
--    ^ hover: (local) minInterval: number
local formatted = PanelMixin.TimeFormatter:Format(5)
--    ^ hover: (local) formatted: string
local flat = PanelMixin.GetMinInterval
--                      ^ diag: undefined-field
local short = PanelUtil.TimeFormatter:IsShort()
--    ^ hover: (local) short: boolean
local utilFormatted = PanelUtil.TimeFormatter:Format(5)
--    ^ hover: (local) utilFormatted: string
local found = PanelMixin.FoundFormatter
--    ^ hover: (local) found: PanelFormatter?
local span = PanelMixin.FoundFormatter:GetSpan()
--    ^ hover: (local) span: number
local foundFormatted = PanelMixin.FoundFormatter:Format(5)
--    ^ hover: (local) foundFormatted: string
local styled = PanelMixin.StyledFormatter:IsStyled()
--    ^ hover: (local) styled: boolean
local styledPadding = PanelMixin.StyledFormatter.padding
--    ^ hover: (local) styledPadding: number
local countFormatted = PanelMixin.CountFormatter:Format(5)
--    ^ hover: (local) countFormatted: string
local unitCount = PanelMixin.CountFormatter:GetUnitCount()
--    ^ hover: (local) unitCount: number
local plainCount = PanelUtil.CountFormatter
--    ^ hover: (local) plainCount: PanelFormatter
local cloned = PanelUtil.ClonedFormatter
--    ^ hover: (local) cloned: PanelFormatter
local presetFormatter = PanelPresets.Formatter
--    ^ hover: (local) presetFormatter: PanelFormatter
local copy = PanelUtil.Copy
--    ^ hover: (local) copy: table
