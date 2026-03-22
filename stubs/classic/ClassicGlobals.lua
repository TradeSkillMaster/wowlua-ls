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

-- Classic-only FrameXML functions

function AchievementButton_Collapse(...) end

function AchievementButton_Desaturate(...) end

function AchievementButton_DisplayAchievement(...) end

function AchievementButton_DisplayObjectives(...) end

function AchievementButton_Expand(...) end

function AchievementButton_GetCriteria(...) end

function AchievementButton_GetMeta(...) end

function AchievementButton_GetMiniAchievement(...) end

function AchievementButton_GetProgressBar(...) end

function AchievementButton_OnClick(...) end

function AchievementButton_OnLoad(...) end

function AchievementButton_ResetCriteria(...) end

function AchievementButton_ResetMetas(...) end

function AchievementButton_ResetMiniAchievements(...) end

function AchievementButton_ResetObjectives(...) end

function AchievementButton_ResetProgressBars(...) end

function AchievementButton_Saturate(...) end

function AchievementButton_ToggleMetaView(...) end

function AchievementButton_ToggleTracking(...) end

function AchievementButton_UpdatePlusMinusTexture(...) end

function AchievementCategoryButton_OnClick(...) end

function AchievementCategoryButton_OnLoad(...) end

function AchievementFrameAchievements_AdjustSelection(...) end

function AchievementFrameAchievements_ClearSelection(...) end

function AchievementFrameAchievements_FindSelection(...) end

function AchievementFrameAchievements_SelectButton(...) end

function AchievementFrameAchievements_ToggleView(...) end

function AchievementFrameAchievements_Update(...) end

function AchievementFrameCategories_ClearSelection(...) end

function AchievementFrameCategories_DisplayButton(...) end

function AchievementFrameCategories_GetCategoryList(...) end

function AchievementFrameCategories_OnEvent(...) end

function AchievementFrameCategories_SelectButton(...) end

function AchievementFrameCategories_Update(...) end

function AchievementFrameComparisonContainer_OnLoad(...) end

function AchievementFrameComparisonStatsContainer_OnLoad(...) end

function AchievementFrameComparisonStats_SetHeader(...) end

function AchievementFrameComparisonStats_SetStat(...) end

function AchievementFrameComparison_ClearSelection(...) end

function AchievementFrameComparison_DisplayAchievement(...) end

function AchievementFrameComparison_Update(...) end

function AchievementFrameComparison_UpdateStats(...) end

function AchievementFrameStats_SetHeader(...) end

function AchievementFrameStats_SetStat(...) end

function AchievementFrameStats_Update(...) end

function AchievementFrameSummary_ToggleView(...) end

function AchievementFrame_ClearTextures(...) end

function AchievementFrame_IsFeatOfStrength(...) end

function AchievementFrame_LoadTextures(...) end

function AchievementFrame_SelectSummaryStatistic(...) end

function AchievementFrame_ToggleView(...) end

function AchievementFrame_UpdateTrackedAchievements(...) end

function AchievementMeta_OnEnter(...) end

function AchievementMicroButton_OnEvent(...) end

function AchievementMicroButton_OnLoad(...) end

function AchievementStatButton_OnClick(...) end

function AchievementStatButton_OnEnter(...) end

function AchievementStatButton_OnLoad(...) end

function ActionBarActionEventsFrame_OnEvent(...) end

function ActionBarActionEventsFrame_OnLoad(...) end

function ActionBarActionEventsFrame_RegisterFrame(...) end

function ActionBarActionEventsFrame_UnregisterFrame(...) end

function ActionBarButtonEventsFrame_OnCountdownForCooldownsChanged(...) end

function ActionBarButtonEventsFrame_OnEvent(...) end

function ActionBarButtonEventsFrame_OnLoad(...) end

function ActionBarButtonEventsFrame_RegisterFrame(...) end

function ActionButton_CalculateAction(...) end

function ActionButton_ClearFlash(...) end

function ActionButton_GetOverlayGlow(...) end

function ActionButton_GetPagedID(...) end

function ActionButton_HideGrid(...) end

function ActionButton_IsFlashing(...) end

function ActionButton_OnCooldownDone(...) end

function ActionButton_OnEvent(...) end

function ActionButton_OnLoad(...) end

function ActionButton_OnUpdate(...) end

function ActionButton_OverlayGlowAnimOutFinished(...) end

function ActionButton_OverlayGlowOnUpdate(...) end

function ActionButton_SetTooltip(...) end

function ActionButton_ShowGrid(...) end

function ActionButton_StartFlash(...) end

function ActionButton_StopFlash(...) end

function ActionButton_Update(...) end

function ActionButton_UpdateAction(...) end

function ActionButton_UpdateCount(...) end

function ActionButton_UpdateFlash(...) end

function ActionButton_UpdateFlyout(...) end

function ActionButton_UpdateHighlightMark(...) end

function ActionButton_UpdateHotkeys(...) end

function ActionButton_UpdateOverlayGlow(...) end

function ActionButton_UpdateSpellHighlightMark(...) end

function ActionButton_UpdateState(...) end

function ActionButton_UpdateUsable(...) end

function ActionStatus_DisplayMessage(...) end

function ActionStatus_OnEvent(...) end

function ActionStatus_OnLoad(...) end

function ActionStatus_OnUpdate(...) end

function Add(...) end

function AddFriendEntryFrame_Collapse(...) end

function AddFriendEntryFrame_Expand(...) end

function AddFriendFrame_OnShow(...) end

function AddFriendFrame_ShowEntry(...) end

function AddFriendFrame_ShowInfo(...) end

function AddonListScrollFrame_OnVerticalScroll(...) end

function AddonList_Hide(...) end

function AddonList_OnHide(...) end

function AddonList_OnLoad(...) end

function AddonList_OnShow(...) end

function AddonList_Show(...) end

function AlternatePowerBar_Initialize(...) end

function AlternatePowerBar_OnEvent(...) end

function AlternatePowerBar_OnLoad(...) end

function AlternatePowerBar_OnUpdate(...) end

function AlternatePowerBar_SetLook(...) end

function AlternatePowerBar_SpecializationCheck(...) end

function AlternatePowerBar_UpdateMaxValues(...) end

function AlternatePowerBar_UpdatePowerType(...) end

function AlternatePowerBar_UpdateValue(...) end

function ArenaEnemyBackground_SetOpacity(...) end

function ArenaEnemyFrame_Lock(...) end

function ArenaEnemyFrame_OnEvent(...) end

function ArenaEnemyFrame_OnLoad(...) end

function ArenaEnemyFrame_OnShow(...) end

function ArenaEnemyFrame_SetMysteryPlayer(...) end

function ArenaEnemyFrame_Unlock(...) end

function ArenaEnemyFrame_UpdateCrowdControl(...) end

function ArenaEnemyFrame_UpdatePet(...) end

function ArenaEnemyFrame_UpdatePlayer(...) end

function ArenaEnemyFrame_UpdatePredictionBars(...) end

function ArenaEnemyFrames_CheckEffectiveEnableState(...) end

function ArenaEnemyFrames_Disable(...) end

function ArenaEnemyFrames_Enable(...) end

function ArenaEnemyFrames_GetBestAnchorUnitFrameForOppponent(...) end

function ArenaEnemyFrames_OnEvent(...) end

function ArenaEnemyFrames_OnHide(...) end

function ArenaEnemyFrames_OnLoad(...) end

function ArenaEnemyFrames_OnShow(...) end

function ArenaEnemyFrames_ResetCrowdControlCooldownData(...) end

function ArenaEnemyFrames_UpdateVisible(...) end

function ArenaEnemyPetFrame_OnEvent(...) end

function ArenaEnemyPetFrame_OnLoad(...) end

function ArenaPrepBackground_SetOpacity(...) end

function ArenaPrepFrames_GetBestAnchorUnitFrameForOppponent(...) end

function ArenaPrepFrames_OnEvent(...) end

function ArenaPrepFrames_OnHide(...) end

function ArenaPrepFrames_OnLoad(...) end

function ArenaPrepFrames_OnShow(...) end

function ArenaPrepFrames_UpdateBackground(...) end

function ArenaPrepFrames_UpdateFrames(...) end

function ArenaRegistrarFrameEditBox_OnEnterPressed(...) end

function ArenaRegistrarFrameEditBox_OnEscapePressed(...) end

function ArenaRegistrarFramePurchaseButton_OnClick(...) end

function ArenaRegistrarMoneyFrame_OnLoad(...) end

function ArenaRegistrar_OnEvent(...) end

function ArenaRegistrar_OnHide(...) end

function ArenaRegistrar_OnLoad(...) end

function ArenaRegistrar_OnShow(...) end

function ArenaRegistrar_ShowPurchaseFrame(...) end

function ArenaRegistrar_TurnInPetition(...) end

function ArenaRegistrar_UpdatePrice(...) end

function ArenaTeam_GetTeamSizeID(...) end

function Arena_LoadUI(...) end

function AuctionBrowseFrame_CheckUnlockHighlight(...) end

function AuctionFrameAuctions_OnEvent(...) end

function AuctionFrameAuctions_OnLoad(...) end

function AuctionFrameAuctions_OnShow(...) end

function AuctionFrameAuctions_OnUpdate(...) end

function AuctionFrameAuctions_Update(...) end

function AuctionFrameBid_OnEvent(...) end

function AuctionFrameBid_OnLoad(...) end

function AuctionFrameBid_OnShow(...) end

function AuctionFrameBid_Update(...) end

function AuctionFrameBrowse_OnEvent(...) end

function AuctionFrameBrowse_OnHide(...) end

function AuctionFrameBrowse_OnLoad(...) end

function AuctionFrameBrowse_OnShow(...) end

function AuctionFrameBrowse_Reset(...) end

function AuctionFrameBrowse_Search(...) end

function AuctionFrameBrowse_Update(...) end

function AuctionFrameBrowse_UpdateArrows(...) end

function AuctionFrameFilter_OnClick(...) end

function AuctionFrameItem_OnEnter(...) end

function AuctionFrameItem_OnLeave(...) end

function AuctionFrameTab_OnClick(...) end

function AuctionFrame_GetTimeLeftText(...) end

function AuctionFrame_GetTimeLeftTooltipText(...) end

function AuctionFrame_Hide(...) end

function AuctionFrame_LoadUI(...) end

function AuctionFrame_OnClickSortColumn(...) end

function AuctionFrame_OnEvent(...) end

function AuctionFrame_OnLoad(...) end

function AuctionFrame_OnShow(...) end

function AuctionFrame_SetDialogOverlayShown(...) end

function AuctionFrame_SetSort(...) end

function AuctionFrame_Show(...) end

function AuctionFrame_ShowPostConfirmationDialog(...) end

function AuctionPriceTooltipFrame_OnEnter(...) end

function AuctionPriceTooltipFrame_OnLeave(...) end

function AuctionPriceTooltipFrame_OnLoad(...) end

function AuctionProgressFrame_OnUpdate(...) end

function AuctionSellItemButton_OnClick(...) end

function AuctionSellItemButton_OnEvent(...) end

function AuctionsButton_OnClick(...) end

function AuctionsFrameAuctions_ValidateAuction(...) end

function AuctionsRadioButton_OnClick(...) end

function AuctionsWowTokenAuctionFrame_OnEvent(...) end

function AuctionsWowTokenAuctionFrame_OnLoad(...) end

function AuctionsWowTokenAuctionFrame_Update(...) end

function AuraButton_OnUpdate(...) end

function AuraButton_Update(...) end

function AuraButton_UpdateDuration(...) end

function AutoCastShine_AutoCastStart(...) end

function AutoCastShine_AutoCastStop(...) end

function AutoCastShine_OnLoad(...) end

function AutoCastShine_OnUpdate(...) end

function AutoComplete_OnShow(...) end

function AutoQuestWatch_CheckDeleted(...) end

function AutoQuestWatch_Insert(...) end

function AutoQuestWatch_OnUpdate(...) end

function AutoQuestWatch_Update(...) end

function BackpackButton_OnClick(...) end

function BackpackButton_UpdateChecked(...) end

function BackpackTokenButton_OnClick(...) end

function BackpackTokenFrame_IsShown(...) end

function BackpackTokenFrame_Update(...) end

function BagSlotButton_OnClick(...) end

function BagSlotButton_OnDrag(...) end

function BagSlotButton_OnEnter(...) end

function BagSlotButton_OnModifiedClick(...) end

function BagSlotButton_UpdateChecked(...) end

function BankFrameBagButton_OnEvent(...) end

function BankFrameBagButton_OnLoad(...) end

function BankFrameBaseButton_OnLoad(...) end

function BankFrameItemButtonBag_OnClick(...) end

function BankFrameItemButtonBag_Pickup(...) end

function BankFrameItemButtonGeneric_OnClick(...) end

function BankFrameItemButtonGeneric_OnModifiedClick(...) end

function BankFrameItemButton_OnEnter(...) end

function BankFrameItemButton_OnLoad(...) end

function BankFrameItemButton_Update(...) end

function BankFrameItemButton_UpdateLocked(...) end

function BankFrame_OnEvent(...) end

function BankFrame_OnHide(...) end

function BankFrame_OnLoad(...) end

function BankFrame_OnShow(...) end

function BankFrame_ShowPanel(...) end

function BankFrame_TabOnClick(...) end

function BankFrame_UpdateCooldown(...) end

function BankSlotsFrame_OnLoad(...) end

function BarberShop_CheckForInvalidOptions(...) end

function BarberShop_HandleAlternateFormButtons(...) end

function BarberShop_OnEvent(...) end

function BarberShop_OnHide(...) end

function BarberShop_OnLoad(...) end

function BarberShop_OnShow(...) end

function BarberShop_ResetAll(...) end

function BarberShop_ResetBanner(...) end

function BarberShop_ResetLabelColors(...) end

function BarberShop_SetLabelColor(...) end

function BarberShop_SetSelectedSex(...) end

function BarberShop_SetViewingAlteredForm(...) end

function BarberShop_Update(...) end

function BarberShop_UpdateBanner(...) end

function BarberShop_UpdateCost(...) end

function BarberShop_UpdateCustomizationOptions(...) end

function BarberShop_UpdateSelector(...) end

function BarberShop_UpdateSexSelectors(...) end

function BattlefieldButton_OnClick(...) end

function BattlefieldFrameJoinButton_OnClick(...) end

