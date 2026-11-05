# Grammar

Illustrative Yarrow program. Comments explain language rules; the code is the runnable shape of the language. Formal EBNF lives in [`SYNTAX.md`](SYNTAX.md).

```yarrow
# =============================================================================
# Yarrow at a glance
# =============================================================================
# Stack-based, statically typed, modular. Every value lives on an evaluation
# stack until a declaration, call, or operator consumes it. Words are postfix:
# push operands first, then the operator or keyword.
#
# Related docs: SYNTAX.md (EBNF), AST.md, TYPE_SYSTEM.md, MEMORY_MODEL.md,
# RUNTIME.md, STYLE_GUIDE.md.

# =============================================================================
# Modules
# =============================================================================
# Form: "path" [alias] require
#   - With alias: bindings go under that scope (io.write_line).
#   - Without alias: bindings enter the current scope.
#   - Item path ("std.math.sqrt"): import only that function into the current scope.
# Private top-level entities stay file-local and are not exported.

"std.io" io require           # Whole std.io module → scope named io
"std.error" error require     # Whole std.error module → scope named error
# "std.math.sqrt" require     # Item import: only sqrt into the current scope
# "std.math" require          # Whole module into the current scope

# =============================================================================
# Functions (basics): operators, literals, stack, variables
# =============================================================================
# Visibility defaults to private. Marking `private` here is therefore optional
# but documents intent. Omit `with Type` to return void.

my_function private function do
	# -------------------------------------------------------------------------
	# Arithmetic
	# -------------------------------------------------------------------------
	# Binary ops pop two values (rightmost / top first as the right operand) and
	# push one result. Order on the stack before the op: [left, right].
	1 2 +    # 1 + 2 → 3
	5 3 -    # 5 - 3 → 2
	4 2 *    # 4 * 2 → 8
	10 4 /   # 10 / 4 → 2.5 (true division)
	10 3 //  # 10 // 3 → 3 (floor division)
	10 3 %   # 10 % 3 → 1
	2 3 ^    # 2 ^ 3 → 8
	# Stack: [3, 2, 8, 2.5, 3, 1, 8]
	drop

	# -------------------------------------------------------------------------
	# Concatenation
	# -------------------------------------------------------------------------
	# `~` joins strings (autoderef through reference<string>). `+` is arithmetic
	# (and pointer byte-offset) only; it is not overloaded for strings.
	"hello" " world" ~    # "hello world"
	# Stack: ["hello world"]
	drop

	# -------------------------------------------------------------------------
	# Logical (bool)
	# -------------------------------------------------------------------------
	# `and` / `or` / `not` on bools are logical. The same words on integers are
	# bitwise (see below).
	true false and  # false
	true false or   # true
	true not        # false
	# Stack: [false, true, false]
	drop

	# -------------------------------------------------------------------------
	# Comparison
	# -------------------------------------------------------------------------
	# Always push a bool.
	1 2 ==    # false
	1 2 !=    # true
	5 3 >     # true
	3 5 <     # true
	5 3 >=    # true
	3 5 <=    # true
	# Stack: [false, true, true, true, true, true]
	drop

	# -------------------------------------------------------------------------
	# Bitwise (integers)
	# -------------------------------------------------------------------------
	1 2 and      # 0
	1 5 or       # 5
	4 5 xor      # 1
	5 2 lshift   # 20
	5 2 rshift   # 1
	5 not        # -6 (two's complement style bitwise not)
	# Stack: [0, 5, 1, 20, 1, -6]
	drop

	# -------------------------------------------------------------------------
	# Literals and typeof
	# -------------------------------------------------------------------------
	# Integer literals take the smallest fitting type:
	#   positive → smallest unsigned; negative → smallest signed.
	# Floats take the smallest fitting float. Underscores are digit separators.
	42        # u8
	-900      # i16
	1_000     # u16
	0b100110  # u8 (38)
	0xAB12    # u16 (43890)
	3.14      # f16
	6_329.5   # f32
	"hello"   # string
	'\n'      # rune (character)
	true      # bool
	# Stack: [42, -900, 1_000, 0b100110, 0xAB12, 3.14, 6_329.5, "hello", '\n', true]

	# typeof: pop a value, push its static type (usable with == / !=).
	#   - Simple / copy values are consumed.
	#   - Heap values usually arrive as borrows (variable read or dup); typeof
	#     releases that borrow and leaves the data owned by its owner.
	#   - For reference<T>, reports the pointee type T.
	42 typeof
	# Stack: [..., true, u8]

	# Copy types (dup and variable read push a real copy):
	#   integers, floats, bool, rune, enum, array<T N>, pointer<T>
	#
	# Non-copy types (variable read pushes a borrow; use borrow / move):
	#   string, list<T>, hashmap<K V>, unions, structs

	# -------------------------------------------------------------------------
	# Stack manipulation
	# -------------------------------------------------------------------------
	drop         # Clear the whole stack; release every borrow on it
	42 dup       # [42] → [42, 42]  (copy types only; non-copy → use borrow)
	1 2 swap     # [1, 2] → [2, 1]
	1 2 3 rot    # [1, 2, 3] → [2, 3, 1]
	1 2 3 unrot  # [1, 2, 3] → [3, 1, 2]
	42 pop       # Remove top; if it is a reference, release the borrow

	# -------------------------------------------------------------------------
	# Container literals
	# -------------------------------------------------------------------------
	# () list, [] array, {} empty map/struct placeholder.
	# Empty literals have no element type until a typed context (e.g. a
	# variable declaration) supplies one.
	# Empty literals need a typed binding (or other typed context) before use.
	() emptyList const list<i32>
	[] emptyArr const array<i32 0>
	{} emptyMap const hashmap<string i32>
	drop

	# -------------------------------------------------------------------------
	# Variables
	# -------------------------------------------------------------------------
	# Form: <value> <name> (mutable|const|static) <Type>
	# Declaration pops the value (implicit coercion to Type allowed) and binds
	# it. The variable owns non-copy storage. Names: camelCase (see STYLE_GUIDE).
	#   mutable  - reassign with `name set` (old value dropped)
	#   const    - set once at runtime
	#   static   - compile-time constant; initializer must be known statically
	# Reading the name pushes a copy (copy types) or a borrow (non-copy types).
	42 myVar mutable i32       # coerce u8 → i32; myVar owns 42
	23 myVar set               # drop old value; now 23
	100 myConst const i32
	50 myStatic static i32
	myVar
	# Stack: [23]
	myVar typeof
	# Stack: [23, i32]
	drop

	# -------------------------------------------------------------------------
	# Nested function + call
	# -------------------------------------------------------------------------
	# Nested functions are only callable from this enclosing body.
	# Parameters move onto the local stack in declaration order
	# (first declared = deepest). Body bindings such as `x const T` pop from
	# the top, so the last parameter is bound first.
	# Call form: <args...> <fn> call
	add function
		i32          # first param: moved in; implicit coercion allowed
		i32          # second param (use `T copy` only for heap deep-copy)
	do
		# Stack on entry: [<i32>, <i32>]  (deep → shallow)
		+
		# Stack: [<i32>]
		return       # return top; drop any leftovers
	end with i32

	3 4
	# Stack: [3, 4]
	add call
	# Stack: [7]  (both arguments consumed; sum pushed)
	drop

	# -------------------------------------------------------------------------
	# Control flow: if / else
	# -------------------------------------------------------------------------
	# Condition must already be a bool on the stack. No else-if; use match for
	# multi-way branches. Then/else must leave compatible stacks at join.
	5 10 < if
		"less" io.write_line call
	else
		"not less" io.write_line call
	end

	# Type values from typeof compare like any other values.
	myVar typeof i32 == if
		"is i32" io.write_line call
	else
		"is not i32" io.write_line call
	end

	# -------------------------------------------------------------------------
	# Control flow: match (value)
	# -------------------------------------------------------------------------
	# Subject is borrowed for the whole match; the prior stack is restored at
	# end. Each case: words that leave a bool, then `case` ... `end`.
	# First true case runs; otherwise `else`.
	85 score const i32
	score match
		# Stack during match: [85] (borrowed subject)
		dup 85 == case
			"exact match" io.write_line call
		end

		dup 50 < case
			"under 50" io.write_line call
		end

		else
			"did not match everything above" io.write_line call
		end
	end

	# -------------------------------------------------------------------------
	# Loops
	# -------------------------------------------------------------------------
	# for with a bool on top ≈ while. for with an iterable walks elements.
	# std.loop provides break / continue / value / index helpers.
	"std.loop" loop require

	0 counter mutable i32
	counter 5 < for
		counter 1 + counter set
		loop.break
		# loop.continue
	end

	# -------------------------------------------------------------------------
	# Typed containers
	# -------------------------------------------------------------------------
	# array<T N>: fixed size; N may be inferred from a non-empty literal.
	# list<T>: growable. hashmap<K V>: literal keys in { k v ... }.
	# Struct literals use identifier keys: { field value ... }.
	[10 20 30] numbers static array<i32 3>
	0 sum mutable i32

	numbers for
		sum loop.value + sum set
		# loop.index
	end

	(43 54 65) myList static list<i32>

	{"first" 4 "second" 5} myHashmap static hashmap<string i32>
end

# =============================================================================
# Structs and methods
# =============================================================================
# Default visibility is private for the type and each field; `public` exports.
# Methods are declared in `Type implement` ... `end`. Receivers are usually
# reference<T> (add `mutable` when the method must mutate the pointee).
# Field and method access autoderefs through reference<T>.

Point private struct
	i32 x public
	i32 y private
end

Point implement
	distance public function
		reference<Point>
	do
		self const reference<Point>
		self.x self.x * self.y self.y * +
		return
	end with i32
end

struct_function function do
	{x 5 y 20} point mutable Point
	10 point.x set
	# Methods that take reference<T> need an explicit borrow (or a non-copy
	# read that already yields a borrow).
	point borrow
	point.distance call
	# Stack: [500]
end

# =============================================================================
# Enums
# =============================================================================
# Default underlying type is i32. Write `Name <type> enum` for another carrier
# (e.g. string). Members get sequential discriminants from 0 unless given an
# explicit value; the next implicit member continues after that value.

Color enum
	RED      # 0
	GREEN    # 1
	BLUE     # 2
	# PURPLE 32      # next would be 33
	# YELLOW 0b101101
end

enum_function function do
	Color.RED myColor const Color
	myColor match
		dup Color.RED == case
			"the color is red" io.write_line call
		end

		dup Color.GREEN == case
			"the color is green" io.write_line call
		end

		else
			"the color is not matched" io.write_line call
		end
	end
end

# =============================================================================
# Unions
# =============================================================================
# Holds exactly one of the listed member types. Members must be distinct.
# Init / set accept any member type. typeof on a union reports the union type,
# not the active member.
#
# Union match: `Type case` (not a bool). The arm receives reference<Member>,
# which autoderefs on read. Borrow ends when the match ends; the union is
# unchanged. `else` is optional when every member type has a case.

MyUnion union
	i32
	string
end

union_function function do
	42 val mutable MyUnion
	"Myself" val set

	val typeof
	# Stack: [MyUnion]
	drop

	val match
		i32 case
			# Stack: [reference<i32>]
			dup *
			# Stack: [1764]
			# `pop` (not `drop`): keep the match subject stack intact.
			pop
		end

		string case
			# Stack: [reference<string>]
			greeting const reference<string>
			greeting " says hello!" ~
			# Stack: ["Myself says hello!"]
			pop
		end

		else
		end
	end
end

# =============================================================================
# Defer
# =============================================================================
# defer ... end runs at scope exit. Multiple defers run in reverse registration
# order. Useful for closing files, freeing regions, etc.
# require inside a function only affects that function's scope.

defer_function function do
	"std.fs" fs require

	"myfile.txt" 'r' fs.open_file call handle
		match
			else
				"open failed" io.write_line call
			end
		end
		{fd -1} failed const File
		failed fallback
	end
	file const reference<File>
	# file.read_line call
	defer
		file fs.close_file call
	end

	# Inner statements of one defer still run top-to-bottom; multiple defer
	# blocks run last-registered first.
	defer
		"A" io.write_line call
		"B" io.write_line call
	end
end

# =============================================================================
# Memory: ownership, borrow, move, regions
# =============================================================================
# Safe model: stack ownership, variable ownership, single-borrow references,
# optional regions, and compile-time checks. No lifetime parameters on types.
# Details: MEMORY_MODEL.md

memory_function function do
	"std.list" list require
	"std.region" region require

	# Stack owns temporaries until pop / drop / consume.
	"temp"
	pop

	# Variables own values until set, move, or scope exit.
	"hello" myStr mutable string
	"world" myStr set
	# myStr dropped at scope exit

	# borrow pushes reference<T>. Only one active borrow per value; it may move
	# on the stack. pop (or consuming the reference in a call) releases it.
	(1 2 3) myList mutable list<i32>
	myList borrow
	pop
	myList 4 list.push_last call unwrap

	# move transfers ownership to another variable; source is then unusable.
	() myList2 mutable list<i32>
	myList myList2 move
	# myList 4 list.push_last call          # error: use after move
	myList2 4 list.push_last call unwrap

	# Cannot drop / pop an owner while a borrow is live:
	# myList2 borrow
	# myList2 pop                      # error: release the reference first

	# Regions: attach heap values, free them as a unit (often via defer).
	region.create call myRegion const i64
	(1 2 3) myListRegion mutable list<i32>
	myListRegion myRegion region.put call
	drop
	myRegion region.free call
end

# =============================================================================
# Unsafe and pointer<T>
# =============================================================================
# Safe by default. Escapes are visible at definition (`unsafe function`) and
# at use (`unsafe ... end`). Unsafe does not disable borrow or ownership checks.
# pointer<T> is a typed raw address at compile time; at runtime it is an address.
# Validity of raw pointers is the programmer's responsibility.

Cell struct
	i32 value public
end

pointer_function private unsafe function do
	"std.mem" mem require

	# Even inside an unsafe function, mark the ops with an unsafe block.
	unsafe
		# mem.allocate n → raw address (integer); coerce into pointer<T> by
		# storing into a typed variable.
		16 mem.allocate call p mutable pointer<i32>

		# Typed store / load through pointer<T>
		p 42 store
		p load
		drop

		# pointer + int: byte offset; result stays pointer<T>
		p 4 + q const pointer<i32>
		q 99 store
		q load
		drop

		# mem.store / mem.load: untyped 64-bit words (no pointee check)
		p 123 mem.store call
		p mem.load call
		drop

		# Field access autoderefs through pointer<Struct>
		32 mem.allocate call cp mutable pointer<Cell>
		7 cp.value set
		cp.value
		drop

		cp mem.free call
		p mem.free call
	end
end

# =============================================================================
# Errors: unwrap and handle
# =============================================================================
# error declarations behave like enums specialized for failure. Optional
# qualified name injects members from another error type:
#   MyCustomErrors error.Error error ... end
#
# Fallible functions return a union literal: |Success Err|.
# unwrap: success → push Success; failure → propagate if caller can error,
# otherwise rejected / trap.
# handle: on failure run handler then push fallback; on success keep payload.

MyCustomErrors error
	MY_CUSTOM_ERROR
end

error_function function do
	risky_operation function do
		5 6 +
		MyCustomErrors.MY_CUSTOM_ERROR return
	end with |i32 MyCustomErrors|

	# risky_operation call unwrap
	#   success → i32 on stack
	#   failure → propagate MyCustomErrors (this function's with allows it)
	#   if the caller could not error, unwrap would be a compile error

	risky_operation call handle
		# On error, match discriminates error members (similar to union match).
		match
			error.MY_CUSTOM_ERROR case
				"Caught Custom Error" io.write_line call
			end

			else
				"Unknown error" io.write_line call
			end
		end

		0 fallback
	end

	# Short form: no handler body, only a fallback value.
	risky_operation call handle 0 fallback end
end with |void MyCustomErrors|

# =============================================================================
# Example: structs, regions, methods, errors together
# =============================================================================

Person struct
	string name public
	list<i32> scores public
end

Person implement
	add_score public function
		reference<Person> mutable
		i32
	do
		"std.list" list require

		# Stack: [reference<Person>, <i32>]
		score const i32
		self const reference<Person>

		self.scores score list.push_last call unwrap
		return
	end with |void error.Error|

	greet public function
		reference<Person>
	do
		self const reference<Person>

		self.name " says hello!" ~
		return
	end with |string error.Error|
end

example_function function do
	"std.region" region require
	"std.loop" loop require

	region.create call myRegion const i64
	defer myRegion region.free call end

	# Identifier keys → struct literal (not a hashmap).
	{name "Alice" scores (10 20)} person mutable Person
	person myRegion region.put call
	drop

	person borrow
	person.greet call handle
		match
			else
				"greet failed" io.write_line call
			end
		end
		"?" fallback
	end
	io.write_line call

	person borrow
	30 person.add_score call handle
		match
			error.OUT_OF_MEMORY case
				"No memory" io.write_line call
			end

			else
				"Unknown error" io.write_line call
			end
		end
	end

	[12 27 36] for
		loop.index 30 < if
			"Younger" io.write_line call
		end
	end
	# defer runs: region.free drops person with the region
end

# =============================================================================
# Entry point
# =============================================================================
# Every program needs main. It is the only entity public by default.
# Call ordinary functions with `name call`. Call unsafe functions only inside
# unsafe ... end. Optional numeric return from main sets the process exit code.

main function do
	my_function call
	struct_function call
	enum_function call
	union_function call
	defer_function call
	memory_function call
	error_function call
	example_function call

	unsafe
		pointer_function call
	end

	"Hello, Yarrow!" io.write_line call
end
```
