# Test arrays in lists
"std.io" io require
"std.list" list require

main function do
  # Create arrays
  [1 2 3] arr1 static array<i32 3>
  [4 5 6] arr2 static array<i32 3>
  
  "Arrays work!" io.write_line call
  
  0 return
end