function BattlefieldFrame_OnEvent(...) end

function BattlefieldFrame_OnLoad(...) end

function BattlefieldFrame_OnUpdate(...) end

function BattlefieldFrame_Update(...) end

function BattlefieldFrame_UpdateStatus(...) end

function BattlegroundShineFadeIn(...) end

function BattlegroundShineFadeOut(...) end

function BidButton_OnClick(...) end

function Blizzard_CombatLog_ApplyFilters(...) end

function Blizzard_CombatLog_Refilter(...) end

function Blizzard_CombatLog_RefilterUpdate(...) end

function BossTargetFrame_OnLoad(...) end

function BossTargetFrame_UpdateLevelTextAnchor(...) end

function BrowseButton_OnClick(...) end

function BrowsePriceOptionsButton_OnClick(...) end

function BrowsePriceOptionsFrame_OnShow(...) end

function BrowsePriceOptionsRadioButton_OnClick(...) end

function BrowseResetButton_OnUpdate(...) end

function BrowseSearchButton_OnUpdate(...) end

function BuffButton_OnClick(...) end

function BuffButton_OnLoad(...) end

function BuffFrame_OnEvent(...) end

function BuffFrame_OnLoad(...) end

function BuffFrame_OnUpdate(...) end

function BuffFrame_Update(...) end

function BuffFrame_UpdateAllBuffAnchors(...) end

function BuffFrame_UpdatePositions(...) end

function ButtonInventorySlot(...) end

function COMBAT_TEXT_SCROLL_FUNCTION(...) end

function CanGroupInvite(...) end

function CancelButton_OnClick(...) end

function CastingBarFrame_AddWidgetForFade(...) end

function CastingBarFrame_ApplyAlpha(...) end

function CastingBarFrame_FinishSpell(...) end

function CastingBarFrame_GetEffectiveStartColor(...) end

function CastingBarFrame_OnEvent(...) end

function CastingBarFrame_OnLoad(...) end

function CastingBarFrame_OnShow(...) end

function CastingBarFrame_OnUpdate(...) end

function CastingBarFrame_SetFailedCastColor(...) end

function CastingBarFrame_SetFinishedCastColor(...) end

function CastingBarFrame_SetIcon(...) end

function CastingBarFrame_SetLook(...) end

function CastingBarFrame_SetNonInterruptibleCastColor(...) end

function CastingBarFrame_SetStartCastColor(...) end

function CastingBarFrame_SetStartChannelColor(...) end

function CastingBarFrame_SetUnit(...) end

function CastingBarFrame_SetUseStartColorForFinished(...) end

function CastingBarFrame_SetUseStartColorForFlash(...) end

function CastingBarFrame_UpdateInterruptibleState(...) end

function CastingBarFrame_UpdateIsShown(...) end

function ChallengeModeAlertFrame_SetUp(...) end

function ChallengesFrameBestTimes_Update(...) end

function ChallengesFrameDungeonButton_OnClick(...) end

function ChallengesFrameGuild_OnEnter(...) end

function ChallengesFrameRealm_OnEnter(...) end

function ChallengesFrame_GetSelection(...) end

function ChallengesFrame_OnEvent(...) end

function ChallengesFrame_OnLoad(...) end

function ChallengesFrame_OnShow(...) end

function ChallengesFrame_Update(...) end

function CharacterFrameTab_OnClick(...) end

function CharacterFrame_OnEvent(...) end

function CharacterFrame_OnHide(...) end

function CharacterFrame_OnLoad(...) end

function CharacterFrame_OnShow(...) end

function CharacterFrame_ShowSubFrame(...) end

function CharacterFrame_TabBoundsCheck(...) end

function CharacterMicroButton_OnEvent(...) end

function CharacterMicroButton_OnLoad(...) end

function CharacterMicroButton_SetNormal(...) end

function CharacterMicroButton_SetPushed(...) end

function CharacterModelFrame_OnMouseUp(...) end

function CharacterRangedDamageFrame_OnEnter(...) end

function CharacterSpecificButton_OnClick(...) end

function CharacterSpecificButton_OnEnter(...) end

function CharacterSpecificButton_OnHide(...) end

function CharacterSpecificButton_OnLoad(...) end

function CharacterSpellCritChance_OnEnter(...) end

function ChatChannelDropdown_PopOutChat(...) end

function ChatChannelDropdown_Show(...) end

function ChatClassColorOverrideShown(...) end

function ChatConfigFrameToggleChatButton_OnClick(...) end

function ChatConfigFrameToggleChatButton_UpdateAccountChatDisabled(...) end

function ChatConfig_CreateBoxes(...) end

function ChatEdit_ActivateChat(...) end

function ChatEdit_AddHistory(...) end

function ChatEdit_ChooseBoxForSend(...) end

function ChatEdit_ClearChat(...) end

function ChatEdit_DeactivateChat(...) end

function ChatEdit_ExtractChannel(...) end

function ChatEdit_ExtractTellTarget(...) end

function ChatEdit_FocusActiveWindow(...) end

function ChatEdit_GetActiveWindow(...) end

function ChatEdit_GetChannelTarget(...) end

function ChatEdit_GetLastActiveWindow(...) end

function ChatEdit_GetLastTellTarget(...) end

function ChatEdit_GetLastToldTarget(...) end

function ChatEdit_GetNextTellTarget(...) end

function ChatEdit_HandleChatType(...) end

function ChatEdit_InsertLink(...) end

function ChatEdit_LanguageShow(...) end

function ChatEdit_LinkItem(...) end

function ChatEdit_OnChar(...) end

function ChatEdit_OnEditFocusGained(...) end

function ChatEdit_OnEditFocusLost(...) end

function ChatEdit_OnEnterPressed(...) end

function ChatEdit_OnEscapePressed(...) end

function ChatEdit_OnEvent(...) end

function ChatEdit_OnHide(...) end

function ChatEdit_OnInputLanguageChanged(...) end

function ChatEdit_OnLoad(...) end

function ChatEdit_OnShow(...) end

function ChatEdit_OnSpacePressed(...) end

function ChatEdit_OnTabPressed(...) end

function ChatEdit_OnTextChanged(...) end

function ChatEdit_OnTextSet(...) end

function ChatEdit_OnUpdate(...) end

function ChatEdit_ParseText(...) end

function ChatEdit_ResetChatType(...) end

function ChatEdit_ResetChatTypeToSticky(...) end

function ChatEdit_SecureTabPressed(...) end

function ChatEdit_SendText(...) end

function ChatEdit_SetGameLanguage(...) end

function ChatEdit_SetLastActiveWindow(...) end

function ChatEdit_SetLastTellTarget(...) end

function ChatEdit_SetLastToldTarget(...) end

function ChatEdit_TryInsertChatLink(...) end

function ChatEdit_TryInsertQuestLinkForQuestID(...) end

function ChatEdit_UpdateHeader(...) end

function ChatFrame_ActivateCombatMessages(...) end

function ChatFrame_AddChannel(...) end

function ChatFrame_AddCommunitiesChannel(...) end

function ChatFrame_AddMessageEventFilter(...) end

function ChatFrame_AddMessageGroup(...) end

function ChatFrame_AddNewCommunitiesChannel(...) end

function ChatFrame_AddPrivateMessageTarget(...) end

function ChatFrame_AddSingleMessageType(...) end

function ChatFrame_CanAddChannel(...) end

function ChatFrame_CanChatGroupPerformExpressionExpansion(...) end

function ChatFrame_ChatPageDown(...) end

function ChatFrame_ChatPageUp(...) end

function ChatFrame_ClearChatFocusOverride(...) end

function ChatFrame_ConfigEventHandler(...) end

function ChatFrame_ContainsChannel(...) end

function ChatFrame_ContainsMessageGroup(...) end

function ChatFrame_DisplayChatHelp(...) end

function ChatFrame_DisplayGMOTD(...) end

function ChatFrame_DisplayGameTime(...) end

function ChatFrame_DisplayHelpText(...) end

function ChatFrame_DisplayHelpTextSimple(...) end

function ChatFrame_DisplayLevelUp(...) end

function ChatFrame_DisplayMacroHelpText(...) end

function ChatFrame_DisplaySystemMessage(...) end

function ChatFrame_DisplaySystemMessageInCurrent(...) end

function ChatFrame_DisplaySystemMessageInPrimary(...) end

function ChatFrame_DisplayTimePlayed(...) end

function ChatFrame_DisplayUsageError(...) end

function ChatFrame_ExcludePrivateMessageTarget(...) end

function ChatFrame_GetChatFocusOverride(...) end

function ChatFrame_GetCommunitiesChannelLocalID(...) end

function ChatFrame_GetCommunityAndStreamFromChannel(...) end

function ChatFrame_GetCommunityAndStreamName(...) end

function ChatFrame_GetDefaultChatTarget(...) end

function ChatFrame_GetMessageEventFilters(...) end

function ChatFrame_GetMobileEmbeddedTexture(...) end

function ChatFrame_HandleCautionaryChatMessage(...) end

function ChatFrame_ImportAllListsToHash(...) end

function ChatFrame_ImportEmoteTokensToHash(...) end

function ChatFrame_MessageEventHandler(...) end

function ChatFrame_OnEvent(...) end

function ChatFrame_OnHyperlinkShow(...) end

function ChatFrame_OnLoad(...) end

function ChatFrame_OnMouseWheel(...) end

function ChatFrame_OnUpdate(...) end

function ChatFrame_OpenChat(...) end

function ChatFrame_ReceiveAllPrivateMessages(...) end

function ChatFrame_RegisterForChannels(...) end

function ChatFrame_RegisterForMessages(...) end

function ChatFrame_RemoveAllChannels(...) end

function ChatFrame_RemoveAllMessageGroups(...) end

function ChatFrame_RemoveChannel(...) end

function ChatFrame_RemoveCommunitiesChannel(...) end

function ChatFrame_RemoveExcludePrivateMessageTarget(...) end

function ChatFrame_RemoveMessageEventFilter(...) end

function ChatFrame_RemoveMessageGroup(...) end

function ChatFrame_RemovePrivateMessageTarget(...) end

function ChatFrame_ReplaceIconAndGroupExpressions(...) end

function ChatFrame_ReplyTell(...) end

function ChatFrame_ReplyTell2(...) end

function ChatFrame_ResolveChannelName(...) end

function ChatFrame_ResolvePrefixedChannelName(...) end

function ChatFrame_ScrollDown(...) end

function ChatFrame_ScrollToBottom(...) end

function ChatFrame_ScrollUp(...) end

function ChatFrame_SendBNetTell(...) end

function ChatFrame_SendTell(...) end

function ChatFrame_SendTellWithMessage(...) end

function ChatFrame_SetChatFocusOverride(...) end

function ChatFrame_SetupListProxyTable(...) end

function ChatFrame_SystemEventHandler(...) end

function ChatFrame_TimeBreakDown(...) end

function ChatFrame_TruncateToMaxLength(...) end

function ChatFrame_UpdateColorByID(...) end

function Chat_AddSystemMessage(...) end

function Chat_GetChannelColor(...) end

function Chat_GetChannelShortcutName(...) end

function Chat_GetChatCategory(...) end

function Chat_GetChatFrame(...) end

function Chat_GetColoredChatName(...) end

function Chat_GetCommunitiesChannel(...) end

function Chat_GetCommunitiesChannelColor(...) end

function Chat_GetCommunitiesChannelName(...) end

function Chat_ShouldColorChatByClass(...) end

function CheckBagSettingsTutorial(...) end

function CheckHardcoreGuildLeadStatus(...) end

function ClassTrainerCollapseAllButton_OnClick(...) end

function ClassTrainer_HideSkillDetails(...) end

function ClassTrainer_SelectFirstLearnableSkill(...) end

function ClassTrainer_SetSubTextColor(...) end

function ClassTrainer_SetToClassTrainer(...) end

function ClassTrainer_SetToTradeSkillTrainer(...) end

function ClassTrainer_ShowSkillDetails(...) end

function ClearChargeCooldown(...) end

function ClearPendingGuildBankPermissions(...) end

function ClearPetActionHighlightMarks(...) end

function CloseAuctionStaticPopups(...) end

function CloseBankBagFrames(...) end

function ColorClassesCheckbox_OnClick(...) end

function ColorPaperDollStat(...) end

function CombatLog_AddEvent(...) end

function CombatLog_Color_ColorArrayByEventType(...) end

function CombatLog_Color_ColorArrayBySchool(...) end

function CombatLog_Color_ColorArrayByUnitType(...) end

function CombatLog_Color_ColorStringByEventType(...) end

function CombatLog_Color_ColorStringBySchool(...) end

function CombatLog_Color_ColorStringByUnitType(...) end

function CombatLog_Color_FloatToText(...) end

function CombatLog_Color_HighlightColorArray(...) end

function CombatLog_OnEvent(...) end

function CombatLog_String_DamageResultString(...) end

function CombatLog_String_GetIcon(...) end

function CombatLog_String_PowerType(...) end

function CombatLog_String_SchoolString(...) end

function CombatText_AddMessage(...) end

function CombatText_ClearAnimationList(...) end

function CombatText_FountainScroll(...) end

function CombatText_GetAvailableString(...) end

function CombatText_GetOldestString(...) end

function CombatText_OnEvent(...) end

function CombatText_OnLoad(...) end

function CombatText_OnUpdate(...) end

function CombatText_RemoveMessage(...) end

function CombatText_StandardScroll(...) end

function CombatText_UpdateDisplayedMessages(...) end

function ComboFrame_SetUnit(...) end

function CompactPartyFrame_OnLoad(...) end

function CompactRaidFrameContainer_AddFlaggedUnits(...) end

function CompactRaidFrameContainer_AddGroup(...) end

function CompactRaidFrameContainer_AddGroups(...) end

function CompactRaidFrameContainer_AddPets(...) end

function CompactRaidFrameContainer_AddPlayers(...) end

function CompactRaidFrameContainer_AddUnitFrame(...) end

function CompactRaidFrameContainer_ApplyToFrames(...) end

function CompactRaidFrameContainer_GetUnitFrame(...) end

function CompactRaidFrameContainer_LayoutFrames(...) end

function CompactRaidFrameContainer_OnEvent(...) end

function CompactRaidFrameContainer_OnLoad(...) end

function CompactRaidFrameContainer_OnSizeChanged(...) end

function CompactRaidFrameContainer_ReadyToUpdate(...) end

