-- Test: count-down-loop diagnostic for numeric for-loops with wrong step direction

---@diagnostic disable: empty-block

-- Implicit step 1, counting down → diagnostic
for i = 10, 1 do end
-- ^ diag: count-down-loop

-- Explicit wrong step, counting down with positive step → diagnostic
for i = 10, 1, 1 do end
-- ^ diag: count-down-loop

-- Negative step counting up → diagnostic
for i = 1, 10, -1 do end
-- ^ diag: count-down-loop

-- Correct implicit step, counting up → no diagnostic
for i = 1, 10 do end

-- Correct explicit negative step, counting down → no diagnostic
for i = 10, 1, -1 do end

-- Correct explicit positive step, counting up → no diagnostic
for i = 1, 10, 2 do end

-- Non-literal values → no diagnostic (can't check)
local x, y = 10, 1
for i = x, y do end

-- Same start and end → no diagnostic (zero iterations but not wrong direction)
for i = 5, 5 do end

-- Zero step → diagnostic (infinite loop)
for i = 1, 10, 0 do end
-- ^ diag: count-down-loop

-- Zero step counting down → diagnostic
for i = 10, 1, 0 do end
-- ^ diag: count-down-loop

-- Zero step with same start/end → still infinite loop, warn
for i = 5, 5, 0 do end
-- ^ diag: count-down-loop

-- Suppression via @diagnostic
---@diagnostic disable-next-line: count-down-loop
for i = 10, 1 do end
