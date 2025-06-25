                if ( UnitHasVehicleUI(noPetNoTarget) and
                                (noPetNoTarget == noPetNoTarget:gsub("^[mM][oO][uU][sS][eE][oO][vV][eE][rR]", "")
                                                               :gsub("^[aA][rR][eE][nN][aA]%d", ""))
                                -- NOTE: using these 3 gsubs is faster than a :lower() call and a table lookup
                                -- "target" is not included in the above check because it is already filtered out earlier on
                                ) then
                end
