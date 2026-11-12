# =============================================================================
# Modules
# =============================================================================
# Form: "path" [scope] require
# - With scope: bindings go under that scope (io.write_line).
# - Without scope: bindings enter the current scope.
# - Item path ("std.math.sqrt"): import only that function into the current scope.
# Private top-level entities stay file-local and are not exported.

"std.error" error require     # Whole std.error module into scope named error
"std.io" io require           # Whole std.io module into scope named io

# "std.math.sqrt" require     # Item import: only sqrt into the current scope
# "std.math" require          # Whole module into the current scope

# =============================================================================
# Functions
# =============================================================================
# Visibility defaults to private. Marking `private` here is therefore optional but documents intent.
# Omit `with Type` or explicitly put `with void` to return void.

my_function private function do
	1 2 +       # 1 + 2 = 3
	5 3 -       # 5 - 3 = 2
	4 2 *       # 4 * 2 = 8
	10 4 /      # 10 / 4 = 2.5 (true division)
	10 3 //     # 10 // 3 = 3 (floor division)
	10 3 %      # 10 % 3 = 1
	2 3 ^       # 2 ^ 3 = 8

	# Stack: [3, 2, 8, 2.5, 3, 1, 8]
	drop        # Clear the whole stack

	# -------------------------------------------------------------------------
	# Concatenation
	# -------------------------------------------------------------------------
	# `~` joins strings, raw strings (autoderef through reference<string> or mutable_reference<string>).
	# `+` is arithmetic (and pointer byte-offset) only. It is not overloaded for strings. By the way, there is no overload in this language.

	"hello" " world" ~       # "hello world"

	# Stack: ["hello world"]
	drop

	# -------------------------------------------------------------------------
	# Logical
	# -------------------------------------------------------------------------
	# `and` / `or` / `not` on bools are logical. The same words on integers are bitwise (see below).

	true false and      # false
	true false or       # true
	true not            # false

	# Stack: [false, true, false]
	drop

	# -------------------------------------------------------------------------
	# Comparison
	# -------------------------------------------------------------------------
	# Always push a bool.

	1 2 ==      # false
	1 2 !=      # true
	5 3 >       # true
	3 5 <       # true
	5 3 >=      # true
	3 5 <=      # true

	# Stack: [false, true, true, true, true, true]
	drop

	# -------------------------------------------------------------------------
	# Bitwise
	# -------------------------------------------------------------------------

	1 2 and              # 0
	1 5 or               # 5
	4 5 xor              # 1
	5 2 <<               # 20
	5 2 left_shift       # 20
	5 2 >>               # 1
	5 2 right_shift      # 1
	5 not                # -6 (two's complement style bitwise not)

	# Stack: [0, 5, 1, 20, 20, 1, 1, -6]
	drop

	# -------------------------------------------------------------------------
	# Literals
	# -------------------------------------------------------------------------
	# Underscores are digit separators.

	42             # u8
	-900           # i16
	1_000          # u16
	0b100110       # u8 (38)
	0xAB12         # u16 (43890)
	3.14           # f16
	6_329.5        # f32
	"hello"        # string
	'\n'           # rune (character)
	true           # bool

	# Stack: [42, -900, 1_000, 0b100110, 0xAB12, 3.14, 6_329.5, "hello", '\n', true]
	drop

	# typeof: pop a value, push its static type (usable with == / !=).
	# - Simple / copy values are consumed.
	# - Heap values usually arrive as borrows (variable read or dup). typeof releases that borrow and leaves the data owned by its owner.
	# - For reference<T> or mutable_reference<T>, reports the pointee type T.

	42 typeof

	# Stack: [u8]
	drop

	# -------------------------------------------------------------------------
	# Stack manipulation
	# -------------------------------------------------------------------------

	42 dup         # [42] -> [42, 42]
	5 inc          # [5] -> [6]          Only work on a numerical value.
	5 dec          # [5] -> [4]          Same constraint as above.
	2 1 swap       # [1, 2] -> [2, 1]
	2 3 1 rot      # [1, 2, 3] -> [2, 3, 1]
	3 1 2 unrot    # [1, 2, 3] -> [3, 1, 2]
	56 42 pop      # [56, 42] -> [56]    Remove the top value. If it is a reference, release the borrow.
	drop           # [56] -> []          Clear the whole stack. Release every borrow on it.

	# -------------------------------------------------------------------------
	# Container
	# -------------------------------------------------------------------------
	# Empty literals have no element type until a typed context (e.g. a variable declaration) supplies one.
	# Empty literals need a typed binding (or other typed context) before use.

	() array<i32> as                # array literal
	() hashmap<string i32> as       # hashmap literal
	[] array<i32 0> as              # array literal
	[] hashmap<string i32 0> as     # hashmap literal
	{}                              # struct literal
	drop

	# array<T>: growable and heap allocated.
	(43 54 65) array<i32> as

	# array<T N>: fixed size and inline in its owner. N may be inferred from a non-empty literal.
	[10 20 30] array<i32 3> as
	[10 20 30] array<i32 _> as      # Same as above

	# hashmap<K V N>: fixed size and inline in its owner. Literal keys in [ k v k v ... ].
	["first" 4 "second" 5] hashmap<string i32 2> as
	["first" 4 "second" 5] hashmap<string i32 _> as    # Same as above

	# hashmap<K V>: growable and heap allocated. For dynamic sized hashmap.
	("first" 4 "second" 5) hashmap<string i32> as      # Need a typed context, else will be interpreted as a list.

	# Struct literals use identifier keys: { field value ... }. It will be covered bellow, when we start seeing structs syntax.
	drop

	# Another container is the slice.
	# It's a non-owning view into a contiguous section of an array.
	# A slice can be created on any container via the std.array.slice module.

	# -------------------------------------------------------------------------
	# Variables
	# -------------------------------------------------------------------------
	# Form: <value> <name> (mutable|const|static) <Type>

	# Declaration pops the value (implicit coercion to Type allowed) and binds it.
	# The variable owns non-copy storage.
	# mutable  - Reassignable with `name set` (old value dropped).
	# const    - Set once at runtime, never change afterwards.
	# static   - Compile-time constant, initializer must be known statically.
	# Reading the name pushes a copy (copy types) or a borrow (non-copy types).

	42 myVar mutable i32        # Coerce u8 to i32. myVar owns 42.
	23 myVar set                # Drop old value, now 23.
	100 myConst const i32
	50 myStatic static i32

	myVar       # In this case, push a copy of the i32 value owned by myVar.
	# Stack: [23]

	myVar typeof
	# Stack: [23 i32]
	drop

	# -------------------------------------------------------------------------
	# Nested function and function call
	# -------------------------------------------------------------------------
	# Nested functions are only callable from this enclosing body.
	# Parameters move onto the local stack in declaration order (first declared = deepest).
	# Body bindings such as `x const T` pop from the top, so the last parameter is bound first.
	# The return type of a function is specified at the end with `end with ...`

	# Call form: <args...> <fn> call

	add function
		i32
		i32
	do
		+ return
	end with i32      # This function must return an i32 in any of it's branches.

	3 4 add call      # Here 3 goes first and 4 goes second, so the local stack of the function is [4, 3].

	# -------------------------------------------------------------------------
	# Control flow
	# -------------------------------------------------------------------------
	# `if` need to consume a boolean to branch.
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

	# Match any subject with a condition. Can affect the current stack.
	# Subject is borrowed for the whole match.
	# For each case an operation that leave a bool is needed.
	# First true case runs, otherwise `else` run.

	85 score const i32

	score match
		dup 85 == case    # We need to dup, else the comparision would consume the value and we would be forced to call score again.
			"exact match" io.write_line call
		end

		dup 50 < case
			"under 50" io.write_line call
		end

		dup 60 < case     # For multi case condition.
		dup 70 > case
			"between 60 and 70" io.write_line call
		end

		else
			"did not match everything above" io.write_line call
			drop    # Would drop the local stack of my_function.
		end
	end

	# -------------------------------------------------------------------------
	# Loops
	# -------------------------------------------------------------------------
	# `for` with a bool on top is a while loop.
	# `for` with an iterable walks elements and is a common for loop.

	"std.loop" loop require

	0 counter mutable i32
	counter 5 < for      # While count is less than 5
		# The index will be put on the stack and removed at this loops end.
		counter inc set
	end

	[4 5 6 7 8 9 10] array<i32 _> as for    # Keep in mind that for consume the subject.
		# The current loop value is put on the stack, next to the index.

		# Stack: [0 4] for the first loop.
	end

	# std.loop provides range and other helpers.
	0 10 loop.integer_range for    # The integer_range function essentially create an array with the range numbers.
		# Stack: [0 0]
	end
