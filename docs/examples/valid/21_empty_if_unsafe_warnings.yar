# Stage 38: W409 empty if then branch and W410 empty unsafe block.
# Check succeeds (exit 0) while emitting warning diagnostics.

"std.io" io require

main function do
	true if
	else
		"ok" io.write_line call
	end

	unsafe
	end
end
