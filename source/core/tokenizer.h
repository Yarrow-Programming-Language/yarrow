#ifndef TOKENIZER_H
#define TOKENIZER_H

#define TOKENIZER_VERSION "0.0.1"

typedef enum {
	TOKEN_LEFT_PAREN,          // (
	TOKEN_RIGHT_PAREN,         // )
	TOKEN_LEFT_CURLY,          // {
	TOKEN_RIGHT_CURLY,         // }
	TOKEN_LEFT_SQUARE,         // [
	TOKEN_RIGHT_SQUARE,        // ]

	TOKEN_PLUS,                // +
	TOKEN_MINUS,               // -
	TOKEN_ASTERISK,            // *
	TOKEN_SLASH,               // /
	TOKEN_SLASH_SLASH,         // //
	TOKEN_PERCENT,             // %
	TOKEN_TILDE,               // ~
	TOKEN_CARET,               // ^
	TOKEN_PIPE,                // |
	TOKEN_DOT,                 // .
	TOKEN_QUESTION,            // ?
	TOKEN_BANG,                // !
	TOKEN_COLON,               // :
	TOKEN_LEFT_ARROW,          // <-
	TOKEN_RIGHT_ARROW,         // ->
	TOKEN_EQUAL,               // =
	TOKEN_AMPERSAND,           // &
	TOKEN_UNDERSCORE,          // _
	TOKEN_DOLLAR,               // $
	TOKEN_COMMENT,             // #
	TOKEN_DOC_COMMENT,         // ##

	TOKEN_EQUAL_EQUAL,         // ==
	TOKEN_NOT_EQUAL,           // !=
	TOKEN_GREATER,             // >
	TOKEN_GREATER_EQUAL,       // >=
	TOKEN_LESS,                // <
	TOKEN_LESS_EQUAL,          // <=

	TOKEN_AND,                 // and
	TOKEN_OR,                  // or
	TOKEN_NOT,                 // not
	TOKEN_XOR,                 // xor
	TOKEN_LEFT_SHIFT,          // <<  left_shift
	TOKEN_RIGHT_SHIFT,         // >>  right_shift

	TOKEN_AT,                  // For builtin (e.g @write, @)
	TOKEN_IDENTIFIER,          // For any word not in there

	TOKEN_TYPE,                // For type themselfs (e.g i32, f16, string, rune, even i3 or u5)
	TOKEN_STRING,              // "my string"
	TOKEN_RAW_STRING,          // `my raw string`
	TOKEN_RUNE,                // 'f'
	TOKEN_INTEGER,             // 69 1_200
	TOKEN_BINARY_INTEGER,      // 0b01101
	TOKEN_OCTAL_INTEGER,       // 0o755
	TOKEN_HEX_INTEGER,         // 0xff
	TOKEN_FLOAT,               // 42.67 1_200.006
	TOKEN_SCIENTIFIC_FLOAT,    // 1.5e10

	// Case insensitive word matching
	TOKEN_TRUE,
	TOKEN_FALSE,
	TOKEN_IF,
	TOKEN_ELSE,
	TOKEN_FOR,
	TOKEN_CONTINUE,
	TOKEN_BREAK,
	TOKEN_YIELD,
	TOKEN_MATCH,
	TOKEN_CASE,
	TOKEN_AS,
	TOKEN_UNWRAP,
	TOKEN_TRY,
	TOKEN_HANDLE,
	TOKEN_FUNCTION,
	TOKEN_RETURN,
	TOKEN_TYPEOF,
	TOKEN_SIZEOF,
	TOKEN_CALL,
	TOKEN_DO,
	TOKEN_WITH,
	TOKEN_END,
	TOKEN_CONST,
	TOKEN_STATIC,
	TOKEN_MUTABLE,
	TOKEN_SET,
	TOKEN_UNSAFE,
	TOKEN_PUBLIC,
	TOKEN_PRIVATE,
	TOKEN_COPY,
	TOKEN_ERROR,
	TOKEN_STRUCT,
	TOKEN_IMPLEMENT,
	TOKEN_ENUM,
	TOKEN_UNION,
	TOKEN_REQUIRE,
	TOKEN_DEFER,
	TOKEN_INC,
	TOKEN_DEC,
	TOKEN_POP,
	TOKEN_DROP,
	TOKEN_DUP,
	TOKEN_ROT,
	TOKEN_UNROT,
	TOKEN_SWAP,
	TOKEN_BORROW,
	TOKEN_MUTABLE_BORROW,
	TOKEN_MOVE,
	TOKEN_LOAD,
	TOKEN_STORE,
	TOKEN_VOID,
	TOKEN_NULL,
	TOKEN_EOF
} TokenKind;

typedef struct {
	int line;
	int column;
	int offset;
} TokenLocation;

typedef struct {
	const char *lexeme;
	TokenKind kind;
	TokenLocation location;
	int end_offset;
} Token;

#endif