end

# =============================================================================
# Structs and methods
# =============================================================================
# Default visibility is private for the type and each field. `public` exports.
# Methods are declared in `Type implement` ... `end`.

# Receivers are usually reference<T> or mutable_reference<T>.
# Field and method access autoderefs through reference<T> and mutable_reference<T>.

Point private struct
	i32 x public
	i32 y private
end

Point implement
	distance public function
		reference<Point>
	do
		self const reference<Point>
		self.x self.x * self.y self.y * + return
	end with i32

	setX public function
		mutable_reference<Point>
		i32
	do
		self const mutable_reference<Point>   # Here we use const because we change the pointee value, not the variable value itself.
		newValue const i32

		newValue self.x set                  # Here the changes would be carried past end of scope.
	end
end

struct_function function do
	{x 5 y 20} point mutable Point    # Here we initialize a variable of type Point with a struct literal that match Point structure.
	10 point.x set

	# Methods that take reference<T> need an explicit borrow (or a non-copy read that already yields a borrow).
	point borrow
	point.distance call
	# Stack: [500]

	# It is possible to get the type of a struct field without any need to initialize a new struct.
	Point.x typeof
	# Stack: [500, i32]

	# You can also use struct literal for types.
	# That also mean you can use a struct literal as function return type to force a certain structure.
	{x 6 y 50} otherPoint mutable {i32 x i32 y}    # The syntax is the same, but we specify type and field name instead of field name and value.
