# Stage 31: W407 unreachable code after `return`.
# Check succeeds (exit 0) while emitting warning diagnostics.

"std.io" io require

main function do
	"alive" io.write_line call return
	"dead" io.write_line call
end