function CompactRaidFrameContainer_ReleaseAllReservedFrames(...) end

function CompactRaidFrameContainer_SetBorderShown(...) end

function CompactRaidFrameContainer_SetDisplayMainTankAndAssist(...) end

function CompactRaidFrameContainer_SetDisplayPets(...) end

function CompactRaidFrameContainer_SetFlowFilterFunction(...) end

function CompactRaidFrameContainer_SetFlowSortFunction(...) end

function CompactRaidFrameContainer_SetGroupFilterFunction(...) end

function CompactRaidFrameContainer_SetGroupMode(...) end

function CompactRaidFrameContainer_TryUpdate(...) end

function CompactRaidFrameContainer_UpdateBorder(...) end

function CompactRaidFrameContainer_UpdateDisplayedUnits(...) end

function CompactRaidFrameManagerDisplayFrameProfileSelector_OnLoad(...) end

function CompactRaidFrameManagerDisplayFrameProfileSelector_OnShow(...) end

function CompactRaidFrameManager_AttachPartyFrames(...) end

function CompactRaidFrameManager_GetFilterOptions(...) end

function CompactRaidFrameManager_LockContainer(...) end

function CompactRaidFrameManager_ResetContainerPosition(...) end

function CompactRaidFrameManager_ResizeFrame_CheckMagnetism(...) end

function CompactRaidFrameManager_ResizeFrame_LoadPosition(...) end

function CompactRaidFrameManager_ResizeFrame_OnDragStart(...) end

function CompactRaidFrameManager_ResizeFrame_OnDragStop(...) end

function CompactRaidFrameManager_ResizeFrame_OnResizeStart(...) end

function CompactRaidFrameManager_ResizeFrame_OnResizeStop(...) end

function CompactRaidFrameManager_ResizeFrame_OnUpdate(...) end

function CompactRaidFrameManager_ResizeFrame_Reanchor(...) end

function CompactRaidFrameManager_ResizeFrame_SavePosition(...) end

function CompactRaidFrameManager_ResizeFrame_UpdateContainerSize(...) end

function CompactRaidFrameManager_SetFilterOptions(...) end

function CompactRaidFrameManager_SetupRaidMarkerDropdown(...) end

function CompactRaidFrameManager_UnlockContainer(...) end

function CompactRaidFrameManager_UpdateContainerLockVisibility(...) end

function CompactRaidGroup_StartMoving(...) end

function CompactRaidGroup_StopAllMoving(...) end

function CompactRaidGroup_StopMoving(...) end

function CompactUnitFrameProfile_UpdateAutoActivationDisabledLabel(...) end

function CompactUnitFrameProfilesCheckButton_InitializeWidget(...) end

function CompactUnitFrameProfilesCheckButton_OnClick(...) end

function CompactUnitFrameProfilesCheckButton_Update(...) end

function CompactUnitFrameProfilesDropdown_OnLoad(...) end

function CompactUnitFrameProfilesDropdown_OnShow(...) end

function CompactUnitFrameProfilesDropdown_Update(...) end

function CompactUnitFrameProfilesGeneralOptionsFrame_OnShow(...) end

function CompactUnitFrameProfilesNewProfileDialogBaseProfileSelector_OnLoad(...) end

function CompactUnitFrameProfilesNewProfileDialogBaseProfileSelector_OnShow(...) end

function CompactUnitFrameProfilesOption_OnLoad(...) end

function CompactUnitFrameProfilesProfileSelector_OnLoad(...) end

function CompactUnitFrameProfilesProfileSelector_OnShow(...) end

function CompactUnitFrameProfilesSlider_InitializeWidget(...) end

function CompactUnitFrameProfilesSlider_OnValueChanged(...) end

function CompactUnitFrameProfilesSlider_Update(...) end

function CompactUnitFrameProfiles_ActivateRaidProfile(...) end

function CompactUnitFrameProfiles_AfterConfirmUnsavedChanges(...) end

function CompactUnitFrameProfiles_ApplyCurrentSettings(...) end

function CompactUnitFrameProfiles_ApplyProfile(...) end

function CompactUnitFrameProfiles_CancelCallback(...) end

function CompactUnitFrameProfiles_CancelChanges(...) end

function CompactUnitFrameProfiles_CheckAutoActivation(...) end

function CompactUnitFrameProfiles_ConfirmProfileDeletion(...) end

function CompactUnitFrameProfiles_ConfirmUnsavedChanges(...) end

function CompactUnitFrameProfiles_CreateProfile(...) end

function CompactUnitFrameProfiles_DefaultCallback(...) end

function CompactUnitFrameProfiles_GetAutoActivationState(...) end

function CompactUnitFrameProfiles_GetLastActivationType(...) end

function CompactUnitFrameProfiles_HideNewProfileDialog(...) end

function CompactUnitFrameProfiles_HidePopups(...) end

function CompactUnitFrameProfiles_OnEvent(...) end

function CompactUnitFrameProfiles_OnLoad(...) end

function CompactUnitFrameProfiles_ProfileMatchesAutoActivation(...) end

function CompactUnitFrameProfiles_ResetToDefaults(...) end

function CompactUnitFrameProfiles_SaveChanges(...) end

function CompactUnitFrameProfiles_SetLastActivationType(...) end

function CompactUnitFrameProfiles_SetRaidProfile(...) end

function CompactUnitFrameProfiles_ShowNewProfileDialog(...) end

function CompactUnitFrameProfiles_UpdateCurrentPanel(...) end

function CompactUnitFrameProfiles_UpdateManagementButtons(...) end

function CompactUnitFrameProfiles_UpdateNewProfileCreateButton(...) end

function CompactUnitFrameProfiles_ValidateProfilesLoaded(...) end

function CompactUnitFrame_GetHideHealth(...) end

function CompactUnitFrame_IsPlayerAttacking(...) end

function CompactUnitFrame_SetHideHealth(...) end

function CompactUnitFrame_SetupHealPredictions(...) end

function CompactUnitFrame_UpdateBuffs(...) end

function CompactUnitFrame_UpdateClassificationIndicator(...) end

function CompactUnitFrame_UpdateCooldownFrame(...) end

function CompactUnitFrame_UpdateDebuffs(...) end

function CompactUnitFrame_UpdateDispellableDebuffs(...) end

function CompactUnitFrame_UtilIsBossAura(...) end

function CompactUnitFrame_UtilIsPriorityDebuff(...) end

function CompactUnitFrame_UtilShouldDisplayBuff(...) end

function CompactUnitFrame_UtilShouldDisplayDebuff(...) end

function ConquestQueueFrameButton_OnClick(...) end

function ConquestQueueFrameButton_OnEnter(...) end

function ConquestQueueFrameJoinButton_OnClick(...) end

function ConquestQueueFrame_OnEvent(...) end

function ConquestQueueFrame_OnLoad(...) end

function ConquestQueueFrame_OnShow(...) end

function ConquestQueueFrame_SelectButton(...) end

function ConquestQueueFrame_Update(...) end

function ConquestQueueFrame_UpdateConquestBar(...) end

function ConquestQueueFrame_UpdateJoinButton(...) end

function ConsolidatedBuffs_OnEnter(...) end

function ConsolidatedBuffs_OnHide(...) end

function ConsolidatedBuffs_OnShow(...) end

function ConsolidatedBuffs_OnUpdate(...) end

function ConsolidatedBuffs_UpdateAllAnchors(...) end

function ContainerFrameItemButton_BagStatic_AnimateUpdate(...) end

function ContainerFrameItemButton_OnDrag(...) end

function ContainerFrameItemButton_OnLeave(...) end

function ContainerFrameItemButton_OnLoad(...) end

function ContainerFrameItemButton_OnModifiedClick(...) end

function ContainerFrameItemButton_SetForceExtended(...) end

function ContainerFramePortraitButton_OnEnter(...) end

function ContainerFramePortraitButton_OnLeave(...) end

function ContainerFrame_EngravingTargetingModeChanged(...) end

function ContainerFrame_RefreshRuneIcons(...) end

function ContainerFrame_SetBackpackForceExtended(...) end

function ContainerFrame_Update(...) end

function ContainerFrame_UpdateCooldown(...) end

function ContainerFrame_UpdateCooldowns(...) end

function ContainerFrame_UpdateQuestItem(...) end

function ContainerFrame_UpdateSearchBox(...) end

function ContainerFrame_UpdateSearchResults(...) end

function CraftButton_OnClick(...) end

function CraftCollapseAllButton_OnClick(...) end

function CraftFrame_LoadUI(...) end

function CraftFrame_OnEvent(...) end

function CraftFrame_OnHide(...) end

function CraftFrame_OnLoad(...) end

function CraftFrame_OnShow(...) end

function CraftFrame_SetSelection(...) end

function CraftFrame_Update(...) end

function Craft_SetSubTextColor(...) end

function Craft_UpdateTrainingPoints(...) end

function CreateAvailableChatChannelList(...) end

function DeathKnniggetThrobFunction(...) end

function DebuffButton_UpdateAnchors(...) end

function DefaultCompactNamePlateEnemyFrameSetup(...) end

function DefaultCompactNamePlateFrameSetup(...) end

function DefaultCompactNamePlateFrameSetupInternal(...) end

function DefaultCompactNamePlateFriendlyFrameSetup(...) end

function DefaultsButton_OnClick(...) end

function DequoteString(...) end

function Disable_BagButtons(...) end

function DoesInstanceTypeMatchBattlefieldMapSettings(...) end

function DressUpFrame_OnDressModel(...) end

function DressUpSources(...) end

function DurabilityFrame_SetAlerts(...) end

function EmbeddedItemTooltip_OnTooltipSetItem(...) end

function Enable_BagButtons(...) end

function EncounterJournal_OpenJournalLink(...) end

function EngravingFrameSearchBox_OnEditFocusGained(...) end

function EngravingFrameSearchBox_OnEditFocusLost(...) end

function EngravingFrameSearchBox_OnShow(...) end

function EngravingFrameSearchBox_OnTextChanged(...) end

function EngravingFrameSpell_OnClick(...) end

function EngravingFrame_CalculateScroll(...) end

function EngravingFrame_HideAllHeaders(...) end

function EngravingFrame_OnEvent(...) end

function EngravingFrame_OnHide(...) end

function EngravingFrame_OnLoad(...) end

function EngravingFrame_OnShow(...) end

function EngravingFrame_SetupFilterDropdown(...) end

function EngravingFrame_UpdateCollectedLabel(...) end

function EngravingFrame_UpdateRuneList(...) end

function ExhaustionTick_OnEvent(...) end

function ExhaustionTick_OnLoad(...) end

function ExhaustionTick_OnUpdate(...) end

function ExhaustionToolTipText(...) end

function ExpBar_OnEnter(...) end

function ExpBar_Update(...) end

function ExpBar_UpdateTextString(...) end

function Expertise_OnEnter(...) end

function ExtraActionBar_OnEvent(...) end

function ExtraActionBar_OnHide(...) end

function ExtraActionBar_OnShow(...) end

function EyeTemplate_OnUpdate(...) end

function EyeTemplate_StartAnimating(...) end

function EyeTemplate_StopAnimating(...) end

function FCF_UpdateDockPosition(...) end

function FilterButton_SetUp(...) end

function FloatingChatFrame_OnEvent(...) end

function FloatingChatFrame_OnLoad(...) end

function FocusFrame_OnDragStart(...) end

function FocusFrame_OnDragStop(...) end

function FocusFrame_UpdateBuffsOnTop(...) end

function FramePositionDelegate_Override_HandleExtraBars(...) end

function FramePositionDelegate_Override_QuestTimerOffsets(...) end

function FramePositionDelegate_Override_QuestWatchFrameOffsets(...) end

function FramePositionDelegate_Override_VehicleSeatIndicatorOffsets(...) end

function FriendsFrameBattlenetFrame_HideBroadcastFrame(...) end

function FriendsFrameBattlenetFrame_SetBroadcast(...) end

function FriendsFrameBattlenetFrame_ShowBroadcastFrame(...) end

function FriendsFrameBattlenetFrame_UpdateBroadcast(...) end

function FriendsFrameBroadcastInput_OnClearPressed(...) end

function FriendsFrameBroadcastInput_OnEnterPressed(...) end

function FriendsFrameBroadcastInput_OnEscapePressed(...) end

function FriendsFrameBroadcastInput_UpdateDisplay(...) end

function FriendsFrameFriendButton_OnClick(...) end

function FriendsFrameGuildStatusButton_OnClick(...) end

function FriendsFrameIgnoreButton_OnClick(...) end

function FriendsFrameTooltip_Show(...) end

function FriendsFrameWhoButton_OnClick(...) end

function FriendsFrame_BattlenetInvite(...) end

function FriendsFrame_CheckDethroneStatus(...) end

function FriendsFrame_ShouldShowGuildTab(...) end

function FriendsFrame_UpdateFriends(...) end

function FriendsFrame_UpdateGuildTabVisibility(...) end

function FriendsFrame_UpdateVisibleTabs(...) end

function FriendsFriendsButton_OnClick(...) end

function FriendsFriendsFrame_Reset(...) end

function FriendsFriendsFrame_SendRequest(...) end

function FriendsFriendsList_Update(...) end

function FriendsList_GetScrollFrameTopButton(...) end

function FriendsTabHeader_ClickTab(...) end

function FriendsTabHeader_ResizeTabs(...) end

function GameMenuFrame_OnShow(...) end

function GameMenuFrame_UpdateStoreButtonState(...) end

function GameMenuFrame_UpdateVisibleButtons(...) end

function GameTimeFrame_Update(...) end

function GameTimeFrame_UpdateTooltip(...) end

function GameTooltip_AdvanceSecondaryCompareItem(...) end

function GameTooltip_AnchorComparisonTooltips(...) end

function GameTooltip_ClearInsertedFrames(...) end

function GameTooltip_InitializeComparisonTooltips(...) end

function GameTooltip_OnTooltipSetItem(...) end

function GameTooltip_OnTooltipSetShoppingItem(...) end

function GameTooltip_OnTooltipSetSpell(...) end

function GameTooltip_OnTooltipSetUnit(...) end

function GameTooltip_ShowCompareSpell(...) end

function GameTooltip_UpdateStyle(...) end

function GetActiveRaidProfile(...) end

function GetBackgroundTexCoordsForRole(...) end

function GetBattlefieldMapInstanceType(...) end

