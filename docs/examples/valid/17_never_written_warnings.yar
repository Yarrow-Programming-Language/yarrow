# Stage 31: W404 never-written mutable (scalar) and W405 redundant `copy`.
# Check succeeds (exit 0) while emitting warning diagnostics.

"std.io" io require

demo function
	i32
	i32 copy
do
	+ return
end with i32

main function do
	7 x mutable i32
	x
	drop
	3 4 demo call
	drop
	"ok" io.write_line call
end
