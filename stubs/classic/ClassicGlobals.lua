---@meta _
-- Classic-only WoW API stubs (auto-generated from warcraft.wiki.gg)

---[Documentation](https://warcraft.wiki.gg/wiki/API_AbandonQuest)
function AbandonQuest() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_AddQuestWatch)
---@param questIndex number
---@param watchTime? number
function AddQuestWatch(questIndex, watchTime) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_AddTrackedAchievement)
---@param achievementID number
function AddTrackedAchievement(achievementID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ArenaTeamRoster)
---@param index number
function ArenaTeamRoster(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_BNGetFriendGameAccountInfo)
---@param friendIndex any
---@return boolean hasFocus
---@return string characterName
---@return string client
---@return string realmName
---@return number realmID
---@return string faction
---@return string race
---@return string class
---@return string guild
---@return string zoneName
---@return string level
---@return string gameText
---@return string broadcastText
---@return number broadcastTime
---@return boolean canSoR
---@return number toonID
---@return number bnetIDAccount
---@return boolean isGameAFK
---@return boolean isGameBusy
function BNGetFriendGameAccountInfo(friendIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_BNGetFriendInfo)
---@param friendIndex any
---@return number bnetAccountID
---@return string accountName
---@return string battleTag
---@return any isBattleTagPresence
---@return string characterName
---@return number bnetIDGameAccount
---@return string client
---@return boolean isOnline
---@return number lastOnline
---@return boolean isAFK
---@return boolean isDND
---@return string messageText
---@return string noteText
---@return boolean isRIDFriend
---@return number messageTime
---@return boolean canSoR
---@return boolean isReferAFriend
---@return boolean canSummonFriend
function BNGetFriendInfo(friendIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_BNGetNumFriendGameAccounts)
---@param friendIndex number
---@return number numGameAccounts
function BNGetNumFriendGameAccounts(friendIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_BNSendGameData)
---@param gameAccountID number
---@param prefix string
---@param text string
function BNSendGameData(gameAccountID, prefix, text) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_BNSendWhisper)
---@param bnetAccountID number
---@param message string
function BNSendWhisper(bnetAccountID, message) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_BNSetAFK)
---@param bool boolean
function BNSetAFK(bool) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_BNSetCustomMessage)
---@param text string
function BNSetCustomMessage(text) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_BNSetDND)
---@param bool boolean
function BNSetDND(bool) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_BankButtonIDToInvSlotID)
---@param buttonID number
---@param isBag? number
---@return number invSlot
function BankButtonIDToInvSlotID(buttonID, isBag) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_BuyStableSlot)
function BuyStableSlot() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CanAbandonQuest)
---@param questID number
---@return boolean canAbandon
function CanAbandonQuest(questID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CanSendAuctionQuery)
---@return boolean canQuery
---@return boolean canQueryAll
function CanSendAuctionQuery() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CanUpgradeExpansion)
---@return boolean canUpgradeExpansion
function CanUpgradeExpansion() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CancelSell)
function CancelSell() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CancelTrackingBuff)
function CancelTrackingBuff() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CastGlyph)
---@param index number
function CastGlyph(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CastGlyphByID)
---@param spellID number
---@param slot number
function CastGlyphByID(spellID, slot) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CastGlyphByName)
---@param name string
---@param slot number
function CastGlyphByName(name, slot) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CastingInfo)
---@return string name
---@return string text
---@return number texture
---@return number startTime
---@return number endTime
---@return boolean isTradeSkill
---@return string castID
---@return boolean notInterruptible
---@return number spellID
function CastingInfo() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ChannelInfo)
---@return string name
---@return string text
---@return number texture
---@return number startTime
---@return number endTime
---@return boolean isTradeSkill
---@return boolean notInterruptible
---@return number spellID
function ChannelInfo() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ClickStablePet)
---@param index number
function ClickStablePet(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CloseAuctionHouse)
function CloseAuctionHouse() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CloseBankFrame)
function CloseBankFrame() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ClosePetStables)
function ClosePetStables() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CloseTradeSkill)
function CloseTradeSkill() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CollapseFactionHeader)
---@param rowIndex number
function CollapseFactionHeader(rowIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CollapseSkillHeader)
---@param index number
function CollapseSkillHeader(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CollapseTrainerSkillLine)
---@param index number
function CollapseTrainerSkillLine(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CombatLogAdvanceEntry)
---@param count number
---@param ignoreFilter? boolean
---@return boolean isValidIndex
function CombatLogAdvanceEntry(count, ignoreFilter) end

---@return number timestamp
---@return string subevent
---@return boolean hideCaster
---@return string sourceGUID
---@return string sourceName
---@return number sourceFlags
---@return number sourceRaidFlags
---@return string destGUID
---@return string destName
---@return number destFlags
---@return number destRaidFlags
---@return any ...
function CombatLogGetCurrentEntry() end

---@return number timestamp
---@return string subevent
---@return boolean hideCaster
---@return string sourceGUID
---@return string sourceName
---@return number sourceFlags
---@return number sourceRaidFlags
---@return string destGUID
---@return string destName
---@return number destFlags
---@return number destRaidFlags
---@return any ...
function CombatLogGetCurrentEventInfo() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CombatLogSetCurrentEntry)
---@param index number
---@param ignoreFilter? boolean
---@return boolean isValidIndex
function CombatLogSetCurrentEntry(index, ignoreFilter) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CombatTextSetActiveUnit)
---@param unit string
function CombatTextSetActiveUnit(unit) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ConfirmBarbersChoice)
function ConfirmBarbersChoice() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ConfirmBinder)
function ConfirmBinder() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ConfirmPetUnlearn)
function ConfirmPetUnlearn() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ConvertToParty)
function ConvertToParty() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CursorCanGoInSlot)
---@param invSlot number
---@return boolean fitsInSlot
function CursorCanGoInSlot(invSlot) end