end

# =============================================================================
# Enums
# =============================================================================
# Default underlying type is i32. Write `Name <type> enum` for another carrier (e.g. string).
# Members get sequential discriminants from 0 unless given an explicit value (does not apply to type other than number).
# The next implicit member continues after that value.

Color enum
	RED         # 0
	GREEN       # 1
	BLUE 10     # 10
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

MyEnum string enum
	MY_VALUE "some string"     # Not providing a value for a non number type don't compile.
end

# =============================================================================
# Unions
# =============================================================================
# Holds exactly one of the listed member types.
# Init or set accept any member type.

# If not a named union, members must be distinct.
MyUnion union
	i32
	string
end

union_function function do
	42 val mutable MyUnion
	"a string" val set

	val typeof
	# Stack: [MyUnion]
	drop

	# typeof on a union reports the union type, not the active member.
	val match
		i32 case
			val 4 +
		end

		string case
			val io.write_line call
		end

		else     # Here it's optional and will never match since all the union field are matched above.
		end
	end

	# You can define an union literal using `|type type|`.
	# This is specialy used in error handling and with functions returning a value or an error.
	"some text" unionLiteral mutable |i32 string|
end

# =============================================================================
# Defer
# =============================================================================
# Defer runs at scope exit.
# Multiple defers run in reverse registration order.
# Useful for closing files, freeing memory, etc.

defer_function function do
	"std.fs" fs require     # require inside a function only affects that function's scope.

	"myfile.txt" 'r' fs.open_file call unwrap   # Here the function can return an error, so we need to unwrap or handle it. Will be explained bellow.

	file const reference<File>
	# file.read_line call
	defer file fs.close_file call end

	# Inner statements of one defer still run top-to-bottom.
	# Multiple defer blocks run last-registered first.

	defer
		"A" io.write_line call
		"B" io.write_line call
	end
end

# =============================================================================
# Memory
# =============================================================================
# Model: stack ownership, variable ownership, single-borrow references and compile-time checks.

memory_function function do
	"std.array" array require

	# Stack owns temporaries until pop, drop or consumed.
	"temp" pop

	# Variables own values until set, move, or scope exit.
	"hello" myStr mutable string
	"world" myStr set

	# Borrow pushes reference<T> or mutable_reference<T>.
	# An immutable borrow lets you read a value.
	# A mutable borrow lets you read and modify a value.
	# Multiple simultaneous immutable borrows are allowed.
	# However, only one exclusive mutable borrow is allowed.
	# It may move on the stack.
	# Pop (or consuming the reference in a call) releases it.

	(1 2 3) myList mutable array<i32>

	myList borrow
	# Stack: [reference<array<i32>>]
	pop

	# Keep in mind that only a single exclusive mutable borrow can exist.
	# If the above borrow was not popped, creating a mutable_borrow would cause an error.
	myList mutable_borrow             # Using an immutable borrow here would cause the bellow array push to fail and cause an error.
	# Stack: [mutable_reference<array<i32>>]
	4 array.push_last call unwrap     # This function need a dynamic array.

	# move transfers ownership to another variable. Source is then unusable.
	() myList2 mutable array<i32>
	myList myList2 move
	# myList 4 array.push_last call unwrap    # This will cause a use after move error.
	myList2 4 array.push_last call unwrap

	# Cannot drop or pop an owner while a borrow is live.
	# myList2 borrow        # error: release the reference first
	# myList2 pop           # error: release the reference first

	# Copy types (dup and variable read push a real copy):
	# - integers
	# - floats
	# - bool
	# - rune
	# - enum
	# - pointer<T>
	# - reference<T>
	# - array<T N>       (unless T is a non-copy type)
	# - hashmap<K V N>   (unless K or V are non-copy types)

	# Non-copy types (variable read pushes a borrow):
	# - string
	# - array<T>
	# - hashmap<K V>
	# - unions
	# - structs
	# - mutable_reference<T>    (here no duplication allowed at all)
