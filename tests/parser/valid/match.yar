main function do
	85 score const i32

	score match
		dup 85 == case
			"exact match" io.write_line call
		end

		dup 50 < case
		dup 70 > case
			"between 50 and 70" io.write_line call
		end

		else
			"no match" io.write_line call
		end
	end

	Color.RED myColor const Color

	myColor match
		dup Color.RED == case
			"red" io.write_line call
		end

		else
		end
	end
end
