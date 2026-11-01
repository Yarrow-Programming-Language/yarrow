# Stage 31: W406 ambiguous require (function wins over nested module).
# Check succeeds (exit 0) while emitting warning diagnostics.

"std.io" io require

"helpers.ambig.nested" require

main function do
	nested call
	"ok" io.write_line call
end
