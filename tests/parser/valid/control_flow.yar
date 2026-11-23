main function do
	5 10 < if
		1
	else
		2
	end

	0 counter mutable i32
	counter 5 < for
		counter inc set
	end

	defer
		"bye" drop
	end

	unsafe
		1
	end

	risky_operation call handle
		match
			dup MY_ERROR == case
				"caught" drop
			end

			else
				"unknown" drop
			end
		end
		0 return
	end
end