function GetChallengeBestTime(...) end

function GetChallengeModeMapPlayerStats(...) end

function GetChatConfigChannelInfo(...) end

function GetChatTimestampFormat(...) end

function GetColoredName(...) end

function GetDeathStaticPopup(...) end

function GetDisplayedAllyFrames(...) end

function GetEffectiveAuctionsScrollFrameOffset(...) end

function GetEffectivePlayerMaxLevel(...) end

function GetEffectiveSelectedOwnerAuctionItemIndex(...) end

function GetHolidayBGHonorCurrencyBonuses(...) end

function GetKeyRingSize(...) end

function GetLocalizedNumberAbbreviationData(...) end

function GetMeleeMissChance(...) end

function GetPowerEnumFromEnergizeString(...) end

function GetQuestFrameSize(...) end

function GetQuestIDFromLogIndex(...) end

function GetQuestLogIndexByName(...) end

function GetRaceAtlas(...) end

function GetRandomBGHonorCurrencyBonuses(...) end

function GetRangedMissChance(...) end

function GetSpellMissChance(...) end

function GetStatInfoInCategory(...) end

function GetTemplateForChatConfigFrame(...) end

function GetTexCoordsForRoleSmall(...) end

function GetTexCoordsForRoleSmallCircle(...) end

function GetTimeStringFromSeconds(...) end

function GetTimerTextColor(...) end

function GlyphFrameGlyph_OnClick(...) end

function GlyphFrameGlyph_OnEnter(...) end

function GlyphFrameGlyph_OnLeave(...) end

function GlyphFrameGlyph_OnLoad(...) end

function GlyphFrameGlyph_OnUpdate(...) end

function GlyphFrameGlyph_SetGlyphType(...) end

function GlyphFrameGlyph_UpdateSlot(...) end

function GlyphFrameHeader_OnClick(...) end

function GlyphFrameSpell_OnClick(...) end

function GlyphFrameSpell_OnEnter(...) end

function GlyphFrame_CalculateScroll(...) end

function GlyphFrame_LoadUI(...) end

function GlyphFrame_OnEnter(...) end

function GlyphFrame_OnEvent(...) end

function GlyphFrame_OnHide(...) end

function GlyphFrame_OnLeave(...) end

function GlyphFrame_OnLoad(...) end

function GlyphFrame_OnShow(...) end

function GlyphFrame_OnTextChanged(...) end

function GlyphFrame_OnUpdate(...) end

function GlyphFrame_Open(...) end

function GlyphFrame_PulseGlow(...) end

function GlyphFrame_SetupFilterDropdown(...) end

function GlyphFrame_StartSlotAnimation(...) end

function GlyphFrame_StopSlotAnimation(...) end

function GlyphFrame_Toggle(...) end

function GlyphFrame_Update(...) end

function GlyphFrame_UpdateGlyphList(...) end

function GossipFrameActiveQuestsUpdate(...) end

function GroupLootFrame_OpenNewFrame(...) end

function GroupsButton_Update(...) end

function GuildBankLogScroll(...) end

function GuildBankTabPermissionsTab_OnClick(...) end

function GuildControlCheckboxUpdate(...) end

function GuildControlPopupAcceptButton_OnClick(...) end

function GuildControlPopupFrameAddRankButton_OnUpdate(...) end

function GuildControlPopupFrameDropdownButton_ClickedRank(...) end

function GuildControlPopupFrameDropdown_OnLoad(...) end

function GuildControlPopupFrameRemoveRankButton_OnClick(...) end

function GuildControlPopupFrameRemoveRankButton_OnUpdate(...) end

function GuildControlPopupFrame_HideGuildBankOptions(...) end

function GuildControlPopupFrame_Initialize(...) end

function GuildControlPopupFrame_OnEvent(...) end

function GuildControlPopupFrame_OnHide(...) end

function GuildControlPopupFrame_OnLoad(...) end

function GuildControlPopupFrame_OnShow(...) end

function GuildControlPopupFrame_SetAllCheckboxesEnabled(...) end

function GuildControlPopup_UpdateBankTabOptions(...) end

function GuildControlPopupframe_Update(...) end

function GuildControlPopupframe_UpdateGoldWithdrawalOptions(...) end

function GuildControlPopupframe_UpdateItemWithdrawalOptions(...) end

function GuildEventLog_Update(...) end

function GuildFrameControlButton_OnUpdate(...) end

function GuildFrameGuildListToggleButton_OnClick(...) end

function GuildFramePopup_HideAll(...) end

function GuildFramePopup_Show(...) end

function GuildFrame_CheckName(...) end

function GuildFrame_GetLastOnline(...) end

function GuildInstanceDifficulty_OnEnter(...) end

function GuildStatus_Update(...) end

function HasPetActionHighlightMark(...) end

function HasUnseenCommunityInvitations(...) end

function HideClassColors(...) end

function HidePartyFrame(...) end

function HidePetActionBar(...) end

function HideStats(...) end

function HideTextStatusBarText(...) end

function HideWatchBarText(...) end

function HideWatchedReputationBarText(...) end

function HonorFrame_GetCurrencyFrame(...) end

function HonorFrame_SetGuild(...) end

function HonorFrame_SetLevel(...) end

function HonorFrame_Update(...) end

function HonorFrame_UpdateShown(...) end

function HonorQueueFrameBonusFrame_OnShow(...) end

function HonorQueueFrameBonusFrame_OnUpdate(...) end

function HonorQueueFrameBonusFrame_SelectButton(...) end

function HonorQueueFrameBonusFrame_SetButtonState(...) end

function HonorQueueFrameBonusFrame_Update(...) end

function HonorQueueFrameBonusFrame_UpdateExcludedBattlegrounds(...) end

function HonorQueueFrameBonusFrame_UpdateWorldPVPTime(...) end

function HonorQueueFrameSpecificBattlegroundButton_OnClick(...) end

function HonorQueueFrameSpecificList_FindAndSelectBattleground(...) end

function HonorQueueFrameSpecificList_ResetInfo(...) end

function HonorQueueFrameSpecificList_Update(...) end

function HonorQueueFrameTypeDropDown_Initialize(...) end

function HonorQueueFrameTypeDropDown_OnClick(...) end

function HonorQueueFrame_OnEvent(...) end

function HonorQueueFrame_OnLoad(...) end

function HonorQueueFrame_Queue(...) end

function HonorQueueFrame_SetType(...) end

function HonorQueueFrame_UpdateQueueButtons(...) end

function IgnoreList_SetHeader(...) end

function InGuildCheck(...) end

function IncludedBattlegroundsDropDown_Initialize(...) end

function IncludedBattlegroundsDropDown_OnClick(...) end

function IncludedBattlegroundsDropDown_OnLoad(...) end

function IncludedBattlegroundsDropDown_Toggle(...) end

function InspectHonorFrame_OnEvent(...) end

function InspectHonorFrame_OnLoad(...) end

function InspectHonorFrame_OnShow(...) end

function InspectHonorFrame_Update(...) end

function InviteToGroup(...) end

function IsAlreadyInQueue(...) end

function IsInProvingGround(...) end

function IsMacroEditBox(...) end

function IsSelectedOwnerAuctionItemIndexAToken(...) end

function ItemAnim_OnAnimFinished(...) end

function ItemAnim_OnEvent(...) end

function ItemAnim_OnLoad(...) end

function ItemUpgradeFrame_GetUpgradeInfo(...) end

function JumpToCollectionsTab(...) end

function KeyBindingButton_OnClick(...) end

function KeyBindingFrame_AttemptKeybind(...) end

function KeyBindingFrame_CancelBinding(...) end

function KeyBindingFrame_ChangeBindingProfile(...) end

function KeyBindingFrame_LoadCategories(...) end

function KeyBindingFrame_LoadKeyBindingButtons(...) end

function KeyBindingFrame_OnEvent(...) end

function KeyBindingFrame_OnHide(...) end

function KeyBindingFrame_OnKeyDown(...) end

function KeyBindingFrame_OnLoad(...) end

function KeyBindingFrame_OnMouseWheel(...) end

function KeyBindingFrame_OnShow(...) end

function KeyBindingFrame_ResetBindingsToDefault(...) end

function KeyBindingFrame_SetBinding(...) end

function KeyBindingFrame_SetSelected(...) end

function KeyBindingFrame_UnbindKey(...) end

function KeyBindingFrame_Update(...) end

function KeyBindingFrame_UpdateHeaderText(...) end

function KeyBindingFrame_UpdateUnbindKey(...) end

function KeybindingsCategoryListButton_OnClick(...) end

function LFDQueueFrameSpecificList_Update(...) end

function LFGBrowseActivityButton_OnClick(...) end

function LFGBrowseActivityDropDownResetButton_OnClick(...) end

function LFGBrowseActivityDropDown_Initialize(...) end

function LFGBrowseActivityDropDown_IsAnyValueSelectedForActivityGroup(...) end

function LFGBrowseActivityDropDown_Reset(...) end

function LFGBrowseActivityDropDown_SetAllValuesForActivityGroup(...) end

function LFGBrowseActivityDropDown_UpdateHeader(...) end

function LFGBrowseActivityDropDown_ValueIsSelected(...) end

function LFGBrowseActivityDropDown_ValueReset(...) end

function LFGBrowseActivityDropDown_ValueSetSelected(...) end

function LFGBrowseActivityDropDown_ValueToggleSelected(...) end

function LFGBrowseActivityGroupButton_OnClick(...) end

function LFGBrowseCategoryButton_OnClick(...) end

function LFGBrowseCategoryDropDown_Initialize(...) end

function LFGBrowseCategoryDropDown_Reset(...) end

function LFGBrowseGroupDataDisplayComment_Update(...) end

function LFGBrowseGroupDataDisplayEnumerate_Update(...) end

function LFGBrowseGroupDataDisplayPlayerCount_Update(...) end

function LFGBrowseGroupDataDisplayRoleCount_Update(...) end

function LFGBrowseGroupDataDisplaySolo_Update(...) end

function LFGBrowseGroupDataDisplay_Update(...) end

function LFGBrowseGroupInviteButton_OnClick(...) end

function LFGBrowseSearchButton_OnClick(...) end

function LFGBrowseSearchEntryTooltip_Load(...) end

function LFGBrowseSearchEntryTooltip_UpdateAndShow(...) end

function LFGBrowseSearchEntry_Init(...) end

function LFGBrowseSearchEntry_OnClick(...) end

function LFGBrowseSearchEntry_OnEnter(...) end

function LFGBrowseSearchEntry_OnEvent(...) end

function LFGBrowseSearchEntry_OnLeave(...) end

function LFGBrowseSearchEntry_OnLoad(...) end

function LFGBrowseSearchEntry_SetSelection(...) end

function LFGBrowseSearchEntry_Update(...) end

function LFGBrowseSendMessageButton_OnClick(...) end

function LFGBrowseUtil_GetBestDisplayTypeForActivityIDs(...) end

function LFGBrowseUtil_GetInviteActionForResult(...) end

function LFGBrowseUtil_MapRoleStatesToRoleIcons(...) end

function LFGBrowseUtil_ReportAdvertisement(...) end

function LFGBrowseUtil_ReportListing(...) end

function LFGBrowseUtil_SortSearchResults(...) end

function LFGBrowse_DoSearch(...) end

function LFGDropDown_OnClick(...) end

function LFGDropDown_OnEnter(...) end

function LFGDropDown_OnLeave(...) end

function LFGListEntryCreation_OnPlayStyleSelected(...) end

function LFGListEntryCreation_SetPlaystyleLabelTextFromActivityInfo(...) end

function LFGListSearchEntry_SetSelected(...) end

function LFGListUtil_SortActivitiesByShortname(...) end

function LFGListingActivityView_ActivityFiltersChanged(...) end

function LFGListingActivityView_CanPostWithCurrentComment(...) end

function LFGListingActivityView_InitActivityButton(...) end

function LFGListingActivityView_InitActivityGroupButton(...) end

function LFGListingActivityView_OnLoad(...) end

function LFGListingActivityView_OnShow(...) end

function LFGListingActivityView_UpdateActivities(...) end

function LFGListingBackButton_OnClick(...) end

function LFGListingBackButton_OnEvent(...) end

function LFGListingBackButton_OnLoad(...) end

function LFGListingBackButton_UpdateText(...) end

function LFGListingCategorySelectionButton_OnClick(...) end

function LFGListingCategorySelection_ActivityFiltersChanged(...) end

function LFGListingCategorySelection_AddButton(...) end

function LFGListingCategorySelection_OnShow(...) end

function LFGListingCategorySelection_UpdateCategoryButtons(...) end

function LFGListingComment_GetComment(...) end

function LFGListingComment_OnMouseDown(...) end

function LFGListingComment_OnTextChanged(...) end

function LFGListingLockedView_OnEvent(...) end

function LFGListingLockedView_OnLoad(...) end

function LFGListingLockedView_RefreshContent(...) end

function LFGListingLockedView_SafeAcquireFrame(...) end

function LFGListingLockedView_SetLineContent(...) end

function LFGListingNewPlayerFriendlyButtonCheckButton_OnClick(...) end

function LFGListingNewPlayerFriendlyButtonCheckButton_OnShow(...) end

function LFGListingPostButton_OnClick(...) end

function LFGListingPostButton_OnEvent(...) end

function LFGListingPostButton_OnLoad(...) end

function LFGListingPostButton_UpdateText(...) end

function LFGListingRoleDropDownButton_OnClick(...) end

function LFGListingRoleDropDown_Initialize(...) end

function LFGListingRoleIcon_UpdateRoleTexture(...) end

function LFGListingRolePollButton_OnClick(...) end

function LFGListingRolePollButton_OnEvent(...) end

function LFGListingRolePollButton_OnLoad(...) end

function LFGListingRolePollButton_UpdateEnableState(...) end

function LFGListingUtil_CanEditListing(...) end

function LFGMicroButton_OnLoad(...) end

function LFGParentFrameTab1_OnClick(...) end

function LFGParentFrameTab2_OnClick(...) end

function LFGParentFrame_SearchActiveEntry(...) end

function LFGUtil_GetActivityGroupForActivity(...) end

function LFGUtil_GetActivityInfoName(...) end

function LFGUtil_GetFilteredActivities(...) end

function LFGUtil_OrganizeActivitiesByActivityGroup(...) end

function LFGUtil_SortActivityGroupIDs(...) end

