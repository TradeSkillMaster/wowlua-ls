local a = {
  test = function()
    return {}
  end
}
a:test().hi, b = "t2", 2
function a    .   test()
  return "hi" -- hi
end
--[==[
  testing
  ]==]
