# Stage 20 warnings: unused binding, unused require, dead stack.
# Check succeeds (exit 0) while emitting warning diagnostics.

"std.io" io require
"std.math" math require

main function do
	42 unused const i32
	"Hello, Yarrow!" io.write_line call
	99
end