function LFGUtil_SortActivityIDs(...) end

function LevelUpDisplaySide_AnimStep(...) end

function LevelUpDisplaySide_OnHide(...) end

function LevelUpDisplaySide_OnShow(...) end

function LevelUpDisplaySide_Remove(...) end

function LevelUpDisplay_AddBattlePetCaptureEvent(...) end

function LevelUpDisplay_AddBattlePetLevelUpEvent(...) end

function LevelUpDisplay_AddBattlePetLootReward(...) end

function LevelUpDisplay_AddBattlePetTrapUpgradeEvent(...) end

function LevelUpDisplay_AnimOut(...) end

function LevelUpDisplay_AnimOutFinished(...) end

function LevelUpDisplay_AnimStep(...) end

function LevelUpDisplay_BuildCharacterList(...) end

function LevelUpDisplay_BuildEmptyList(...) end

function LevelUpDisplay_BuildGuildList(...) end

function LevelUpDisplay_BuildPetBattleWinnerList(...) end

function LevelUpDisplay_BuildPetList(...) end

function LevelUpDisplay_ChatPrint(...) end

function LevelUpDisplay_CreateOrAppendItem(...) end

function LevelUpDisplay_OnEvent(...) end

function LevelUpDisplay_OnLoad(...) end

function LevelUpDisplay_PlayScenario(...) end

function LevelUpDisplay_Show(...) end

function LevelUpDisplay_ShowSideDisplay(...) end

function LevelUpDisplay_Start(...) end

function LevelUpDisplay_StopAllAnims(...) end

function LocalizeNumberAbbreviation_Asian(...) end

function LocalizekoKR(...) end

function LockPetActionBar(...) end

function LootButton_OnClick(...) end

function LootFrame_AdjustTextLocation(...) end

function LootFrame_Close(...) end

function LootFrame_InitAutoLootTable(...) end

function LootFrame_OnEvent(...) end

function LootFrame_OnHide(...) end

function LootFrame_OnLoad(...) end

function LootFrame_OnShow(...) end

function LootFrame_OnUpdate(...) end

function LootFrame_PageDown(...) end

function LootFrame_PageUp(...) end

function LootFrame_Show(...) end

function LootFrame_Update(...) end

function LootFrame_UpdateButton(...) end

function LootHistoryDropdown_GiveMasterLoot(...) end

function LootHistoryFrameUtil_ShouldDisplayPlayer(...) end

function LootHistoryFrame_CollapseAll(...) end

function LootHistoryFrame_FullUpdate(...) end

function LootHistoryFrame_GetPlayerFrame(...) end

function LootHistoryFrame_OnEvent(...) end

function LootHistoryFrame_OnHide(...) end

function LootHistoryFrame_OnLoad(...) end

function LootHistoryFrame_OpenToRoll(...) end

function LootHistoryFrame_RecycleAllPlayers(...) end

function LootHistoryFrame_ResetHighlights(...) end

function LootHistoryFrame_SetRollExpanded(...) end

function LootHistoryFrame_ToggleRollExpanded(...) end

function LootHistoryFrame_ToggleWithRoll(...) end

function LootHistoryFrame_UpdateItemFrame(...) end

function LootHistoryFrame_UpdatePlayerFrame(...) end

function LootHistoryFrame_UpdatePlayerFrames(...) end

function LootHistoryFrame_UpdatePlayerRoll(...) end

function LootHistoryPlayerFrame_OnClick(...) end

function LootItem_OnEnter(...) end

function MainMenuBarBackpackButton_OnEvent(...) end

function MainMenuBarBackpackButton_UpdateFreeSlots(...) end

function MainMenuBarVehicleLeaveButton_OnClicked(...) end

function MainMenuBarVehicleLeaveButton_OnEnter(...) end

function MainMenuBarVehicleLeaveButton_OnEvent(...) end

function MainMenuBarVehicleLeaveButton_OnLoad(...) end

function MainMenuBarVehicleLeaveButton_Update(...) end

function MainMenuBar_GetNumArtifactTraitsPurchasableFromXP(...) end

function MainMenuBar_OnEvent(...) end

function MainMenuBar_OnLoad(...) end

function MainMenuBar_UpdateExperienceBars(...) end

function MainMenuBar_UpdateKeyRing(...) end

function MainMenuMicroButton_SetNormal(...) end

function MainMenuMicroButton_SetPushed(...) end

function MainMenuTrackingBar_Configure(...) end

function ManageBackpackTokenFrame(...) end

function MarkCommunitiesInvitiationDisplayed(...) end

function MeleeHitChance_OnEnter(...) end

function MessageFrameScrollButton_OnLoad(...) end

function MessageFrameScrollButton_OnUpdate(...) end

function MicroButtonAlert_CreateAlert(...) end

function MicroButtonAlert_OnHide(...) end

function MicroButtonAlert_OnLoad(...) end

function MicroButtonAlert_OnShow(...) end

function MicroButtonAlert_SetText(...) end

function MicroButton_KioskModeDisable(...) end

function MicroButton_OnEnter(...) end

function MiniMapBattlefieldFrame_OnClick(...) end

function MiniMapBattlefieldFrame_ShowContextMenu(...) end

function MiniMapInstanceDifficulty_OnEvent(...) end

function MiniMapInstanceDifficulty_Update(...) end

function MiniMapLFGFrame_OnClick(...) end

function MiniMapLFGFrame_OnEnter(...) end

function MiniMapLFGFrame_OnEvent(...) end

function MiniMapLFGFrame_OnLeave(...) end

function MiniMapLFGFrame_OnLoad(...) end

function MiniMapTrackingShineFadeIn(...) end

function MiniMapTrackingShineFadeOut(...) end

function MiniMapTracking_Update(...) end

function MinimapButton_OnMouseDown(...) end

function MinimapButton_OnMouseUp(...) end

function Minimap_OnClick(...) end

function Minimap_OnEvent(...) end

function Minimap_OnLoad(...) end

function Minimap_SetPing(...) end

function Minimap_UpdateRotationSetting(...) end

function Minimap_ZoomInClick(...) end

function Minimap_ZoomOutClick(...) end

function MirrorTimerFrame_OnEvent(...) end

function MirrorTimerFrame_OnLoad(...) end

function MirrorTimerFrame_OnUpdate(...) end

function MirrorTimer_Show(...) end

function ModelControlButton_OnMouseDown(...) end

function ModelControlButton_OnMouseUp(...) end

function Model_OnHide(...) end

function Model_OnUpdate(...) end

function Model_Reset(...) end

function Model_RotateLeft(...) end

function Model_RotateRight(...) end

function Model_SetDefaultRotation(...) end

function Model_StartPanning(...) end

function Model_StopPanning(...) end

function MoneyFrame_AccumulateAlignmentWidths(...) end

function MoneyFrame_ResetAlignment(...) end

function MoneyFrame_UpdateAlignment(...) end

function MountJournal_EvaluateListHelpTip(...) end

function MoveMicroButtons(...) end

function MultiActionBar_UpdateGrid(...) end

function MultiActionBar_UpdateGridVisibility(...) end

function MultibarGrid_IsVisible(...) end

function Nameplate_CastBar_AdjustPosition(...) end

function OkayButton_OnClick(...) end

function OpenGlyphFrame(...) end

function OpenQuestMapLog(...) end

function OpenStackSplitFrame(...) end

function OverrideActionBar_CalcSize(...) end

function OverrideActionBar_GetMicroButtonAnchor(...) end

function OverrideActionBar_Leave(...) end

function OverrideActionBar_OnEvent(...) end

function OverrideActionBar_OnLoad(...) end

function OverrideActionBar_OnShow(...) end

function OverrideActionBar_SetPitchValue(...) end

function OverrideActionBar_SetSkin(...) end

function OverrideActionBar_Setup(...) end

function OverrideActionBar_UpdateMicroButtons(...) end

function OverrideActionBar_UpdateSkin(...) end

function OverrideActionBar_UpdateXpBar(...) end

function OverrideMicroMenuPosition(...) end

function PVEFrame_OnEvent(...) end

function PVEFrame_OnLoad(...) end

function PVEFrame_OnShow(...) end

function PVPBannerCustomization_Left(...) end

function PVPBannerCustomization_Right(...) end

function PVPBannerFrameAcceptButton_OnClick(...) end

function PVPBannerFrameCloseButton_OnHide(...) end

function PVPBannerFrameCloseButton_OnShow(...) end

function PVPBannerFrame_OnShow(...) end

function PVPBannerFrame_OpenColorPicker(...) end

function PVPBannerFrame_SaveBanner(...) end

function PVPBannerFrame_SetBannerColor(...) end

function PVPBannerFrame_SetBorderColor(...) end

function PVPBannerFrame_SetEmblemColor(...) end

function PVPConquestFrame_ButtonClicked(...) end

function PVPConquestFrame_ConfigureConquestRewards(...) end

function PVPConquestFrame_OnEvent(...) end

function PVPConquestFrame_OnLoad(...) end

function PVPConquestFrame_OnShow(...) end

function PVPConquestFrame_Update(...) end

function PVPFrameConquestBar_OnEnter(...) end

function PVPFrameToggleButton_OnClick(...) end

function PVPFrame_ExpansionSpecificOnEvent(...) end

function PVPFrame_ExpansionSpecificOnLoad(...) end

function PVPFrame_JoinClicked(...) end

function PVPFrame_OnEvent(...) end

function PVPFrame_OnHide(...) end

function PVPFrame_OnLoad(...) end

function PVPFrame_OnShow(...) end

function PVPFrame_OpenMenu(...) end

function PVPFrame_RoleButtonClicked(...) end

function PVPFrame_SetFaction(...) end

function PVPFrame_SetRoles(...) end

function PVPFrame_TabClicked(...) end

function PVPFrame_Update(...) end

function PVPFrame_UpdateCurrency(...) end

function PVPFrame_UpdateSelectedRoles(...) end

function PVPFrame_UpdateTabs(...) end

function PVPHonorFrame_OnEvent(...) end

function PVPHonorFrame_OnLoad(...) end

function PVPHonorFrame_OnShow(...) end

function PVPHonorFrame_ResetInfo(...) end

function PVPHonorFrame_UpdateGroupAvailable(...) end

function PVPHonor_ButtonClicked(...) end

function PVPHonor_GetRandomBattlegroundInfo(...) end

function PVPHonor_Update(...) end

function PVPHonor_UpdateInfo(...) end

function PVPHonor_UpdateQueueStatus(...) end

function PVPHonor_UpdateRandomInfo(...) end

function PVPQueueFrame_CanChangeRoles(...) end

function PVPQueueFrame_RoleButtonClicked(...) end

function PVPQueueFrame_SetRoles(...) end

function PVPQueueFrame_UpdateAvailableRoles(...) end

function PVPQueueFrame_UpdateCurrencies(...) end

function PVPQueueFrame_UpdateRolesChangeable(...) end

function PVPQueueFrame_UpdateSelectedRoles(...) end

function PVPQueue_UpdateRandomInfo(...) end

function PVPRoleButtonTemplate_OnEnter(...) end

function PVPRoleButtonTemplate_OnLoad(...) end

function PVPStandard_OnLoad(...) end

function PVPTeamDetailsAddTeamMember_OnClick(...) end

function PVPTeamDetailsAddTeamMember_OnEnter(...) end

function PVPTeamDetailsAddTeamMember_OnLeave(...) end

function PVPTeamDetailsButton_OnClick(...) end

function PVPTeamDetailsToggleButton_OnClick(...) end

function PVPTeamDetails_OnHide(...) end

function PVPTeamDetails_OnShow(...) end

function PVPTeamDetails_Update(...) end

function PVPTeamManagementFrame_ToggleSeasonal(...) end

function PVPTeam_OnClick(...) end

function PVPTeam_OnEnter(...) end

function PVPTeam_OnLeave(...) end

function PVPTeam_OnMouseDown(...) end

function PVPTeam_OnMouseUp(...) end

function PVPTeam_SoloUpdate(...) end

function PVPTeam_TeamsUpdate(...) end

function PVPTeam_Update(...) end

function PVP_DisableRoleButton(...) end

function PVP_EnableRoleButton(...) end

function PVP_PermanentlyDisableRoleButton(...) end

function PVP_UpdateAvailableRoleButton(...) end

function PVP_UpdateAvailableRoles(...) end

function PaperDollFrame_CollapseStatCategory(...) end

function PaperDollFrame_ExpandStatCategory(...) end

function PaperDollFrame_SetAttackBothHands(...) end

function PaperDollFrame_SetCombatManaRegen(...) end

function PaperDollFrame_SetDefense(...) end

function PaperDollFrame_SetExpertise(...) end

function PaperDollFrame_SetGuild(...) end

function PaperDollFrame_SetMeleeCritChance(...) end

function PaperDollFrame_SetMeleeDPS(...) end

function PaperDollFrame_SetMeleeHaste(...) end

function PaperDollFrame_SetMeleeHitChance(...) end

function PaperDollFrame_SetPrimaryStats(...) end

function PaperDollFrame_SetPvpPower(...) end

function PaperDollFrame_SetRangedAttack(...) end

function PaperDollFrame_SetRangedAttackPower(...) end

function PaperDollFrame_SetRangedAttackSpeed(...) end

function PaperDollFrame_SetRangedCritChance(...) end

function PaperDollFrame_SetRangedDPS(...) end

function PaperDollFrame_SetRangedDamage(...) end

function PaperDollFrame_SetRangedHaste(...) end

function PaperDollFrame_SetRangedHitChance(...) end

function PaperDollFrame_SetResistance(...) end

function PaperDollFrame_SetResistances(...) end

function PaperDollFrame_SetSpellBonusDamage(...) end

function PaperDollFrame_SetSpellBonusHealing(...) end

function PaperDollFrame_SetSpellCritChance(...) end

function PaperDollFrame_SetSpellHaste(...) end

function PaperDollFrame_SetSpellHitChance(...) end

function PaperDollFrame_SetSpellPenetration(...) end

function PaperDollFrame_UpdateStatCategory(...) end

function PaperDollFrame_UpdateStatScrollChildHeight(...) end

function PaperDollItemSlotButton_EngravingTargetingModeChanged(...) end

function PaperDollItemSlotButton_RefreshRuneIcon(...) end

function PaperDollStatCategory_OnDragStart(...) end

function PaperDollStatCategory_OnDragStop(...) end

