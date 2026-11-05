# Baseline error type. Members are program-unique tags; inject them into
# custom error types with `MyErr error.Error error … end` after
# `"std.error" error require`.
Error public error
	OUT_OF_MEMORY
	IO_ERROR
	INVALID_ARGUMENT
	NOT_FOUND
end
