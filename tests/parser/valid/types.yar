main function do
	42 myVar mutable i32
	100 myConst const i32
	50 myStatic static i32
	"some text" unionLiteral mutable |i32 string|
	{} gpa const mem.allocator.GeneralPurposeAllocator
	i32 100 gpa.allocate call unwrap p mutable pointer<i32>
	q const const_pointer<Cell>
	r const mutable_reference<Point>

	1 2 + drop
end
