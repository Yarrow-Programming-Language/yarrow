# mutable, const and static bindings, coercion on declare, and typeof.

"std.io" io require

main function do
	42 answer mutable i32
	100 answer set

	7 limit const i32
	3 piApprox static i32

	answer typeof i32 == if
		"answer is i32" io.write_line call
	else
		"unexpected type" io.write_line call
	end

	drop
	limit pop
	piApprox pop
end