function PaperDollStatCategory_OnDragUpdate(...) end

function PaperDoll_FindCategoryById(...) end

function PaperDoll_InitStatCategories(...) end

function PaperDoll_MoveCategoryDown(...) end

function PaperDoll_MoveCategoryUp(...) end

function PaperDoll_SaveStatCategoryOrder(...) end

function PaperDoll_UpdateCategoryPositions(...) end

function PartyMemberBackground_SaveOpacity(...) end

function PartyMemberBackground_SetOpacity(...) end

function PartyMemberBackground_ToggleOpacity(...) end

function PartyMemberBuffTooltip_Update(...) end

function PartyMemberFrame_OnEvent(...) end

function PartyMemberFrame_OnLoad(...) end

function PartyMemberFrame_OnUpdate(...) end

function PartyMemberFrame_RefreshPetDebuffs(...) end

function PartyMemberFrame_ToPlayerArt(...) end

function PartyMemberFrame_ToVehicleArt(...) end

function PartyMemberFrame_UpdateArt(...) end

function PartyMemberFrame_UpdateLeader(...) end

function PartyMemberFrame_UpdateMember(...) end

function PartyMemberFrame_UpdateMemberHealth(...) end

function PartyMemberFrame_UpdateNotPresentIcon(...) end

function PartyMemberFrame_UpdateOnlineStatus(...) end

function PartyMemberFrame_UpdatePet(...) end

function PartyMemberFrame_UpdatePvPStatus(...) end

function PartyMemberFrame_UpdateReadyCheck(...) end

function PartyMemberFrame_UpdateStatusBarText(...) end

function PartyMemberFrame_UpdateVoiceActivityNotification(...) end

function PartyMemberFrame_VoiceActivityNotificationCreatedCallback(...) end

function PartyMemberHealthCheck(...) end

function PetActionBarFrame_IsAboveStance(...) end

function PetActionBarFrame_OnUpdate(...) end

function PetActionBar_HideGrid(...) end

function PetActionBar_OnEvent(...) end

function PetActionBar_OnHide(...) end

function PetActionBar_OnLoad(...) end

function PetActionBar_OnShow(...) end

function PetActionBar_ShowGrid(...) end

function PetActionBar_Update(...) end

function PetActionBar_UpdateCooldowns(...) end

function PetActionButtonDown(...) end

function PetActionButtonUp(...) end

function PetActionButton_IsFlashing(...) end

function PetActionButton_OnClick(...) end

function PetActionButton_OnDragStart(...) end

function PetActionButton_OnEnter(...) end

function PetActionButton_OnEvent(...) end

function PetActionButton_OnLeave(...) end

function PetActionButton_OnLoad(...) end

function PetActionButton_OnModifiedClick(...) end

function PetActionButton_OnReceiveDrag(...) end

function PetActionButton_OnUpdate(...) end

function PetActionButton_SetHotkeys(...) end

function PetActionButton_StartFlash(...) end

function PetActionButton_StopFlash(...) end

function PetCastingBarFrame_OnEvent(...) end

function PetCastingBarFrame_OnLoad(...) end

function PetExpBar_Update(...) end

function PetFrame_AdjustPoint(...) end

function PetFrame_OnEvent(...) end

function PetFrame_OnLoad(...) end

function PetFrame_OnUpdate(...) end

function PetFrame_SetHappiness(...) end

function PetFrame_Update(...) end

function PetPaperDollFrame_OnEvent(...) end

function PetPaperDollFrame_OnHide(...) end

function PetPaperDollFrame_OnLoad(...) end

function PetPaperDollFrame_OnShow(...) end

function PetPaperDollFrame_QueuedUpdate(...) end

function PetPaperDollFrame_SetResistances(...) end

function PetPaperDollFrame_SetStats(...) end

function PetPaperDollFrame_Update(...) end

function PetPaperDollFrame_UpdateIsAvailable(...) end

function PetStableSlot_Lock_OnEnter(...) end

function PetStableSlot_OnReceiveDrag(...) end

function PetStable_GetPetSlot(...) end

function PetStable_NextPage(...) end

function PetStable_NoPetsAllowed(...) end

function PetStable_OnEvent(...) end

function PetStable_OnHide(...) end

function PetStable_OnLoad(...) end

function PetStable_OnMouseWheel(...) end

function PetStable_OnShow(...) end

function PetStable_PrevPage(...) end

function PetStable_SetSelectedPetInfo(...) end

function PetStable_Update(...) end

function PetStable_UpdatePetModelScene(...) end

function PetStable_UpdateSlot(...) end

function PetTab_Update(...) end

function PetitionFrameRenameButton_OnClick(...) end

function PlayerFrameMultiGroupFrame_OnEvent(...) end

function PlayerFrameMultiGroupFrame_OnLoad(...) end

function PlayerFrameMultiGroupframe_OnEnter(...) end

function PlayerFrame_AnimFinished(...) end

function PlayerFrame_AnimateOut(...) end

function PlayerFrame_HideVehicleTexture(...) end

function PlayerFrame_IsAnimatedOut(...) end

function PlayerFrame_OnDragStart(...) end

function PlayerFrame_OnDragStop(...) end

function PlayerFrame_ResetPosition(...) end

function PlayerFrame_ResetUserPlacedPosition(...) end

function PlayerFrame_SequenceFinished(...) end

function PlayerFrame_SetLocked(...) end

function PlayerFrame_SetRunicPower(...) end

function PlayerFrame_SetupDeathKnightLayout(...) end

function PlayerFrame_ShowVehicleTexture(...) end

function PlayerFrame_UpdateLayout(...) end

function PlayerFrame_UpdateLevelTextAnchor(...) end

function PlayerFrame_UpdatePVPTimer(...) end

function PlayerGlyphTab_OnClick(...) end

function PlayerGlyphTab_OnEvent(...) end

function PlayerGlyphTab_OnLoad(...) end

function PlayerSpecTab_Load(...) end

function PlayerSpecTab_OnClick(...) end

function PlayerSpecTab_OnEnter(...) end

function PlayerSpecTab_Update(...) end

function PlayerTalentFrameActivateButton_OnClick(...) end

function PlayerTalentFrameActivateButton_OnEvent(...) end

function PlayerTalentFrameActivateButton_OnHide(...) end

function PlayerTalentFrameActivateButton_OnLoad(...) end

function PlayerTalentFrameActivateButton_OnShow(...) end

function PlayerTalentFrameActivateButton_Update(...) end

function PlayerTalentFrameDownArrow_OnClick(...) end

function PlayerTalentFrameLearnButton_OnClick(...) end

function PlayerTalentFrameLearnButton_OnEnter(...) end

function PlayerTalentFrameResetButton_OnClick(...) end

function PlayerTalentFrameResetButton_OnEnter(...) end

function PlayerTalentFrameSpec_OnLoad(...) end

function PlayerTalentFrameSpec_OnShow(...) end

function PlayerTalentFrameTab_OnClick(...) end

function PlayerTalentFrameTab_OnEnter(...) end

function PlayerTalentFrameTab_OnLoad(...) end

function PlayerTalentFrameTalent_OnClick(...) end

function PlayerTalentFrameTalent_OnDrag(...) end

function PlayerTalentFrameTalent_OnEnter(...) end

function PlayerTalentFrameTalent_OnEvent(...) end

function PlayerTalentFrameTalents_OnLoad(...) end

function PlayerTalentFrame_ClearTalentSelections(...) end

function PlayerTalentFrame_Close(...) end

function PlayerTalentFrame_CreateSpecSpellButton(...) end

function PlayerTalentFrame_GetTalentSelections(...) end

function PlayerTalentFrame_GetTutorial(...) end

function PlayerTalentFrame_HideGlyphFrame(...) end

function PlayerTalentFrame_HidePetSpecTab(...) end

function PlayerTalentFrame_HideSpecsTab(...) end

function PlayerTalentFrame_HideTalentTab(...) end

function PlayerTalentFrame_LoadUI(...) end

function PlayerTalentFrame_OnClickClose(...) end

function PlayerTalentFrame_OnEvent(...) end

function PlayerTalentFrame_OnHide(...) end

function PlayerTalentFrame_OnLoad(...) end

function PlayerTalentFrame_OnShow(...) end

function PlayerTalentFrame_Open(...) end

function PlayerTalentFrame_OpenGlyphFrame(...) end

function PlayerTalentFrame_PetSpec_OnLoad(...) end

function PlayerTalentFrame_PostUpdateActiveSpec(...) end

function PlayerTalentFrame_Refresh(...) end

function PlayerTalentFrame_RefreshClearInfo(...) end

function PlayerTalentFrame_SelectTalent(...) end

function PlayerTalentFrame_ShowGlyphFrame(...) end

function PlayerTalentFrame_ShowTalentTab(...) end

function PlayerTalentFrame_ShowsPetSpecTab(...) end

function PlayerTalentFrame_ShowsSpecTab(...) end

function PlayerTalentFrame_Toggle(...) end

function PlayerTalentFrame_ToggleGlyphFrame(...) end

function PlayerTalentFrame_ToggleTutorial(...) end

function PlayerTalentFrame_Update(...) end

function PlayerTalentFrame_UpdateActiveSpec(...) end

function PlayerTalentFrame_UpdateControls(...) end

function PlayerTalentFrame_UpdateSpecFrame(...) end

function PlayerTalentFrame_UpdateSpecs(...) end

function PlayerTalentFrame_UpdateTabs(...) end

function PlayerTalentFrame_UpdateTitleText(...) end

function PlayerTalentTab_GetBestDefaultTab(...) end

function PlayerTalentTab_OnClick(...) end

function PlayerTalentTab_OnEvent(...) end

function PlayerTalentTab_OnLoad(...) end

function PossessBar_Update(...) end

function PossessBar_UpdateState(...) end

function PossessButton_OnClick(...) end

function PossessButton_OnEnter(...) end

function ProductChoiceFrameInsetClaimButton_OnClick(...) end

function ProductChoiceFrameItem_SetUpDisplay(...) end

function ProductChoiceFrame_ClaimItem(...) end

function ProductChoiceFrame_OnEvent(...) end

function ProductChoiceFrame_OnFriendsListShown(...) end

function ProductChoiceFrame_OnLoad(...) end

function ProductChoiceFrame_OnMouseWheel(...) end

function ProductChoiceFrame_OnShow(...) end

function ProductChoiceFrame_PageClick(...) end

function ProductChoiceFrame_RefreshConfirmationModel(...) end

function ProductChoiceFrame_SetUp(...) end

function ProductChoiceFrame_ShowAlerts(...) end

function ProductChoiceFrame_ShowConfirmation(...) end

function ProductChoiceFrame_StartRotating(...) end

function ProductChoiceFrame_StopRotating(...) end

function ProductChoiceFrame_Update(...) end

function ProductChoiceItemDisplay_OnMouseDown(...) end

function ProductChoiceItemDisplay_OnMouseUp(...) end

function ProductChoiceItem_OnClick(...) end

function PutKeyInKeyRing(...) end

function QuestChoice_LoadUI(...) end

function QuestDetailsFrame_OnHide(...) end

function QuestDetailsFrame_OnShow(...) end

function QuestFrameDetailPanel_OnHide(...) end

function QuestFrameDetailPanel_OnUpdate(...) end

function QuestFrameItems_Update(...) end

function QuestFrame_SetAsLastShown(...) end

function QuestInfoRewardItemCodeTemplate_OnClick(...) end

function QuestInfoRewardItemCodeTemplate_OnEnter(...) end

function QuestInfo_FadeInAlphaDependentText(...) end

function QuestInfo_HideAlphaDependentText(...) end

function QuestInfo_ShowAlphaDependentText(...) end

function QuestInfo_ToggleRewardElement(...) end

function QuestLogCollapseAllButton_OnClick(...) end

function QuestLogControlPanel_UpdatePosition(...) end

function QuestLogControlPanel_UpdateState(...) end

function QuestLogDetailFrame_AttachToQuestLog(...) end

function QuestLogDetailFrame_DetachFromQuestLog(...) end

function QuestLogDetailFrame_OnHide(...) end

function QuestLogDetailFrame_OnLoad(...) end

function QuestLogDetailFrame_OnShow(...) end

function QuestLogFrameTrackButton_OnClick(...) end

function QuestLogListScrollFrame_OnLoad(...) end

function QuestLogRewardItem_OnClick(...) end

function QuestLogShowMapPOI_UpdatePosition(...) end

function QuestLogTitleButton_OnClick(...) end

function QuestLogTitleButton_OnEnter(...) end

function QuestLogTitleButton_OnEvent(...) end

function QuestLogTitleButton_OnLeave(...) end

function QuestLogTitleButton_OnLoad(...) end

function QuestLogTitleButton_Resize(...) end

function QuestLogUpdateQuestCount(...) end

function QuestLog_GetFirstSelectableQuest(...) end

function QuestLog_OnEvent(...) end

function QuestLog_OnHide(...) end

function QuestLog_OnLoad(...) end

function QuestLog_OnShow(...) end

function QuestLog_OnUpdate(...) end

function QuestLog_OpenToQuest(...) end

function QuestLog_SetFirstValidSelection(...) end

function QuestLog_SetSelection(...) end

function QuestLog_Update(...) end

function QuestLog_UpdateMapButton(...) end

function QuestLog_UpdatePartyInfoTooltip(...) end

function QuestLog_UpdatePortrait(...) end

function QuestLog_UpdateQuestDetails(...) end

function QuestMapFrame_PingQuestID(...) end

function QuestMoneyFrame_OnLoad(...) end

function QuestPOIButton_EvaluateManagedHighlight(...) end

function QuestPOIButton_OnClick(...) end

function QuestPOIButton_OnEnter(...) end

function QuestPOIButton_OnLeave(...) end

function QuestPOIButton_OnMouseDown(...) end

function QuestPOIButton_OnMouseUp(...) end

function QuestPOI_CalculateNumericTexCoords(...) end

function QuestPOI_ClearSelection(...) end

function QuestPOI_FindButton(...) end

function QuestPOI_GetButton(...) end

function QuestPOI_GetButtonAlpha(...) end

function QuestPOI_GetCampaignAtlasInfoNormal(...) end

function QuestPOI_GetCampaignAtlasInfoPushed(...) end

function QuestPOI_GetPinScale(...) end

function QuestPOI_GetQuestCompleteAtlas(...) end

