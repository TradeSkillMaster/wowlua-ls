  if details.itemID == Syndicator.Constants.BattlePetCageID and details.itemLink:find("battlepet:") then
    local petID = details.itemLink:match("battlepet:(%d+)")
    details.itemName = C_PetJournal.GetPetInfoBySpeciesID(tonumber(petID))
  end
