# Test maps with various types
"std.io" io require

main function do
  # Map with simple types
  { "a" 1 "b" 2 } m1 static hashmap<string i32>
  
  "Maps work!" io.write_line call
  
  0 return
end