function QuestPOI_GetStyleFromQuestData(...) end

function QuestPOI_GetTextureInfoHighlight(...) end

function QuestPOI_GetTextureInfoNormal(...) end

function QuestPOI_GetTextureInfoPushed(...) end

function QuestPOI_HideAllButtons(...) end

function QuestPOI_HideUnusedButtons(...) end

function QuestPOI_Initialize(...) end

function QuestPOI_ResetUsage(...) end

function QuestPOI_SelectButton(...) end

function QuestPOI_SelectButtonByQuestID(...) end

function QuestPOI_SetNumber(...) end

function QuestPOI_SetPinScale(...) end

function QuestPOI_UpdateButtonStyle(...) end

function QuestPOI_UpdateNormalStyle(...) end

function QuestPOI_UpdateNormalStyleTexture(...) end

function QuestPOI_UpdateNumericStyle(...) end

function QuestPOI_UpdateNumericStyleTextures(...) end

function QuestUtils_GetQuestTagTextureCoords(...) end

function QuestUtils_GetQuestTypeTextureMarkupString(...) end

function QuestWatch_OnLogin(...) end

function QuestWatch_Update(...) end

function QuestsFrame_OnLoad(...) end

function QueueStatusDropdown_Show(...) end

function RaidFrameReadyCheckButton_Update(...) end

function RaidOptionsFrame_UpdatePartyFrames(...) end

function RaidPulloutFrameTemplate_CreateContextMenu(...) end

function RangedHitChance_OnEnter(...) end

function RealPartyIsFull(...) end

function ReforgeFrame_NewStat_Initialize(...) end

function ReforgeFrame_OldStat_Initialize(...) end

function ReforgingFrame_AddItemClick(...) end

function ReforgingFrame_GetStatRow(...) end

function ReforgingFrame_Hide(...) end

function ReforgingFrame_OnEvent(...) end

function ReforgingFrame_OnFinishedAnim(...) end

function ReforgingFrame_OnHide(...) end

function ReforgingFrame_OnLoad(...) end

function ReforgingFrame_OnShow(...) end

function ReforgingFrame_ReforgeClick(...) end

function ReforgingFrame_RestoreClick(...) end

function ReforgingFrame_Show(...) end

function ReforgingFrame_Update(...) end

function Reforging_LoadUI(...) end

function RefreshBuffs(...) end

function RefreshBuffsOrDebuffs(...) end

function RefreshDebuffs(...) end

function RefreshRuneFrameControlButton(...) end

function ReputationBar_DrawHorizontalLine(...) end

function ReputationBar_DrawVerticalLine(...) end

function ReputationBar_OnClick(...) end

function ReputationFrame_OnEvent(...) end

function ReputationFrame_OnHide(...) end

function ReputationFrame_OnLoad(...) end

function ReputationFrame_OnShow(...) end

function ReputationFrame_SetRowType(...) end

function ReputationFrame_Update(...) end

function ReputationWatchBar_Update(...) end

function ReputationWatchBar_UpdateMaxLevel(...) end

function RequestChallengeModeLeaders(...) end

function RolePollPopupRoleButton_SetNotRecommended(...) end

function RolePollPopupRoleButton_SetRecommended(...) end

function RuneButton_OnEnter(...) end

function RuneButton_OnLeave(...) end

function RuneButton_OnLoad(...) end

function RuneButton_ShineFadeIn(...) end

function RuneButton_ShineFadeOut(...) end

function RuneButton_Update(...) end

function RuneFrameControlButton_OnClick(...) end

function RuneFrameControlButton_OnLoad(...) end

function RuneFrameControlButton_OnShow(...) end

function RuneFrame_OnEvent(...) end

function RuneFrame_OnLoad(...) end

function RuneHeader_OnClick(...) end

function RuneSpellButton_OnEnter(...) end

function SavePendingGuildBankTabPermissions(...) end

function ScorePlayer_OnClick(...) end

function ScorePlayer_OnMouseUp(...) end

function SeatIndicator_Pulse(...) end

function SendMailEditBox_OnLoad(...) end

function SendMailEditBox_OnTabPressed(...) end

function SetActiveRaidProfile(...) end

function SetAllInventorySlotsFiltered(...) end

function SetEffectiveSelectedOwnerAuctionItemIndex(...) end

function SetItemButtonSubTexture(...) end

function SetLookingForGroupUIAvailable(...) end

function SetMaxStackSize(...) end

function SetPendingGuildBankTabPermissions(...) end

function SetPendingGuildBankTabWithdraw(...) end

function SetSpectatorModeForOtherFrames(...) end

function SetTalentButtonLocation(...) end

function SetTextStatusBarText(...) end

function SetTextStatusBarTextPrefix(...) end

function SetTextStatusBarTextZeroText(...) end

function Setup_Dropdown(...) end

function ShakeFrame(...) end

function ShakeFrameRandom(...) end

function ShouldDisplaySpecIconInBackground(...) end

function ShouldDisplaySpecTextInGlyphSubtext(...) end

function ShowFriendshipReputationTooltip(...) end

function ShowHardcoreGuildHandoff(...) end

function ShowLFGParentFrame(...) end

function ShowOptionsPanel(...) end

function ShowPVPQueueUI(...) end

function ShowPartyFrame(...) end

function ShowPetActionBar(...) end

function ShowTextStatusBarText(...) end

function ShowWatchBarText(...) end

function ShowWatchedReputationBarText(...) end

function SideDressUpFrame_OnHide(...) end

function SideDressUpFrame_OnShow(...) end

function SkillBar_OnClick(...) end

function SkillDetailFrame_SetStatusBar(...) end

function SkillFrame_OnEvent(...) end

function SkillFrame_OnLoad(...) end

function SkillFrame_OnShow(...) end

function SkillFrame_SetStatusBar(...) end

function SkillFrame_UpdateSkills(...) end

function SocialsMicroButton_UpdateNotificationIcon(...) end

function SortButton_UpdateArrow(...) end

function SpecButton_OnClick(...) end

function SpecButton_OnEnter(...) end

function SpecButton_OnLeave(...) end

function SpellBookFrameTabButton_OnClick(...) end

function SpellBookNextPageButton_OnClick(...) end

function SpellBookPrevPageButton_OnClick(...) end

function SpellBookSkillLineTab_OnClick(...) end

function SpellBook_GetAutoCastShine(...) end

function SpellBook_GetButtonForID(...) end

function SpellBook_GetCoreAbilityButton(...) end

function SpellBook_GetCoreAbilitySpecTab(...) end

function SpellBook_GetCurrentPage(...) end

function SpellBook_GetSpellBookSlot(...) end

function SpellBook_GetWhatChangedItem(...) end

function SpellBook_ReleaseAutoCastShine(...) end

function SpellBook_UpdateCoreAbilitiesTab(...) end

function SpellBook_UpdatePetTab(...) end

function SpellBook_UpdatePlayerTab(...) end

function SpellBook_UpdateProfTab(...) end

function SpellBook_UpdateWhatHasChangedTab(...) end

function SpellHitChance_OnEnter(...) end

function SplitLink(...) end

function StackSplitFrameCancel_Click(...) end

function StackSplitFrameLeft_Click(...) end

function StackSplitFrameOkay_Click(...) end

function StackSplitFrameRight_Click(...) end

function StackSplitFrame_OnChar(...) end

function StackSplitFrame_OnHide(...) end

function StackSplitFrame_OnKeyDown(...) end

function StackSplitFrame_OnKeyUp(...) end

function StanceBar_OnEvent(...) end

function StanceBar_OnLoad(...) end

function StanceBar_Select(...) end

function StanceBar_Update(...) end

function StanceBar_UpdateState(...) end

function StanceButton_OnEnter(...) end

function StartChargeCooldown(...) end

function Stat_OnClick(...) end

function Stat_SetButtonChecked(...) end

function StoreFrame_OpenGameTimeCategory(...) end

function StoreFrame_SelectActivateProduct(...) end

function StorePurchaseAlertFrame_OnClick(...) end

function StorePurchaseAlertFrame_SetUp(...) end

function StoreShowPreview(...) end

function SubstituteChatMessageBeforeSend(...) end

function TabardCharacterModelFrame_OnLoad(...) end

function TabardCharacterModelFrame_OnUpdate(...) end

function TabardCharacterModelRotateLeftButton_OnClick(...) end

function TabardCharacterModelRotateRightButton_OnClick(...) end

function TalentFrame_DrawLines(...) end

function TalentFrame_GetArrowTexture(...) end

function TalentFrame_GetArrowTextureCount(...) end

function TalentFrame_GetBranchTexture(...) end

function TalentFrame_GetBranchTextureCount(...) end

function TalentFrame_ResetArrowTextureCount(...) end

function TalentFrame_ResetBranchTextureCount(...) end

function TalentFrame_ResetBranches(...) end

function TalentFrame_SetArrowTexture(...) end

function TalentFrame_SetBranchTexture(...) end

function TalentFrame_SetPrereqs(...) end

function TalentFrame_UpdateSpecInfoCacheFromSpecializationInfo(...) end

function TalentFrame_UpdateSpecInfoCacheFromTalentTabInfo(...) end

function TalentFrame_UpdateTalentPoints(...) end

function TalentMicroButton_OnEvent(...) end

function TargetFrame_CheckBattlePet(...) end

function TargetFrame_CheckClassification(...) end

function TargetFrame_CheckDead(...) end

function TargetFrame_CheckDishonorableKill(...) end

function TargetFrame_CheckFaction(...) end

function TargetFrame_CheckLevel(...) end

function TargetFrame_CreateSpellbar(...) end

function TargetFrame_CreateTargetofTarget(...) end

function TargetFrame_HealthUpdate(...) end

function TargetFrame_OnCVarChanged(...) end

function TargetFrame_OnDragStart(...) end

function TargetFrame_OnDragStop(...) end

function TargetFrame_OnEvent(...) end

function TargetFrame_OnHide(...) end

function TargetFrame_OnLoad(...) end

function TargetFrame_OnUpdate(...) end

function TargetFrame_OnVariablesLoaded(...) end

function TargetFrame_ResetUserPlacedPosition(...) end

function TargetFrame_SetLocked(...) end

function TargetFrame_ShouldShowDebuffs(...) end

function TargetFrame_Update(...) end

function TargetFrame_UpdateAuraPositions(...) end

function TargetFrame_UpdateAuras(...) end

function TargetFrame_UpdateBuffsOnTop(...) end

function TargetFrame_UpdateLevelTextAnchor(...) end

function TargetFrame_UpdateRaidTargetIcon(...) end

function Target_Spellbar_AdjustPosition(...) end

function Target_Spellbar_OnEvent(...) end

function TargetofTargetHealthCheck(...) end

function TargetofTarget_CheckDead(...) end

function TargetofTarget_OnHide(...) end

function TargetofTarget_Update(...) end

function TargetofTarget_UpdateDebuffs(...) end

function TempEnchantButton_OnClick(...) end

function TempEnchantButton_OnEnter(...) end

function TempEnchantButton_OnLoad(...) end

function TempEnchantButton_OnUpdate(...) end

function TemporaryEnchantFrame_Hide(...) end

function TemporaryEnchantFrame_OnUpdate(...) end

function TemporaryEnchantFrame_Update(...) end

function TextStatusBar_Initialize(...) end

function TextStatusBar_OnEvent(...) end

function TextStatusBar_OnValueChanged(...) end

function TextStatusBar_UpdateTextString(...) end

function TextStatusBar_UpdateTextStringWithValues(...) end

function TimeManagerClockButton_UpdateShowClockSetting(...) end

function ToggleChatChannel(...) end

function ToggleEngravingFrame(...) end

function ToggleFramerate(...) end

function ToggleGlyphFrame(...) end

function ToggleGuildEventLog(...) end

function ToggleGuildInfoFrame(...) end

function ToggleLFGParentFrame(...) end

function ToggleMiniMapRotation(...) end

function TogglePVPFrame(...) end

function ToggleQuestMap(...) end

function ToggleSpellBook(...) end

function ToggleTalentFrame(...) end

function ToggleWorldStateScoreFrame(...) end

function TokenButtonLinkButton_OnClick(...) end

function TokenButton_OnClick(...) end

function TokenButton_OnLoad(...) end

function TokenFramePopup_CloseIfHidden(...) end

function TokenFrame_OnEvent(...) end

function TokenFrame_OnLoad(...) end

function TokenFrame_OnShow(...) end

function TokenFrame_Update(...) end

function TokenFrame_UpdatePopup(...) end

function TradeSkilSubSkillRank_Set(...) end

function TradeSkilSubSkillRank_StartFlash(...) end

function TradeSkilSubSkillRank_StopFlash(...) end

function TradeSkillCollapseAllButton_OnClick(...) end

function TradeSkillFrameButton_OnEnter(...) end

function TradeSkillFrameButton_OnLeave(...) end

function TradeSkillFrameDecrement_OnClick(...) end

function TradeSkillFrameIncrement_OnClick(...) end

function TradeSkillFrame_Hide(...) end

function TradeSkillFrame_InitFilterMenu(...) end

function TradeSkillFrame_LoadUI(...) end

function TradeSkillFrame_OnEvent(...) end

function TradeSkillFrame_OnHide(...) end

function TradeSkillFrame_OnLoad(...) end

function TradeSkillFrame_OnMouseWheel(...) end

function TradeSkillFrame_OnShow(...) end

function TradeSkillFrame_OnUpdate(...) end

function TradeSkillFrame_PlaytimeUpdate(...) end

function TradeSkillFrame_SetSelection(...) end

function TradeSkillFrame_SetupInvSlotDropdown(...) end

function TradeSkillFrame_SetupSubClassDropdown(...) end

function TradeSkillFrame_Update(...) end

function TradeSkillItem_OnEnter(...) end

function TradeSkillSearch_OnTextChanged(...) end

function TradeSkillSkillButton_OnClick(...) end

function TutorialFrame_CheckIntro(...) end

function TutorialFrame_GetAlertButton(...) end

function TutorialFrame_HideAllAlerts(...) end

function UIDropDownMenu_StartCounting(...) end

function UIDropDownMenu_StopCounting(...) end

function UIParent_ManageFramePosition(...) end

function UIParent_OnUpdate(...) end

function UnbindButton_OnClick(...) end

function UnitFrameUtil_UpdateFillBar(...) end

function UnitFrameUtil_UpdateFillBarBase(...) end

