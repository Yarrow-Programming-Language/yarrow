#ifndef PARSER_H
#define PARSER_H

#include <stdbool.h>
#include <stddef.h>

#include "tokenizer.h"

#define PARSER_VERSION "0.0.1"

typedef struct ast_node AstNode;
typedef struct ast_type AstType;

typedef struct {
	AstNode **items;
	size_t count;
	size_t capacity;
} AstBody;

typedef struct {
	const Token *first;
	size_t count;
} AstSpan;

typedef AstSpan AstName;

typedef enum {
	AST_TYPE_BASE,
	AST_TYPE_GENERIC,
	AST_TYPE_UNION,
	AST_TYPE_STRUCT
} AstTypeKind;

typedef enum {
	AST_TYPE_MEMBER_TYPE,
	AST_TYPE_MEMBER_SIZE,
	AST_TYPE_MEMBER_PLACEHOLDER
} AstTypeMemberKind;

typedef struct {
	AstTypeMemberKind kind;
	TokenLocation start;
	TokenLocation end;
	union {
		AstType *type;
		Token size;
	} as;
} AstTypeMember;

typedef struct {
	AstType *type;
	AstName name;
} AstTypeField;

typedef struct {
	AstTypeMember *items;
	size_t count;
	size_t capacity;
} AstTypeMembers;

typedef struct {
	AstTypeField *items;
	size_t count;
	size_t capacity;
} AstTypeFields;

struct ast_type {
	AstTypeKind kind;
	TokenLocation start;
	TokenLocation end;
	union {
		struct { AstName name; } base;
		struct { AstType *base; AstTypeMembers arguments; } generic;
		struct { AstTypeMembers members; } union_type;
		struct { AstTypeFields fields; } struct_type;
	} as;
};

typedef enum {
	// Declarations
	AST_DECL_REQUIRE,
	AST_DECL_FUNCTION,
	AST_DECL_STRUCT,
	AST_DECL_ENUM,
	AST_DECL_UNION,
	AST_DECL_ERROR,
	AST_DECL_IMPLEMENT,
	// Declaration records
	AST_PARAM,
	AST_FIELD,
	AST_ENUM_MEMBER,
	AST_UNION_MEMBER,
	// Body words
	AST_LITERAL,
	AST_ARRAY_LITERAL,
	AST_FIXED_ARRAY_LITERAL,
	AST_STRUCT_LITERAL,
	AST_WORD,
	AST_VARIABLE,
	AST_SET,
	AST_CALL,
	AST_AS,
	AST_IF,
	AST_MATCH,
	AST_CASE,
	AST_FOR,
	AST_DEFER,
	AST_UNSAFE,
	AST_HANDLE
} AstKind;

struct ast_node {
	AstKind kind;
	TokenLocation start;
	TokenLocation end;
	union {
		struct { Token path; AstName scope; bool has_scope; } require;
		struct { AstName name; bool is_public; bool is_unsafe; AstBody params; AstBody body; AstType *result; bool has_result; } function;
		struct { AstName name; bool is_public; AstBody fields; } struct_decl;
		struct { AstName name; bool is_public; AstType *carrier; bool has_carrier; AstBody members; } enum_decl;
		struct { AstName name; bool is_public; AstBody members; } union_decl;
		struct { AstName name; bool is_public; AstBody members; } error_decl;
		struct { AstName name; AstBody methods; } implement;
		struct { AstType *type; } param;
		struct { AstType *type; AstName name; bool is_public; } field;
		struct { AstName name; bool has_value; Token value; } enum_member;
		struct { AstType *type; } union_member;
		struct { Token token; } literal;
		struct { AstBody elements; } container;
		struct { AstName name; } word;
		struct { AstName name; Token mutability; AstType *type; } variable;
		struct { AstName name; } set;
		struct { AstName name; } call;
		struct { AstType *type; } as_cast;
		struct { AstBody then_body; AstBody else_body; bool has_else; } if_else;
		struct { AstBody cases; AstBody else_body; bool has_else; } match;
		struct { AstBody condition; AstBody body; } match_case;
		struct { AstBody body; } block;
		struct { AstName name; AstBody body; } handle;
	} as;
};

typedef enum {
	PARSER_TYPE_ABSENT,
	PARSER_TYPE_VALID,
	PARSER_TYPE_INVALID
} ParserTypeStatus;

typedef struct {
	const Token *tokens;
	size_t count;
	size_t current;
	const char *file_name;
} Parser;

void parser_free(AstBody *program);
void parser_print(const AstBody *program);

int parser_parse(const Token *tokens, size_t count, const char *file_name, AstBody *program);

#endif
