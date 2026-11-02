# Test iteration over list<T>
"std.io" io require
"std.list" list require
"std.loop" loop require

main function do
  # Create a list of integers
  () my_list mutable list<i32>
  my_list 10 list.push_last call unwrap
  my_list 20 list.push_last call unwrap
  my_list 30 list.push_last call unwrap
  
  # Iterate over the list
  my_list for
    loop.value io.write_int call
    " " io.write call
  end
  "" io.write_line call
  
  # Create a list of strings
  () str_list mutable list<string>
  str_list "hello" list.push_last call unwrap
  str_list "world" list.push_last call unwrap
  
  str_list for
    loop.value io.write call
    " " io.write call
  end
  "" io.write_line call
  
  0 return
end