function UnitFrameUtil_UpdateManaFillBar(...) end

function UnitFrame_IsHealPredictionEnabled(...) end

function UnlockPetActionBar(...) end

function UpdateArenaEnemyBackground(...) end

function UpdateBagButtonHighlight(...) end

function UpdateBagSlotStatus(...) end

function UpdateColorClassCheckboxes(...) end

function UpdateDeposit(...) end

function UpdateFloatingCombatTextSafe(...) end

function UpdateMainMenuBarArt(...) end

function UpdateMaximumButtons(...) end

function UpdateMenuBarTop(...) end

function UpdateMicroButtonsParent(...) end

function UpdatePartyMemberBackground(...) end

function UpdatePetActionHighlightMarks(...) end

function UpdateProfessionButton(...) end

function UpdateStackSplitFrame(...) end

function UpdateUIParentRelativeToDebugMenu(...) end

function VehicleSeatIndicatorButton_OnClick(...) end

function VehicleSeatIndicatorButton_OnEnter(...) end

function VehicleSeatIndicatorButton_OnLeave(...) end

function VehicleSeatIndicator_OnEvent(...) end

function VehicleSeatIndicator_OnLoad(...) end

function VehicleSeatIndicator_SetUpVehicle(...) end

function VehicleSeatIndicator_UnloadTextures(...) end

function VehicleSeatIndicator_Update(...) end

function WarGameButtonHeader_OnClick(...) end

function WarGameButton_OnClick(...) end

function WarGameButton_OnEnter(...) end

function WarGameButton_OnLeave(...) end

function WarGameStartButton_GetErrorTooltip(...) end

function WarGameStartButton_OnClick(...) end

function WarGameStartButton_OnEnter(...) end

function WarGameStartButton_Update(...) end

function WarGamesFrame_FindAndSelectBattleground(...) end

function WarGamesFrame_InitButton(...) end

function WarGamesFrame_OnEvent(...) end

function WarGamesFrame_OnLoad(...) end

function WarGamesFrame_OnShow(...) end

function WarGamesFrame_Update(...) end

function WarGamesQueueFrame_GetTopButton(...) end

function WarGamesQueueFrame_OnEvent(...) end

function WarGamesQueueFrame_OnLoad(...) end

function WarGamesQueueFrame_OnShow(...) end

function WarGamesQueueFrame_Update(...) end

function WardrobeCollectionFrameModelDropdown_SetFavorite(...) end

function WatchFrameAutoQuest_AddPopUp(...) end

function WatchFrameAutoQuest_ClearPopUp(...) end

function WatchFrameAutoQuest_ClearPopUpByLogIndex(...) end

function WatchFrameAutoQuest_DisplayAutoQuestPopUps(...) end

function WatchFrameAutoQuest_GetOrCreateFrame(...) end

function WatchFrameAutoQuest_OnUpdate(...) end

function WatchFrameAutoQuest_SlideIn(...) end

function WatchFrameHeader_OnClick(...) end

function WatchFrameItem_OnClick(...) end

function WatchFrameItem_OnEnter(...) end

function WatchFrameItem_OnEvent(...) end

function WatchFrameItem_OnHide(...) end

function WatchFrameItem_OnLoad(...) end

function WatchFrameItem_OnShow(...) end

function WatchFrameItem_OnUpdate(...) end

function WatchFrameItem_UpdateCooldown(...) end

function WatchFrameLineTemplate_OnLoad(...) end

function WatchFrameLines_AddUpdateFunction(...) end

function WatchFrameLines_OnUpdate(...) end

function WatchFrameLines_RemoveUpdateFunction(...) end

function WatchFrameLinkButtonTemplate_Highlight(...) end

function WatchFrameLinkButtonTemplate_OnClick(...) end

function WatchFrameLinkButtonTemplate_OnLeftClick(...) end

function WatchFrameLinkButtonTemplate_ShowContextMenu(...) end

function WatchFrameQuestPOI_OnClick(...) end

function WatchFrameScenarioBonusHeader_OnEnter(...) end

function WatchFrameScenarioBonusHeader_OnUpdate(...) end

function WatchFrameScenario_DisplayScenario(...) end

function WatchFrameScenario_GetCriteriaLine(...) end

function WatchFrameScenario_OnBeginSlideOut(...) end

function WatchFrameScenario_OnFinishSlideIn(...) end

function WatchFrameScenario_OnFinishSlideOut(...) end

function WatchFrameScenario_PlayCriteriaAnimation(...) end

function WatchFrameScenario_SetLine(...) end

function WatchFrameScenario_SlideIn(...) end

function WatchFrameScenario_SlideOut(...) end

function WatchFrameScenario_StopAllAnimations(...) end

function WatchFrameScenario_StopCriteriaAnimations(...) end

function WatchFrameScenario_UpdateScenario(...) end

function WatchFrameSlideInFrame_OnUpdate(...) end

function WatchFrame_AbandonQuest(...) end

function WatchFrame_AddObjectiveHandler(...) end

function WatchFrame_ClearDisplay(...) end

function WatchFrame_Collapse(...) end

function WatchFrame_CollapseExpandButton_OnClick(...) end

function WatchFrame_DisplayQuestTimers(...) end

function WatchFrame_DisplayTrackedAchievements(...) end

function WatchFrame_DisplayTrackedQuests(...) end

function WatchFrame_Expand(...) end

function WatchFrame_GetCurrentMapQuests(...) end

function WatchFrame_GetVisibleIndex(...) end

function WatchFrame_HandleDisplayQuestTimers(...) end

function WatchFrame_HandleDisplayTrackedAchievements(...) end

function WatchFrame_HandleQuestTimerUpdate(...) end

function WatchFrame_MoveQuest(...) end

function WatchFrame_OnEvent(...) end

function WatchFrame_OnLoad(...) end

function WatchFrame_OnSizeChanged(...) end

function WatchFrame_OnUpdate(...) end

function WatchFrame_OpenAchievementFrame(...) end

function WatchFrame_OpenMapToQuest(...) end

function WatchFrame_OpenQuestLog(...) end

function WatchFrame_QuestTimerUpdateFunction(...) end

function WatchFrame_RemoveObjectiveHandler(...) end

function WatchFrame_SetFilter(...) end

function WatchFrame_SetLine(...) end

function WatchFrame_SetSorting(...) end

function WatchFrame_SetWidth(...) end

function WatchFrame_ShareQuest(...) end

function WatchFrame_SlideInFrame(...) end

function WatchFrame_StopTrackingAchievement(...) end

function WatchFrame_StopTrackingQuest(...) end

function WatchFrame_Update(...) end

function WatchFrame_UpdateTimedAchievements(...) end

function WhoFrameEditBox_OnEnterPressed(...) end

function WorldMapContinentDropdown_OnLoad(...) end

function WorldMapContinentDropdown_OnShow(...) end

function WorldMapFrame_ChangeOpacity(...) end

function WorldMapFrame_SaveOpacity(...) end

function WorldMapFrame_SetMapName(...) end

function WorldMapFrame_SetOpacity(...) end

function WorldMapLevelDown_OnClick(...) end

function WorldMapLevelUp_OnClick(...) end

function WorldMapTitleButton_OnClick(...) end

function WorldMapTitleButton_OnDragStart(...) end

function WorldMapTitleButton_OnDragStop(...) end

function WorldMapTitleButton_OnLoad(...) end

function WorldMapTitleDropdown_Reset(...) end

function WorldMapTitleDropdown_ToggleOpacity(...) end

function WorldMapTrackQuest_Toggle(...) end

function WorldMapZoneDropdown_OnLoad(...) end

function WorldMapZoneDropdown_OnShow(...) end

function WorldMapZoneMinimapDropdown_GetText(...) end

function WorldMapZoneMinimapDropdown_OnEnter(...) end

function WorldMapZoneMinimapDropdown_OnLeave(...) end

function WorldMapZoneMinimapDropdown_OnLoad(...) end

function WorldMapZoneMinimapDropdown_OnShow(...) end

function WorldStateChallengeModeAnim_OnFinished(...) end

function WorldStateChallengeModeFrame_UpdateMedal(...) end

function WorldStateChallengeModeFrame_UpdateValues(...) end

function WorldStateChallengeModeTimer_OnUpdate(...) end

function WorldStateChallengeMode_CheckTimers(...) end

function WorldStateChallengeMode_DisplayTimers(...) end

function WorldStateChallengeMode_HideTimer(...) end

function WorldStateChallengeMode_OnEvent(...) end

function WorldStateChallengeMode_OnLoad(...) end

function WorldStateChallengeMode_ShowTimer(...) end

function WorldStateProvingGroundsAnim_OnFinished(...) end

function WorldStateProvingGroundsFrame_UpdateValues(...) end

function WorldStateProvingGroundsTimer_OnUpdate(...) end

function WorldStateProvingGrounds_CheckTimers(...) end

function WorldStateProvingGrounds_DisplayTimers(...) end

function WorldStateProvingGrounds_HideTimer(...) end

function WorldStateProvingGrounds_OnEvent(...) end

function WorldStateProvingGrounds_ShowTimer(...) end

function WorldStateScoreFrameTab_OnClick(...) end

function WorldStateScoreFrame_CanSeeDamageAndHealing(...) end

function WorldStateScoreFrame_OnClose(...) end

function WorldStateScoreFrame_OnEvent(...) end

function WorldStateScoreFrame_OnHide(...) end

function WorldStateScoreFrame_OnLoad(...) end

function WorldStateScoreFrame_OnShow(...) end

function WorldStateScoreFrame_OnVerticalScroll(...) end

function WorldStateScoreFrame_Resize(...) end

function WorldStateScoreFrame_Update(...) end

function _QuestLog_ToggleQuestWatch(...) end

function _QuestMap_HighlightQuest(...) end

function _QuestMap_HighlightSelectedQuest(...) end

function escapePatternSymbols(...) end

-- Classic-only global frames

---@type any
ArenaEnemyFrames = nil

---@type any
ArenaPrepFrames = nil

---@type any
ArenaRegistrarFrame = nil

---@type any
ArtifactRelicHelpBox = nil

---@type any
AuctionFrame = nil

---@type any
AuctionProgressFrame = nil

---@type any
AutoCastShineManager = nil

---@type any
AzeriteItemInBagHelpBox = nil

---@type any
BagHelpBox = nil

---@type any
BarberShopBannerFrame = nil

---@type any
BarbersChoiceConfirmFrame = nil

---@type any
BattlefieldFrame = nil

---@type any
CastingBarFrame = nil

---@type any
ChallengesLeaderboardFrame = nil

---@type any
CommentatorEventAlertsFrame = nil

---@type any
CompactUnitFrameProfiles = nil

---@type any
ConsolidatedBuffs = nil

---@type any
ConsolidatedBuffsTooltip = nil

---@type any
ContainerFrame10 = nil

---@type any
ContainerFrame11 = nil

---@type any
ContainerFrame12 = nil

---@type any
ContainerFrame13 = nil

---@type any
ContainerFrame7 = nil

---@type any
ContainerFrame8 = nil

---@type any
ContainerFrame9 = nil

---@type any
CraftFrame = nil

---@type any
EngravingFrame = nil

---@type any
FramerateLabel = nil

---@type any
FramerateText = nil

---@type any
GuildControlPopupFrame = nil

---@type any
KeyBindingFrame = nil

---@type any
KeyBindingFrameBindingButtonTemplate = nil

---@type any
KeyBindingFrameBindingButtonTemplateWithLabel = nil

---@type any
KeyBindingFrameBindingTemplate = nil

---@type any
LFGBrowseSearchEntryTooltip = nil

---@type any
LFGListGroupDataDisplayTemplate = nil

---@type any
LFGParentFrame = nil

---@type any
LevelUpDisplay = nil

---@type any
LevelUpDisplaySide = nil

---@type any
LootHistoryFrame = nil

---@type any
MirrorTimer1 = nil

---@type any
MirrorTimer2 = nil

---@type any
MirrorTimer3 = nil

---@type any
PVPBannerFrame = nil

---@type any
PVPFrame = nil

---@type any
PartyMemberBackground = nil

---@type any
PartyMemberFrame1 = nil

---@type any
PartyMemberFrame2 = nil

---@type any
PartyMemberFrame3 = nil

---@type any
PartyMemberFrame4 = nil

---@type any
PetStableFrame = nil

---@type any
PlayerReportFrame = nil

---@type any
PlayerTalentFrame = nil

---@type any
ProductChoiceFrame = nil

---@type any
QuestChoiceFrame = nil

---@type any
QuestLogControlPanel = nil

---@type any
QuestLogDetailFrame = nil

---@type any
QuestLogFrame = nil

---@type any
QuestLogHighlightFrame = nil

---@type any
QuestMapHighlightFrame = nil

---@type any
QuestMapSelectFrame = nil

---@type any
QuestNPCModel = nil

---@type any
QuestTimerFrame = nil

---@type any
QuestWatchFrame = nil

---@type any
ReforgingFrame = nil

---@type any
SmallTextTooltip = nil

---@type any
SpellBookFrame = nil

---@type any
TalentMicroButtonAlert = nil

---@type any
TemporaryEnchantFrame = nil

---@type any
TradeSkillFrame = nil

---@type any
TutorialFrameAlertButton1 = nil

---@type any
TutorialFrameAlertButton10 = nil

---@type any
TutorialFrameAlertButton2 = nil

---@type any
TutorialFrameAlertButton3 = nil

---@type any
TutorialFrameAlertButton4 = nil

---@type any
TutorialFrameAlertButton5 = nil

---@type any
TutorialFrameAlertButton6 = nil

---@type any
TutorialFrameAlertButton7 = nil

---@type any
TutorialFrameAlertButton8 = nil

---@type any
TutorialFrameAlertButton9 = nil

---@type any
TutorialFrameParent = nil

---@type any
VerticalMultiBarsContainer = nil

---@type any
WatchFrame = nil

---@type any
WorldMapCompareTooltip1 = nil

---@type any
WorldMapCompareTooltip2 = nil

---@type any
WorldMapScreenAnchor = nil

---@type any
WorldMapTooltip = nil

---@type any
WorldStateChallengeModeFrame = nil

---@type any
WorldStateChallengeModeTimer = nil

---@type any
WorldStateProvingGroundsFrame = nil

---@type any
WorldStateProvingGroundsTimer = nil

---@type any
WorldStateScoreFrame = nil
