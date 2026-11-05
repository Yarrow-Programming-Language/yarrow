# Filesystem surface over host `fs_*` helpers. Open / read / write are
# fallible (`|T error.Error|`); close ignores invalid fds (failed-open
# fallbacks). Modes: `'r'` read, `'w'` write+truncate+create, `'a'` append.

"std.error" error require

File public struct
	i64 fd public
end

open_file public function
	string
	rune
do
	mode const rune
	path const string
	path mode @fs_open fd const i64
	fd 0 < if
		0 fd - code const i64
		code 2 == if
			error.NOT_FOUND return
		else
			code 3 == if
				error.INVALID_ARGUMENT return
			else
				error.IO_ERROR return
			end
		end
	else
		{fd fd} f const File
		f return
	end
end with |File error.Error|

close_file public function
	reference<File>
do
	file const reference<File>
	file.fd @fs_close
	pop
end

read_file public function
	reference<File>
do
	file const reference<File>
	file.fd @fs_read s const string
	@fs_last_error code const i64
	code 0 != if
		code 2 == if
			error.NOT_FOUND return
		else
			code 3 == if
				error.INVALID_ARGUMENT return
			else
				error.IO_ERROR return
			end
		end
	else
		s return
	end
end with |string error.Error|

write_file public function
	reference<File>
	string
do
	content const string
	file const reference<File>
	file.fd content @fs_write code const i64
	code 0 == if
		return
	else
		code 2 == if
			error.NOT_FOUND return
		else
			code 3 == if
				error.INVALID_ARGUMENT return
			else
				error.IO_ERROR return
			end
		end
	end
end with |void error.Error|