---@param token EmoteToken
---@param unit? UnitToken
---@param hold? boolean
---@return boolean? restricted
function DoEmote(token, unit, hold) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_DoTradeSkill)
---@param index number
---@param repeat number
function DoTradeSkill(index, repeat) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_DoesSpellExist)
---@param spellName string
---@return boolean spellExists
function DoesSpellExist(spellName) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ExpandCurrencyList)
---@param id any
---@param expanded any
function ExpandCurrencyList(id, expanded) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ExpandFactionHeader)
---@param rowIndex number
function ExpandFactionHeader(rowIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ExpandSkillHeader)
---@param index number
function ExpandSkillHeader(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ExpandTradeSkillSubClass)
---@param index number
function ExpandTradeSkillSubClass(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ExpandTrainerSkillLine)
---@param index number
function ExpandTrainerSkillLine(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_FactionToggleAtWar)
---@param rowIndex number
function FactionToggleAtWar(rowIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_FillLocalizedClassList)
---@param tbl any
---@param isFemale? boolean
---@return any tbl
function FillLocalizedClassList(tbl, isFemale) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_FindBaseSpellByID)
---@param spellID number
---@return number baseSpellID
function FindBaseSpellByID(spellID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_FindSpellOverrideByID)
---@param spellID number
---@return number overrideSpellID
function FindSpellOverrideByID(spellID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetAbandonQuestName)
---@return string questName
function GetAbandonQuestName() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetArenaTeam)
---@param id any
---@return string teamName
---@return number teamSize
---@return number teamRating
---@return number weekPlayed
---@return number weekWins
---@return number seasonPlayed
---@return number seasonWins
---@return number playerPlayed
---@return number seasonPlayerPlayed
---@return number teamRank
---@return any playerRating
function GetArenaTeam(id) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetArenaTeamIndexBySize)
---@param size number
---@return number index
function GetArenaTeamIndexBySize(size) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetArenaTeamRosterInfo)
---@param teamindex number
---@param playerid any
---@return string name
---@return number rank
---@return number level
---@return string class
---@return number online
---@return number played
---@return number win
---@return number seasonPlayed
---@return number seasonWin
---@return number personalRating
function GetArenaTeamRosterInfo(teamindex, playerid) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetArmorPenetration)
---@return number armorPen
function GetArmorPenetration() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetAuctionItemBattlePetInfo)
---@param type string
---@param index number
---@return number creatureID
---@return number displayID
function GetAuctionItemBattlePetInfo(type, index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetAuctionItemInfo)
---@param type string
---@param index number
---@return string name
---@return number texture
---@return number count
---@return Enum.ItemQuality quality
---@return boolean canUse
---@return number level
---@return string levelColHeader
---@return number minBid
---@return number minIncrement
---@return number buyoutPrice
---@return number bidAmount
---@return string? highBidder
---@return string? bidderFullName
---@return string owner
---@return string? ownerFullName
---@return number saleStatus
---@return number itemId
---@return boolean hasAllInfo
function GetAuctionItemInfo(type, index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetAuctionItemLink)
---@param type string
---@param index number
---@return any itemLink
function GetAuctionItemLink(type, index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetAuctionItemSubClasses)
---@param classID number
---@return any subClass1
---@return any subClass2
---@return any subClass3
function GetAuctionItemSubClasses(classID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetAuctionItemTimeLeft)
---@param type any
---@param index any
---@return any timeleft
function GetAuctionItemTimeLeft(type, index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetAuctionSellItemInfo)
---@return any name
---@return any texture
---@return any count
---@return any quality
---@return any canUse
---@return any price
---@return any pricePerUnit
---@return any stackCount
---@return any totalCount
---@return any itemID
function GetAuctionSellItemInfo() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetBackpackCurrencyInfo)
---@param index number
---@return string name
---@return number count
---@return number icon
---@return number currencyID
function GetBackpackCurrencyInfo(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetBankSlotCost)
---@param numSlots number
---@return number cost
function GetBankSlotCost(numSlots) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetBattlefieldFlagPosition)
---@param index number
---@return number flagX
---@return number flagY
---@return string flagToken
function GetBattlefieldFlagPosition(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetBattlefieldInstanceInfo)
---@param index number
---@return number instanceID
function GetBattlefieldInstanceInfo(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetBattlefieldStatInfo)
---@param index number
---@return string name
---@return string icon
---@return string tooltip
function GetBattlefieldStatInfo(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetBattlegroundInfo)
---@param index number
---@return string localizedName
---@return boolean canEnter
---@return boolean isHoliday
---@return boolean isRandom
---@return number battleGroundID
---@return string mapDescription
---@return number bgInstanceID
---@return number maxPlayers
---@return string gameType
---@return number iconTexture
---@return string shortDescription
---@return string longDescription
---@return number hasControllingHoliday
function GetBattlegroundInfo(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCompanionCooldown)
---@param "type" any
---@param id any
---@return any startTime
---@return any duration
---@return any isEnabled
function GetCompanionCooldown("type", id) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftDescription)
---@param index number
---@return string craftDescription
function GetCraftDescription(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftDisplaySkillLine)
---@return string name
---@return number rank
---@return number maxRank
function GetCraftDisplaySkillLine() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftInfo)
---@param index number
---@return any craftName
---@return any craftSubSpellName
---@return string craftType
---@return any numAvailable
---@return any isExpanded
---@return any trainingPointCost
---@return any requiredLevel
function GetCraftInfo(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftItemLink)
---@param index number
---@return any itemLink
function GetCraftItemLink(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftName)
---@return string craftName
function GetCraftName() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftNumReagents)
---@param index any
---@return any numRequiredReagents
function GetCraftNumReagents(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftReagentInfo)
---@param index number
---@param n number
---@return any name
---@return any texturePath
---@return any numRequired
---@return any numHave
function GetCraftReagentInfo(index, n) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftReagentItemLink)
---@param index number
---@param n number
---@return string reagentLink
function GetCraftReagentItemLink(index, n) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftRecipeLink)
---@param index number
---@return string link
function GetCraftRecipeLink(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftSkillLine)
---@param n number
---@return string currentCraftingWindow
function GetCraftSkillLine(n) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftSpellFocus)
---@param index number
---@return any catalystName
---@return any number1
function GetCraftSpellFocus(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCurrencyInfo)
---@param id or "currencyLink" or "currencyString" any
---@return any name
---@return any currentAmount
---@return any texture
---@return any earnedThisWeek
---@return any weeklyMax
---@return any totalMax
---@return any isDiscovered
---@return any rarity
function GetCurrencyInfo(id or "currencyLink" or "currencyString") end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCurrencyLink)
---@param currencyID any
---@param currencyAmount any
---@return any currencyLink
function GetCurrencyLink(currencyID, currencyAmount) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCurrencyListInfo)
---@param index any
---@return any name
---@return any isHeader
---@return any isExpanded
---@return any isUnused
---@return any isWatched
---@return any count
---@return any icon
---@return any maximum
---@return any hasWeeklyLimit
---@return any currentWeeklyAmount
---@return any unknown
---@return any itemID
function GetCurrencyListInfo(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCurrencyListSize)
---@return any listSize
function GetCurrencyListSize() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCurrentCombatTextEventInfo)
---@return string,number? desc1
---@return string,number? desc2
function GetCurrentCombatTextEventInfo() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCurrentLevelSpells)
---@param level any
---@return any id
function GetCurrentLevelSpells(level) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCurrentResolution)
---@return any index
function GetCurrentResolution() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetDeathRecapLink)
---@param recapID number
---@return string recapLink
function GetDeathRecapLink(recapID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetEclipseDirection)
---@return any direction
function GetEclipseDirection() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetFactionInfo)
---@param factionIndex any
---@return string name
---@return string description
---@return number standingID
---@return number barMin
---@return number barMax
---@return number barValue
---@return boolean atWarWith
---@return boolean canToggleAtWar
---@return boolean isHeader
---@return boolean isCollapsed
---@return boolean hasRep
---@return boolean isWatched
---@return boolean isChild
---@return number factionID
---@return boolean hasBonusRepGain
---@return any canBeLFGBonus
function GetFactionInfo(factionIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetFirstBagBankSlotIndex)
---@return number index
function GetFirstBagBankSlotIndex() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetFirstTradeSkill)
---@return number skillId
function GetFirstTradeSkill() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetGlyphClearInfo)
---@return string name
---@return number count
---@return number icon
---@return number spellId
---@return number cost
function GetGlyphClearInfo() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetGlyphInfo)
---@param index number
---@return string name
---@return number glyphType
---@return boolean isKnown
---@return number icon
---@return number glyphID
---@return string glyphLink
function GetGlyphInfo(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetGlyphSocketInfo)
---@param socketID number
---@param talentGroup? number
---@return boolean enabled
---@return number glyphType
---@return number glyphIndex
---@return number? glyphSpellID
---@return number? iconFile
---@return number glyphID
function GetGlyphSocketInfo(socketID, talentGroup) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetGuildFactionInfo)
---@return string guildName
---@return string description
---@return number standingID
---@return number barMin
---@return number barMax
---@return number barValue
function GetGuildFactionInfo() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetInspectPVPRankProgress)
---@return number rankProgress
function GetInspectPVPRankProgress() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetInventoryItemGems)
---@param invSlot any
---@return any gem1
---@return any gem2
function GetInventoryItemGems(invSlot) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetItemStats)
---@param itemLink any
---@param statTable? any
---@return any stats
function GetItemStats(itemLink, statTable) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNextStableSlotCost)
---@return number nextSlotCost
function GetNextStableSlotCost() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumAuctionItems)
---@param list any
---@return any batch
---@return any count
function GetNumAuctionItems(list) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumBankSlots)
---@return number numSlots
---@return boolean full
function GetNumBankSlots() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumBattlefieldStats)
---@return number numStats
function GetNumBattlefieldStats() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumBattlefields)
---@return number numBattlefields
function GetNumBattlefields() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumCrafts)
---@return any numberOfCrafts
function GetNumCrafts() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumFactions)
---@return number numFactions
function GetNumFactions() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumGlyphSockets)
---@return number numGlyphSockets
function GetNumGlyphSockets() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumGlyphs)
---@return number numGlyphs
function GetNumGlyphs() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumQuestLogEntries)
---@return number numEntries
---@return number numQuests
function GetNumQuestLogEntries() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumQuestLogRewardCurrencies)
---@param questID? number
---@return number numCurrencies
function GetNumQuestLogRewardCurrencies(questID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumQuestWatches)
---@return number numWatches
function GetNumQuestWatches() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumRewardCurrencies)
---@return number numCurrencies
function GetNumRewardCurrencies() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumSkillLines)
---@return number numSkills
function GetNumSkillLines() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumSpellTabs)
---@return number numTabs
function GetNumSpellTabs() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumStableSlots)
---@return number numSlots
function GetNumStableSlots() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumTalentGroups)
---@param isInspect? boolean
---@return number num
function GetNumTalentGroups(isInspect) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumTalentTabs)
---@param isInspect? boolean
---@return number numTabs
function GetNumTalentTabs(isInspect) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumTalents)
---@param tabIndex number
---@return number numTalents
function GetNumTalents(tabIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumTrackedAchievements)
---@return number numTracked
function GetNumTrackedAchievements() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumTradeSkills)
---@return number numSkills
function GetNumTradeSkills() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetOwnerAuctionItems)
function GetOwnerAuctionItems() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetPVPLastWeekStats)
---@return number hk
---@return number dk
---@return number contribution
---@return number rank
function GetPVPLastWeekStats() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetPVPRankInfo)
---@param rankID number
---@param faction? number
---@return string rankName
---@return number rankNumber
function GetPVPRankInfo(rankID, faction) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetPVPRankProgress)
---@return number progress
function GetPVPRankProgress() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetPVPThisWeekStats)
---@return number hk
---@return number contribution
function GetPVPThisWeekStats() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetPendingGlyphInfo)
---@return any newGlyphName
function GetPendingGlyphInfo() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetPetHappiness)
---@return number happiness
---@return number damagePercentage
---@return number loyaltyRate
function GetPetHappiness() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetPetLoyalty)
---@return string petLoyaltyText
function GetPetLoyalty() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetPetTrainingPoints)
---@return number totalPoints
---@return number spent
function GetPetTrainingPoints() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestCurrencyInfo)
---@param itemType string
---@param index number
---@return string name
---@return string texture
---@return number numItems
---@return number quality
function GetQuestCurrencyInfo(itemType, index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestGreenRange)
---@return number range
function GetQuestGreenRange() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestIndexForTimer)
---@param timerId number
---@return number questIndex
function GetQuestIndexForTimer(timerId) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestIndexForWatch)
---@param watchIndex number
---@return number questIndex
function GetQuestIndexForWatch(watchIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestLogGroupNum)
---@param questID any
---@return number suggestedGroup
function GetQuestLogGroupNum(questID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestLogIndexByID)
---@param questID number
---@return number questLogIndex
function GetQuestLogIndexByID(questID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestLogPushable)
---@return boolean isPushable
function GetQuestLogPushable() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestLogRewardCurrencyInfo)
---@param index number
---@param questId? number
---@return string name
---@return string texture
---@return number numItems
---@return number currencyId
---@return number quality
function GetQuestLogRewardCurrencyInfo(index, questId) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestLogSelection)
---@return any questSelected
function GetQuestLogSelection() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestLogTitle)
---@param questLogIndex number
---@return string title
---@return number level
---@return number suggestedGroup
---@return boolean isHeader
---@return boolean isCollapsed
---@return number isComplete
---@return number frequency
---@return number questID
---@return boolean startEvent
---@return boolean displayQuestID
---@return boolean isOnMap
---@return boolean hasLocalPOI
---@return boolean isTask
---@return boolean isBounty
---@return boolean isStory
---@return boolean isHidden
---@return boolean isScaling
function GetQuestLogTitle(questLogIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestTagInfo)
---@param questID number
---@return number tagID
---@return string tagName
---@return number worldQuestType
---@return number rarity
---@return boolean isElite
---@return any tradeskillLineIndex
---@return any displayTimeLeft
function GetQuestTagInfo(questID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestTimers)
---@return any questTimers
function GetQuestTimers() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestsCompleted)
---@param table? table
---@return table questsCompleted
function GetQuestsCompleted(table) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetRuneType)
---@param id any
---@return any runeType
function GetRuneType(id) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetScreenResolutions)
---@return any resolution1
---@return any resolution2
---@return any resolution3
function GetScreenResolutions() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSelectedBattlefield)
---@return number selectedIndex
function GetSelectedBattlefield() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSelectedGlyphSpellIndex)
---@return number? selectedIndex
function GetSelectedGlyphSpellIndex() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSelectedSkill)
---@return number skillIndex
function GetSelectedSkill() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSelectedStablePet)
---@return number selectedPet
function GetSelectedStablePet() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSkillLineInfo)
---@param index number
---@return string skillName
---@return number header
---@return number isExpanded
---@return number skillRank
---@return number numTempPoints
---@return number skillModifier
---@return number skillMaxRank
---@return number isAbandonable
---@return number stepCost
---@return number rankCost
---@return number minLevel
---@return number skillCostType
---@return string skillDescription
function GetSkillLineInfo(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellAutocast)
---@param "spellName" or spellId any
---@param bookType string
---@return number autocastable
---@return number autostate
function GetSpellAutocast("spellName" or spellId, bookType) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellBookItemInfo)
---@param spellName any
---@return string spellType
---@return number id
function GetSpellBookItemInfo(spellName) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellBookItemName)
---@param spellName any
---@return string spellName
---@return string spellSubName
---@return number spellID
function GetSpellBookItemName(spellName) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellBookItemTexture)
---@param spell any
---@return number icon
function GetSpellBookItemTexture(spell) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellCharges)
---@param spell any
---@return number currentCharges
---@return number maxCharges
---@return number cooldownStart
---@return number cooldownDuration
---@return number chargeModRate
function GetSpellCharges(spell) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellCount)
---@param spell any
---@return number numCasts
function GetSpellCount(spell) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellDescription)
---@param spellID number
---@return string desc
function GetSpellDescription(spellID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellInfo)
---@param spell any
---@return string name
---@return string subtext
---@return number icon
---@return number castTime
---@return number minRange
---@return number maxRange
---@return number spellID
---@return number originalIcon
function GetSpellInfo(spell) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellLevelLearned)
---@param spell any
---@return number level
function GetSpellLevelLearned(spell) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellLink)
---@param spell               = GetSpellLink(index any
---@param bookType any
---@return string link
---@return any spellId
function GetSpellLink(spell               = GetSpellLink(index, bookType) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellLossOfControlCooldown)
---@param spellSlot number
---@param bookType or spellName or spellID any
---@return number start
---@return number duration
function GetSpellLossOfControlCooldown(spellSlot, bookType or spellName or spellID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellPowerCost)
---@param spell any
---@return table[] costs
function GetSpellPowerCost(spell) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellTabInfo)
---@param tabIndex number
---@return string name
---@return string texture
---@return number offset
---@return number numSlots
---@return boolean isGuild
---@return number offspecID
function GetSpellTabInfo(tabIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellTexture)
---@param spell any
---@return number icon
function GetSpellTexture(spell) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetStablePetInfo)
---@param index number
---@return string petIcon
---@return string petName
---@return number petLevel
---@return string petType
---@return string petTalents
function GetStablePetInfo(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTalentGroupRole)
---@param groupIndex number
---@return string role
function GetTalentGroupRole(groupIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTalentPrereqs)
---@param tabIndex number
---@param talentIndex number
---@param isInspect? boolean
---@param isPet? boolean
---@param talentGroup? number
---@return number tier
---@return number column
---@return number isLearnable
function GetTalentPrereqs(tabIndex, talentIndex, isInspect, isPet, talentGroup) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTrackedAchievements)
---@return any id1
---@return any id2
---@return any idn
function GetTrackedAchievements() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTrackingTexture)
---@return number icon
function GetTrackingTexture() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillDescription)
---@param index any
---@return string description
function GetTradeSkillDescription(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillInfo)
---@param skillIndex number
---@return string skillName
---@return string skillType
---@return number numAvailable
---@return boolean isExpanded
---@return string altVerb
---@return number numSkillUps
---@return number indentLevel
---@return boolean showProgressBar
---@return number currentRank
---@return number maxRank
---@return number startingRank
function GetTradeSkillInfo(skillIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillInvSlotFilter)
---@param slotIndex number
---@return number isVisible
function GetTradeSkillInvSlotFilter(slotIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillInvSlots)
---@return any invSlots
function GetTradeSkillInvSlots() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillItemLink)
---@param skillId number
---@return string link
function GetTradeSkillItemLink(skillId) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillItemStats)
---@param skillId any
---@return table itemStats
function GetTradeSkillItemStats(skillId) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillLine)
---@return string tradeskillName
---@return number currentLevel
---@return number maxLevel
---@return number skillLineModifier
function GetTradeSkillLine() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillListLink)
---@return string? link
function GetTradeSkillListLink() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillNumMade)
---@param skillId number
---@return number minMade
---@return number maxMade
function GetTradeSkillNumMade(skillId) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillNumReagents)
---@param tradeSkillRecipeId number
---@return any numReagents
function GetTradeSkillNumReagents(tradeSkillRecipeId) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillReagentInfo)
---@param tradeSkillRecipeId any
---@param reagentId any
---@return string reagentName
---@return string reagentTexture
---@return number reagentCount
---@return number playerReagentCount
function GetTradeSkillReagentInfo(tradeSkillRecipeId, reagentId) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillReagentItemLink)
---@param skillId number
---@param reagentId number
---@return string link
function GetTradeSkillReagentItemLink(skillId, reagentId) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillRecipeLink)
---@param index number
---@return string link
function GetTradeSkillRecipeLink(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillSelectionIndex)
---@return any local tradeSkillIndex
function GetTradeSkillSelectionIndex() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillSubClasses)
---@return any subClasses
function GetTradeSkillSubClasses() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillTools)
---@param skillIndex any
function GetTradeSkillTools(skillIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeskillRepeatCount)
---@return any local repeatCount
function GetTradeskillRepeatCount() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetUnspentTalentPoints)
---@param isInspected any
---@param isPet any
---@param talentGroup any
---@return any talentPoints
function GetUnspentTalentPoints(isInspected, isPet, talentGroup) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetWatchedFactionInfo)
---@return string name
---@return number standing
---@return number min
---@return number max
---@return number value
---@return number factionID
function GetWatchedFactionInfo() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GlyphMatchesSocket)
---@param socketIndex number
---@return any selectedIndex
function GlyphMatchesSocket(socketIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_HasInspectHonorData)
---@return boolean hasData
function HasInspectHonorData() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_HasPetSpells)
---@return number numSpells
---@return string petToken
function HasPetSpells() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_InActiveBattlefield)
---@return boolean inBattlefield
function InActiveBattlefield() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_InviteUnit)
---@param playerName string
function InviteUnit(playerName) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsAttackSpell)
---@param spellName string
---@return any isAttack
function IsAttackSpell(spellName) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsAuctionSortReversed)
---@param type any
---@param sort any
---@return any sorted
function IsAuctionSortReversed(type, sort) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsCurrentSpell)
---@param spellID number
---@return boolean isCurrent
function IsCurrentSpell(spellID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsFactionInactive)
---@param index number
---@return boolean inactive
function IsFactionInactive(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsGlyphFlagSet)
---@param filter any
---@return boolean isSet
function IsGlyphFlagSet(filter) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsPassiveSpell)
---@param spellId or index any
---@param bookType string
---@return any isPassive
function IsPassiveSpell(spellId or index, bookType) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsPlayerAttacking)
---@param unit string
---@return boolean isAttacking
function IsPlayerAttacking(unit) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsQuestComplete)
---@param questID number
---@return boolean isComplete
function IsQuestComplete(questID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsSpellInRange)
---@param spellName string
---@param unit string
---@return number? inRange
function IsSpellInRange(spellName, unit) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsTalentSpell)
---@param spellName or slotIndex any
---@param bookType string
---@return boolean isTalentSpell
function IsTalentSpell(spellName or slotIndex, bookType) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsTrackedAchievement)
---@param achievementID any
---@return any tracked
function GetAchievementNumCriteria(achievementID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsTradeSkillLinked)
---@return any isLink
---@return any playerName
function IsTradeSkillLinked() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsTrainerServiceLearnSpell)
---@param index number
---@return number isLearnSpell
---@return number isPetLearnSpell
function IsTrainerServiceLearnSpell(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsUnitOnQuest)
---@param questIndex any
---@param unit any
function IsUnitOnQuest(questIndex, unit) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsUsableSpell)
---@param spell any
---@return boolean usable
---@return boolean noMana
function IsUsableSpell(spell) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_JoinBattlefield)
---@param index number
---@param asGroup? boolean
---@param isRated? boolean
function JoinBattlefield(index, asGroup, isRated) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_KeyRingButtonIDToInvSlotID)
---@param buttonID number
---@return number invSlot
function KeyRingButtonIDToInvSlotID(buttonID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_LeaveParty)
function LeaveParty() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_PetAbandon)
function PetAbandon() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_PetCanBeRenamed)
---@return boolean canRename
function PetCanBeRenamed() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_PetRename)
---@param name string
function PetRename(name) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_PickupCurrency)
---@param type number
function PickupCurrency(type) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_PickupSpell)
---@param spellID number
function PickupSpell(spellID) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_PickupSpellBookItem)
---@param spell any
function PickupSpellBookItem(spell) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_PickupStablePet)
---@param index any
function PickupStablePet(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_PlaceAuctionBid)
---@param type any
---@param index any
---@param bid any
function PlaceAuctionBid(type, index, bid) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_PlaceGlyphInSocket)
---@param index number
function PlaceGlyphInSocket(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_PostAuction)
---@param minBid number
---@param buyoutPrice number
---@param runTime number
---@param stackSize number
---@param numStacks number
---@param warningAcknowledged boolean
function PostAuction(minBid, buyoutPrice, runTime, stackSize, numStacks, warningAcknowledged) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_QueryAuctionItems)
---@param text string
---@param minLevel? number
---@param maxLevel? number
---@param page number
---@param usable boolean
---@param rarity? Enum.ItemQuality
---@param getAll boolean
---@param exactMatch boolean
---@param filterData? table
function QueryAuctionItems(text, minLevel, maxLevel, page, usable, rarity, getAll, exactMatch, filterData) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_QuestPOIGetIconInfo)
---@param questId number
---@return boolean completed
---@return number posX
---@return number posY
---@return number objective
function QuestPOIGetIconInfo(questId) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_RemoveGlyphFromSocket)
---@param index number
function RemoveGlyphFromSocket(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_RemoveQuestWatch)
---@param questIndex number
function RemoveQuestWatch(questIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_RemoveTrackedAchievement)
---@param achievementId any
function RemoveTrackedAchievement(achievementId) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_RequestInspectHonorData)
function RequestInspectHonorData() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_RequestInviteFromUnit)
---@param targetName string
function RequestInviteFromUnit(targetName) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SelectQuestLogEntry)
---@param questIndex number
function SelectQuestLogEntry(questIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetAbandonQuest)
function SetAbandonQuest() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetCurrencyBackpack)
---@param id any
---@param backpack any
function SetCurrencyBackpack(id, backpack) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetCurrencyUnused)
---@param id any
---@param unused any
function SetCurrencyUnused(id, unused) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetFactionActive)
---@param index number
function SetFactionActive(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetFactionInactive)
---@param index number
function SetFactionInactive(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetGlyphFilter)
---@param knowChecked boolean
---@param unKnowChecked boolean
---@param index number
function SetGlyphFilter(knowChecked, unKnowChecked, index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetGlyphNameFilter)
---@param name? string
function SetGlyphNameFilter(name) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetPetStablePaperdoll)
---@param modelObject any
function SetPetStablePaperdoll(modelObject) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetScreenResolution)
---@param index? number
function SetScreenResolution(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetSelectedAuctionItem)
---@param type any
---@param index any
function SetSelectedAuctionItem(type, index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetSelectedBattlefield)
---@param index number
function SetSelectedBattlefield(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetSelectedSkill)
---@param index number
function SetSelectedSkill(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetSpecialization)
---@param specIndex number
---@param isPet? boolean
function SetSpecialization(specIndex, isPet) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetTradeSkillInvSlotFilter)
---@param slotIndex any
---@param onOff{ any
---@param exclusive} any
function SetTradeSkillInvSlotFilter(slotIndex, onOff{, exclusive}) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetTradeSkillItemLevelFilter)
---@param minLevel number
---@param maxLevel number
---@return any nil
function SetTradeSkillItemLevelFilter(minLevel, maxLevel) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetTradeSkillSubClassFilter)
---@param slotIndex any
---@param onOff{ any
---@param exclusive} any
function SetTradeSkillSubClassFilter(slotIndex, onOff{, exclusive}) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetWatchedFactionIndex)
---@param index number
function SetWatchedFactionIndex(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ShiftQuestWatches)
---@param id1 any
---@param id2 any
function ShiftQuestWatches(id1, id2) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ShouldKnowUnitHealth)
---@param unit UnitToken
---@return boolean shouldKnowUnitHealth
function ShouldKnowUnitHealth(unit) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SortAuctionItems)
---@param type any
---@param sort any
function SortAuctionItems(type, sort) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SortAuctionSetSort)
---@param type string
---@param column string
---@param reverse boolean
function SortAuctionSetSort(type, column, reverse) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SortQuestWatches)
---@return boolean changed
function SortQuestWatches() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SpellGetVisibilityInfo)
---@param spellId number
---@param visType string
---@return boolean hasCustom
---@return boolean alwaysShowMine
---@return boolean showForMySpec
function SpellGetVisibilityInfo(spellId, visType) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_StopTradeSkillRepeat)
function StopTradeSkillRepeat() end

