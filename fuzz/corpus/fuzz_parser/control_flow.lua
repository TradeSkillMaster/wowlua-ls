local x = 10
if x > 5 then
    x = x - 1
elseif x == 0 then
    x = 100
else
    x = 0
end

while x > 0 do
    x = x - 1
end

repeat
    x = x + 1
until x >= 5

for i = 1, 10, 2 do
    x = x + i
end
