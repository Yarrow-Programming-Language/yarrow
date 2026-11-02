# Test empty placeholders {}
"std.io" io require

Point struct
	i32 x
	i32 y
end

main function do
  # Empty struct placeholder with typed binding
  {} p1 mutable Point
  
  # Empty map placeholder with typed binding
  {} m1 mutable hashmap<string i32>
  
  # Empty list placeholder with typed binding
  () l1 mutable list<i32>
  
  "Empty placeholders work!" io.write_line call
  
  0 return
end
