#ifndef TOKENIZER_H
#define TOKENIZER_H

#include <stddef.h>

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
	TOKEN_UNDERSCORE,          // _
	TOKEN_BANG,                // !
	TOKEN_EQUAL,               // =
	// TOKEN_QUESTION,            // ?
	// TOKEN_COLON,               // :
	// TOKEN_LEFT_ARROW,          // <-
	// TOKEN_RIGHT_ARROW,         // ->
	// TOKEN_AMPERSAND,           // &
	// TOKEN_DOLLAR,              // $
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
	const char *word;
	TokenKind kind;
} TokenizerKeyword;

static const TokenizerKeyword tokenizer_keywords[] = {
	{ "and", TOKEN_AND },
	{ "or", TOKEN_OR },
	{ "not", TOKEN_NOT },
	{ "xor", TOKEN_XOR },
	{ "true", TOKEN_TRUE },
	{ "false", TOKEN_FALSE },
	{ "if", TOKEN_IF },
	{ "else", TOKEN_ELSE },
	{ "for", TOKEN_FOR },
	{ "continue", TOKEN_CONTINUE },
	{ "break", TOKEN_BREAK },
	{ "match", TOKEN_MATCH },
	{ "case", TOKEN_CASE },
	{ "as", TOKEN_AS },
	{ "unwrap", TOKEN_UNWRAP },
	{ "try", TOKEN_TRY },
	{ "handle", TOKEN_HANDLE },
	{ "function", TOKEN_FUNCTION },
	{ "return", TOKEN_RETURN },
	{ "typeof", TOKEN_TYPEOF },
	{ "sizeof", TOKEN_SIZEOF },
	{ "call", TOKEN_CALL },
	{ "do", TOKEN_DO },
	{ "with", TOKEN_WITH },
	{ "end", TOKEN_END },
	{ "const", TOKEN_CONST },
	{ "static", TOKEN_STATIC },
	{ "mutable", TOKEN_MUTABLE },
	{ "set", TOKEN_SET },
	{ "unsafe", TOKEN_UNSAFE },
	{ "public", TOKEN_PUBLIC },
	{ "private", TOKEN_PRIVATE },
	{ "error", TOKEN_ERROR },
	{ "struct", TOKEN_STRUCT },
	{ "implement", TOKEN_IMPLEMENT },
	{ "enum", TOKEN_ENUM },
	{ "union", TOKEN_UNION },
	{ "require", TOKEN_REQUIRE },
	{ "defer", TOKEN_DEFER },
	{ "inc", TOKEN_INC },
	{ "dec", TOKEN_DEC },
	{ "pop", TOKEN_POP },
	{ "drop", TOKEN_DROP },
	{ "dup", TOKEN_DUP },
	{ "rot", TOKEN_ROT },
	{ "unrot", TOKEN_UNROT },
	{ "swap", TOKEN_SWAP },
	{ "borrow", TOKEN_BORROW },
	{ "mutable_borrow", TOKEN_MUTABLE_BORROW },
	{ "move", TOKEN_MOVE },
	{ "load", TOKEN_LOAD },
	{ "store", TOKEN_STORE },
	{ "void", TOKEN_VOID },
	{ "null", TOKEN_NULL }
};

typedef struct {
	int line;
	int column;
} TokenLocation;

typedef struct {
	const char *lexeme;
	size_t length;
	TokenKind kind;
	TokenLocation location;
} Token;

typedef struct {
	const char *start;
	const char *current;
	int line;
	int column;
	int start_line;
	int start_column;
} Tokenizer;

int tokenizer_tokenize(const char *source, Token **tokens, size_t *count);

#endif
