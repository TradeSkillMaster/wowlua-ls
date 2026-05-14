local t = {
    x = 1,
    y = 2,
    [3] = "three",
}
for k, v in pairs(t) do
    print(k, v)
end