end

# =============================================================================
# Errors
# =============================================================================
# Error declarations behave like enums specialized for failure.
# Fallible functions return a union literal: |Success Err|.
# try: if success, push Success. If failure, propagate if caller can error, otherwise rejected or trap.
# unwrap: if success, push Success. If failure, cause a runtime error.
# handle: on failure run handler then push fallback. On success keep payload.

MyCustomErrors error
	MY_CUSTOM_ERROR
end

# We create two more error sets to show how you can manage multiple error propagation within one function.
MyFirstErrors error
	MY_VERY_IMPORTANT_ERROR
end

MySecondErrors error
	MY_OTHER_IMPORTANT_ERROR
end

error_function function do
	risky_operation function do
		MyCustomErrors.MY_CUSTOM_ERROR return
	end with |i32 MyCustomErrors|

	# risky_operation call unwrap    # This would cause an runtime error.
	# risky_operation call try       # Same as above, but push the error to the top calling function.
	# If the caller could not error, `try` would be a compile error.

	# A function that can return an error need to be handled one way or another.
	# If you want handle the error yourself, use `handle ... end` on the call.
	risky_operation call handle
		# The handle stack implicitly has the error value in it, so you can match on it.
		# Stack: [MyCustomErrors.MY_CUSTOM_ERROR]
		match
			dup error.MY_CUSTOM_ERROR == case
				"Caught Custom Error" io.write_line call
			end

			else
				"Unknown error" io.write_line call
			end
		end

		# You can (but are not forced to) push a fallback value on the parent stack.
		# The fallback value need to be of the same type as the success value of the called function.
		0 return
	end

	# Here a short version with only a fallback value.
	risky_operation call handle 0 return end
	# Stack: [0, 0]

	multi_error_function function do
		first_function function do
			MyFirstErrors.MY_VERY_IMPORTANT_ERROR return
		end with |void MyFirstErrors|

		second_function function do
			MySecondErrors.MY_OTHER_IMPORTANT_ERROR return
		end with |void MySecondErrors|

		# first_function call try     # Would cause a runtime error, but propagate successfully the error.
		# second_function call try    # Same as above.

		# Here we use `_` to let the compiler flatten the two error sets.
		# It would create a new union `|MyFirstErrors MySecondErrors|`.
		# That also mean you can replace the `_` with said union if you want, but the former is a more concise syntax.
	end with |void _|

	# A function's error signature should describe only the errors that can escape that function.
	# Here no need to specify an union with an error type because all errors are handled and nothing is propagated.
end

# =============================================================================
# Unsafe context and pointers
# =============================================================================
# Escapes are visible at definition (`unsafe function`) and at use (`unsafe ... end`).
# Unsafe does not disable borrow or ownership checks.
# pointer<T> and const_pointer<T> are typed raw address at compile time.
# Validity of raw pointers is the programmer's responsibility.
# Treat raw pointers as a way to temporarily step outside the guarantees of the ownership system, not as an alternative to it.

Cell struct
	i32 value public
end

pointer_function private unsafe function do
	"std.mem" mem require

	# Even inside an unsafe function, mark the operations with an unsafe block.
	unsafe
		{} gpa const mem.allocator.GeneralPurposeAllocator   # Here we create an allocator. There are other more allocator available in the standard library.

		i32 100 gpa.allocate call unwrap p mutable pointer<i32>     # This allocate 100 bytes and return a pointer to an i32.
		defer p gpa.free call end     # If you allocate memory, you need to free it at some point. Defer help with that.

		# Typed store or load through pointer<T>
		p 42 store
		p load
		drop

		# Adding a number to a pointer type do a byte offset.
		p 4 + q const pointer<i32>   # This give you a pointer that point 4 bytes after the p pointer.
		p 4 mem.byte_offset          # This is the same as above.
		p 1 mem.pointer_offset       # This will offset the pointer by N times the size of the pointee type, 4 bytes in this case.

		q 99 store
		q load
		drop

		# Field access autoderefs through pointer<Struct>
		Cell 100 gpa.allocate call unwrap cp mutable pointer<Cell>
		defer cp gpa.free call end

		{value 7} cp set
		cp.value
		drop
	end
end

# =============================================================================
# Entry point
# =============================================================================
# Every program needs a main function. It is the only entity public by default. It's name can be changed by compiler flags.
# Call ordinary functions with `name call`. Call unsafe functions only inside unsafe ... end.
# Optional numeric return from main sets the process exit code.
# To get the application arguments, you should see the relevent standard library part.

main function do
	my_function call
	struct_function call
	enum_function call
	union_function call
	defer_function call
	memory_function call
	error_function call

	unsafe
		pointer_function call
	end

	"Hello, Yarrow!" io.write_line call
end
