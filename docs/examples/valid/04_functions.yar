# Nested function, parameters, and call.

"std.io" io require

demo function do
	add function
		i32
		i32
	do
		+ return
	end with i32

	3 4 add call
	drop

	"add ok" io.write_line call
end

main function do
	demo call
end
