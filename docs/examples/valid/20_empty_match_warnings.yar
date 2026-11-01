# Stage 38: W408 empty match arm.
# Check succeeds (exit 0) while emitting warning diagnostics.

"std.io" io require

main function do
	1 match
		dup 1 == case
		end

		else
			"ok" io.write_line call
		end
	end
end
