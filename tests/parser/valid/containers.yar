main function do
	() array<i32> as
	() hashmap<string i32> as
	[] array<i32 0> as
	[] hashmap<string i32 0> as
	{} drop

	(43 54 65) array<i32> as
	[10 20 30] array<i32 3> as
	[10 20 30] array<i32 _> as
	["first" 4 "second" 5] hashmap<string i32 2> as
	("first" 4 "second" 5) hashmap<string i32> as

	{x 5 y 20} point mutable Point
	{x 6 y 50} otherPoint mutable {i32 x i32 y}
end
