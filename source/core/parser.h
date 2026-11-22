#ifndef PARSER_H
#define PARSER_H

#include "tokenizer.h"

#define PARSER_VERSION "0.0.1"

typedef struct ast_node AstNode;

typedef struct {
	AstNode **items;
	size_t count;
	size_t capacity;
} AstBody;

typedef enum {
	// Declarations
	AST_DECL_FUNCTION,
	AST_DECL_STRUCT,
	AST_DECL_ENUM,
	AST_DECL_UNION,
	AST_DECL_ERROR,
	AST_DECL_IMPLEMENT,
	AST_DECL_REQUIRE,
	// Body words
	AST_LITERAL,            // number, string, run, true, false, null
	AST_WORD,               // identifiers, operators, stack ops, @builtins
	AST_CALL,
	AST_IF,
	AST_MATCH,
	AST_FOR,
	AST_DEFER,
	AST_UNSAFE,
	AST_HANDLE,
	AST_VARIABLE,
	AST_SET,
	AST_RETURN,
	AST_TYPEOF,
	AST_SIZEOF,
} AstKind;

struct ast_node {
	AstKind kind;
	TokenLocation location;

	union {
		struct { AstBody body; } block;
		struct { AstBody then_body; AstBody else_body; } if_else;
		struct { AstBody body; } loop;
		struct { Token name; Token mutability; Token type; } variable;
	} as;
};

typedef struct {
	Token *tokens;
	size_t count;
	size_t current;
} Parser;

int parser_parse(Token **tokens, size_t *count);

#endif
