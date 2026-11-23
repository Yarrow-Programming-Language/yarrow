"std.io" io require

Point struct
	i32 x public
	i32 y
end

Color enum
	RED
	GREEN 10
	BLUE
end

MyEnum string enum
	MY_VALUE "some string"
end

Shape union
	i32
	string
end

MyErrors error
	NOT_FOUND
	OUT_OF_RANGE
end

Point implement
	distance public function
		reference<Point>
	do
		self const reference<Point>
		self.x self.x * self.y self.y * + return
	end with i32
end