---[Documentation](https://warcraft.wiki.gg/wiki/API_StripHyperlinks)
---@param text string
---@param maintainColor? boolean
---@param maintainBrackets? boolean
---@param stripNewlines? boolean
---@param maintainAtlases? boolean
---@return string stripped
function StripHyperlinks(text, maintainColor, maintainBrackets, stripNewlines, maintainAtlases) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ToggleGlyphFilter)
---@param filter number
function ToggleGlyphFilter(filter) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitAttackBothHands)
---@param unit string
---@return number mainBase
---@return number mainMod
---@return number offBase
---@return number offMod
function UnitAttackBothHands(unit) end

---@param unit UnitId
---@param index number
---@param filter? string
---@return string name
---@return number icon
---@return number count
---@return string? dispelType
---@return number duration
---@return number expirationTime
---@return UnitId source
---@return boolean isStealable
---@return boolean nameplateShowPersonal
---@return number spellId
---@return boolean canApplyAura
---@return boolean isBossDebuff
---@return boolean castByPlayer
---@return boolean nameplateShowAll
---@return number timeMod
---@return ...
function UnitAura(unit, index, filter) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitAura)
---@param unit UnitId
---@param index number
---@param filter? string
---@return string name
---@return number icon
---@return number count
---@return string? dispelType
---@return number duration
---@return number expirationTime
---@return UnitId source
---@return boolean isStealable
---@return boolean nameplateShowPersonal
---@return number spellId
---@return boolean canApplyAura
---@return boolean isBossDebuff
---@return boolean castByPlayer
---@return boolean nameplateShowAll
---@return number timeMod
---@return ...
function UnitBuff(unit, index, filter) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitAura)
---@param unit UnitId
---@param index number
---@param filter? string
---@return string name
---@return number icon
---@return number count
---@return string? dispelType
---@return number duration
---@return number expirationTime
---@return UnitId source
---@return boolean isStealable
---@return boolean nameplateShowPersonal
---@return number spellId
---@return boolean canApplyAura
---@return boolean isBossDebuff
---@return boolean castByPlayer
---@return boolean nameplateShowAll
---@return number timeMod
---@return ...
function UnitDebuff(unit, index, filter) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitCharacterPoints)
---@param unit string
---@return number talentPoints
function UnitCharacterPoints(unit) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitInPhase)
---@param unit string
---@return boolean inPhase
function UnitInPhase(unit) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitIsCivilian)
---@param unit string
---@return boolean isCivilian
function UnitIsCivilian(unit) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitPVPRank)
---@param unit string
---@return number rankID
function UnitPVPRank(unit) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitRangedAttack)
---@param unit string
---@return number base
---@return number modifier
function UnitRangedAttack(unit) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitResistance)
---@param unit string
---@param resistanceIndex? number
---@return number base
---@return number total
---@return number bonus
---@return number minus
function UnitResistance(unit, resistanceIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_VehicleCameraZoomIn)
---@param increment? number
function VehicleCameraZoomIn(increment) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_VehicleCameraZoomOut)
---@param increment? number
function VehicleCameraZoomOut(increment) end

