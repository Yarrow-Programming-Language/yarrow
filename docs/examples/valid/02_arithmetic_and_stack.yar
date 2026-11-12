# Literals, arithmetic, comparisons, and stack words.
# Demonstrates postfix evaluation and drop, dup and swap.

"std.io" io require

main function do
	10 3 +
	10 3 -
	10 3 *
	10 4 /
	10 3 //
	10 3 %
	2 3 ^
	drop

	"hello" " world" ~
	drop

	true false and
	true false or
	true not
	drop

	1 2 ==
	5 3 >
	drop

	1 2 and
	5 2 <<
	5 2 left_shift
	drop

	42 dup
	2 1 swap
	2 3 1 rot
	5 inc
	drop

	"stack ok" io.write_line call
end