-- Undocumented APIs (no wiki page or unparseable)

---[Documentation](https://warcraft.wiki.gg/wiki/API_AddPreviewTalentPoints)
function AddPreviewTalentPoints(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_AreHighResTexturesAvailable)
function AreHighResTexturesAvailable(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_BNGetFriendInfoByID)
function BNGetFriendInfoByID(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_BNGetGameAccountInfo)
function BNGetGameAccountInfo(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_BNGetGameAccountInfoByGUID)
function BNGetGameAccountInfoByGUID(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_BuyArenaCharter)
function BuyArenaCharter(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CalculateAuctionDeposit)
function CalculateAuctionDeposit(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CanCancelAuction)
function CanCancelAuction(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CancelAuction)
function CancelAuction(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CancelEmote)
function CancelEmote(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ClickAuctionSellItemButton)
function ClickAuctionSellItemButton(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CloseArenaTeamRoster)
function CloseArenaTeamRoster(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CloseCraft)
function CloseCraft(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ClosePetitionRegistrar)
function ClosePetitionRegistrar(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CollapseAllFactionHeaders)
function CollapseAllFactionHeaders(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CollapseCraftSkillLine)
function CollapseCraftSkillLine(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CollapseTradeSkillSubClass)
function CollapseTradeSkillSubClass(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CombatLogAddFilter)
function CombatLogAddFilter(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CombatLogClearEntries)
function CombatLogClearEntries(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CombatLogGetNumEntries)
function CombatLogGetNumEntries(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CombatLogGetRetentionTime)
function CombatLogGetRetentionTime(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CombatLogResetFilter)
function CombatLogResetFilter(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CombatLogSetRetentionTime)
function CombatLogSetRetentionTime(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CombatLog_Object_IsA)
function CombatLog_Object_IsA(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ConvertToRaid)
function ConvertToRaid(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CraftIsEnchanting)
function CraftIsEnchanting(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_CraftOnlyShowMakeable)
function CraftOnlyShowMakeable(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_DeathRecap_GetEvents)
function DeathRecap_GetEvents(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_DeathRecap_HasEvents)
function DeathRecap_HasEvents(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_DisableSpellAutocast)
function DisableSpellAutocast(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_DoCraft)
function DoCraft(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_EnableSpellAutocast)
function EnableSpellAutocast(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ExpandAllFactionHeaders)
function ExpandAllFactionHeaders(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ExpandCraftSkillLine)
function ExpandCraftSkillLine(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GMSubmitBug)
function GMSubmitBug(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GMSubmitSuggestion)
function GMSubmitSuggestion(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetAbandonQuestItems)
function GetAbandonQuestItems(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetArenaTeamGdfInfo)
function GetArenaTeamGdfInfo(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetArenaTeamRosterSelection)
function GetArenaTeamRosterSelection(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetArenaTeamRosterShowOffline)
function GetArenaTeamRosterShowOffline(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetAuctionDeposit)
function GetAuctionDeposit(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetAuctionHouseDepositRate)
function GetAuctionHouseDepositRate(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetAuctionSort)
function GetAuctionSort(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetBidderAuctionItems)
function GetBidderAuctionItems(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCVarSettingValidity)
function GetCVarSettingValidity(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftButtonToken)
function GetCraftButtonToken(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftCooldown)
function GetCraftCooldown(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftFilter)
function GetCraftFilter(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftIcon)
function GetCraftIcon(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftNumMade)
function GetCraftNumMade(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftSelectionIndex)
function GetCraftSelectionIndex(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftSlots)
function GetCraftSlots(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCritChanceFromAgility)
function GetCritChanceFromAgility(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCurrentArenaSeasonUsesTeams)
function GetCurrentArenaSeasonUsesTeams(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCurrentGraphicsSetting)
function GetCurrentGraphicsSetting(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetDefaultGraphicsQuality)
function GetDefaultGraphicsQuality(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetDefaultVideoOption)
function GetDefaultVideoOption(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetDefaultVideoOptions)
function GetDefaultVideoOptions(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetDefaultVideoQualityOption)
function GetDefaultVideoQualityOption(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetDuelerInfo)
function GetDuelerInfo(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetFactionInfoByID)
function GetFactionInfoByID(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetGraphicsCVarOffsetForCVar)
function GetGraphicsCVarOffsetForCVar(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetGraphicsCVarOffsetForUI)
function GetGraphicsCVarOffsetForUI(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetGraphicsDropdownIndexByMasterIndex)
function GetGraphicsDropdownIndexByMasterIndex(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetGroupPreviewTalentPointsSpent)
function GetGroupPreviewTalentPointsSpent(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetInspectArenaTeamData)
function GetInspectArenaTeamData(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetInviteReferralInfo)
function GetInviteReferralInfo(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetItemStatDelta)
function GetItemStatDelta(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetMajorTalentTreeBonuses)
function GetMajorTalentTreeBonuses(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetMaxDailyQuests)
function GetMaxDailyQuests(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetMinorTalentTreeBonuses)
function GetMinorTalentTreeBonuses(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNextPetTalentLevel)
function GetNextPetTalentLevel(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNextTalentLevel)
function GetNextTalentLevel(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumArenaTeamMembers)
function GetNumArenaTeamMembers(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumPrimaryProfessions)
function GetNumPrimaryProfessions(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumTalentPoints)
function GetNumTalentPoints(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetOnlyShowMakeable)
function GetOnlyShowMakeable(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetOnlyShowSkillUps)
function GetOnlyShowSkillUps(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetPetitionItemPrice)
function GetPetitionItemPrice(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetPreviewPrimaryTalentTree)
function GetPreviewPrimaryTalentTree(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetPreviewTalentPointsSpent)
function GetPreviewTalentPointsSpent(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetPreviousArenaSeasonUsesTeams)
function GetPreviousArenaSeasonUsesTeams(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestLogIsAutoComplete)
function GetQuestLogIsAutoComplete(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestLogPortraitGiver)
function GetQuestLogPortraitGiver(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestLogRequiredMoney)
function GetQuestLogRequiredMoney(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestLogRewardArenaPoints)
function GetQuestLogRewardArenaPoints(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestLogRewardTalents)
function GetQuestLogRewardTalents(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestLogSelectedID)
function GetQuestLogSelectedID(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestWatchIndex)
function GetQuestWatchIndex(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetQuestWatchInfo)
function GetQuestWatchInfo(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetRewardArenaPoints)
function GetRewardArenaPoints(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetRewardTalentPoints)
function GetRewardTalentPoints(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSelectedAuctionItem)
function GetSelectedAuctionItem(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSelectedFaction)
function GetSelectedFaction(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellAvailableLevel)
function GetSpellAvailableLevel(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellCritChanceFromIntellect)
function GetSpellCritChanceFromIntellect(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellRank)
function GetSpellRank(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellSubtext)
function GetSpellSubtext(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellTradeSkillLink)
function GetSpellTradeSkillLink(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetStablePetFoodTypes)
function GetStablePetFoodTypes(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSuggestedGroupNum)
function GetSuggestedGroupNum(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSuperTrackedQuestID)
function GetSuperTrackedQuestID(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTalentClearInfo)
function GetTalentClearInfo(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTalentTreeEarlySpells)
function GetTalentTreeEarlySpells(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTalentTreeRoles)
function GetTalentTreeRoles(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetToolTipInfo)
function GetToolTipInfo(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillCooldown)
function GetTradeSkillCooldown(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillIcon)
function GetTradeSkillIcon(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillItemLevelFilter)
function GetTradeSkillItemLevelFilter(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillItemNameFilter)
function GetTradeSkillItemNameFilter(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillSubClassFilter)
function GetTradeSkillSubClassFilter(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetUITextureKitInfo)
function GetUITextureKitInfo(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetUnitHealthRegenRateFromSpirit)
function GetUnitHealthRegenRateFromSpirit(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetUnitManaRegenRateFromSpirit)
function GetUnitManaRegenRateFromSpirit(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetVideoOptions)
function GetVideoOptions(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GetWorldPVPQueueMapName)
function GetWorldPVPQueueMapName(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_HasFilledPetition)
function HasFilledPetition(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_HonorSystemEnabled)
function HonorSystemEnabled(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsArenaSeasonActive)
function IsArenaSeasonActive(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsArenaTeamCaptain)
function IsArenaTeamCaptain(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsAtStableMaster)
function IsAtStableMaster(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsAutoRepeatSpell)
function IsAutoRepeatSpell(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsBattlefieldArena)
function IsBattlefieldArena(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsChannelModerator)
function IsChannelModerator(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsChannelOwner)
function IsChannelOwner(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsCompetitiveModeEnabled)
function IsCompetitiveModeEnabled(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsConsumableSpell)
function IsConsumableSpell(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsEncounterInProgress)
function IsEncounterInProgress(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsEncounterLimitingResurrections)
function IsEncounterLimitingResurrections(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsEncounterSuppressingRelease)
function IsEncounterSuppressingRelease(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsHarmfulSpell)
function IsHarmfulSpell(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsHelpfulSpell)
function IsHelpfulSpell(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsInArenaTeam)
function IsInArenaTeam(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsKeyRingEnabled)
function IsKeyRingEnabled(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsPetAssistAvailable)
function IsPetAssistAvailable(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsQuestHardWatched)
function IsQuestHardWatched(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsQuestWatched)
function IsQuestWatched(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsRatedBattleground)
function IsRatedBattleground(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsSpellHidden)
function IsSpellHidden(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsUnitOnQuestByQuestID)
function IsUnitOnQuestByQuestID(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_IsUsingLegacyAuctionClient)
function IsUsingLegacyAuctionClient(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_JoinWorldPVPQueue)
function JoinWorldPVPQueue(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_LearnPreviewTalents)
function LearnPreviewTalents(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_PurchaseSlot)
function PurchaseSlot(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_QueryWorldCountdownTimer)
function QueryWorldCountdownTimer(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_QuestPOIGetQuestIDByIndex)
function QuestPOIGetQuestIDByIndex(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_QuestPOIGetQuestIDByVisibleIndex)
function QuestPOIGetQuestIDByVisibleIndex(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ResetGroupPreviewTalentPoints)
function ResetGroupPreviewTalentPoints(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ResetPreviewTalentPoints)
function ResetPreviewTalentPoints(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SelectCraft)
function SelectCraft(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SelectTradeSkill)
function SelectTradeSkill(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetArenaTeamRosterSelection)
function SetArenaTeamRosterSelection(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetArenaTeamRosterShowOffline)
function SetArenaTeamRosterShowOffline(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetAuctionsTabShowing)
function SetAuctionsTabShowing(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetCraftFilter)
function SetCraftFilter(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetCurrentGraphicsSetting)
function SetCurrentGraphicsSetting(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetDefaultVideoOptions)
function SetDefaultVideoOptions(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetInventoryPortraitTexture)
function SetInventoryPortraitTexture(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetPendingReportArenaTeamName)
function SetPendingReportArenaTeamName(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetPetSlot)
function SetPetSlot(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetPreviewPrimaryTalentTree)
function SetPreviewPrimaryTalentTree(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetPrimaryTalentTree)
function SetPrimaryTalentTree(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetRaidTargetProtected)
function SetRaidTargetProtected(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetSelectedFaction)
function SetSelectedFaction(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetSuperTrackedQuestID)
function SetSuperTrackedQuestID(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SetTradeSkillItemNameFilter)
function SetTradeSkillItemNameFilter(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ShowInventorySellCursor)
function ShowInventorySellCursor(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SortArenaTeamRoster)
function SortArenaTeamRoster(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SortAuctionApplySort)
function SortAuctionApplySort(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SortAuctionClearSort)
function SortAuctionClearSort(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SpellHasRange)
function SpellHasRange(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SpellIsAlwaysShown)
function SpellIsAlwaysShown(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_SpellIsSelfBuff)
function SpellIsSelfBuff(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_StartAuction)
function StartAuction(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_ToggleSpellAutocast)
function ToggleSpellAutocast(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_TradeSkillOnlyShowMakeable)
function TradeSkillOnlyShowMakeable(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_TradeSkillOnlyShowSkillUps)
function TradeSkillOnlyShowSkillUps(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_TurnInArenaPetition)
function TurnInArenaPetition(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitBuff)
function UnitBuff(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitDebuff)
function UnitDebuff(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitDefense)
function UnitDefense(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitIgnoresVehicleComboPoints)
function UnitIgnoresVehicleComboPoints(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitVehicleSkinType)
function UnitVehicleSkinType(...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_WantsAlteredForm)
function WantsAlteredForm(...) end
