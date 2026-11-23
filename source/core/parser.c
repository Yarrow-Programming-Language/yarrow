#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>

#include "parser.h"

static void parser_free_node(AstNode *node);
static void parser_free_body(AstBody *body);
static int parser_parse_body(Parser *parser, AstBody *body, const TokenKind *stops, size_t stop_count);
static int parser_parse_word(Parser *parser, AstBody *body);
static int parser_parse_declaration(Parser *parser, AstBody *program);
static void parser_print_node(const AstNode *node, int depth);

static void
parser_error(const Token *token, const char *format, ...)
{
	va_list args;

	fprintf(stderr, "ERROR: ");

	va_start(args, format);
	vfprintf(stderr, format, args);
	va_end(args);

	fprintf(stderr, " at %d:%d\n", token->location.line, token->location.column);
}

static const Token *
parser_peek(const Parser *parser)
{
	return &parser->tokens[parser->current];
}

static const Token *
parser_peek_at(const Parser *parser, size_t offset)
{
	size_t index = parser->current + offset;

	if (index >= parser->count)
	{
		index = parser->count - 1;
	}

	return &parser->tokens[index];
}

static void
parser_advance(Parser *parser)
{
	if (parser->current < parser->count)
	{
		parser->current++;
	}
}

static bool
parser_check(const Parser *parser, TokenKind kind)
{
	return parser_peek(parser)->kind == kind;
}

static bool
parser_match(Parser *parser, TokenKind kind)
{
	if (parser_check(parser, kind))
	{
		parser_advance(parser);
		return true;
	}

	return false;
}

static const Token *
parser_expect(Parser *parser, TokenKind kind, const char *what)
{
	const Token *token = parser_peek(parser);

	if (token->kind != kind)
	{
		parser_error(token, "expected %s but found `%.*s`", what, (int) token->length, token->lexeme);
		return NULL;
	}

	parser_advance(parser);
	return token;
}

static bool
parser_is_type_start(TokenKind kind)
{
	return kind == TOKEN_TYPE || kind == TOKEN_IDENTIFIER;
}

static bool
parser_is_size_literal(TokenKind kind)
{
	switch (kind)
	{
		case TOKEN_INTEGER:
		case TOKEN_BINARY_INTEGER:
		case TOKEN_OCTAL_INTEGER:
		case TOKEN_HEX_INTEGER:
			return true;

		default:
			return false;
	}
}

static bool
parser_is_literal(TokenKind kind)
{
	switch (kind)
	{
		case TOKEN_INTEGER:
		case TOKEN_BINARY_INTEGER:
		case TOKEN_OCTAL_INTEGER:
		case TOKEN_HEX_INTEGER:
		case TOKEN_FLOAT:
		case TOKEN_SCIENTIFIC_FLOAT:
		case TOKEN_STRING:
		case TOKEN_RAW_STRING:
		case TOKEN_RUNE:
		case TOKEN_TRUE:
		case TOKEN_FALSE:
		case TOKEN_NULL:
			return true;

		default:
			return false;
	}
}

static bool
parser_is_value_literal(TokenKind kind)
{
	switch (kind)
	{
		case TOKEN_INTEGER:
		case TOKEN_BINARY_INTEGER:
		case TOKEN_OCTAL_INTEGER:
		case TOKEN_HEX_INTEGER:
		case TOKEN_FLOAT:
		case TOKEN_SCIENTIFIC_FLOAT:
		case TOKEN_STRING:
		case TOKEN_RAW_STRING:
			return true;

		default:
			return false;
	}
}

static AstNode *
parser_new_node(const Token *token, AstKind kind)
{
	AstNode *node = calloc(1, sizeof(AstNode));

	if (node == NULL)
	{
		parser_error(token, "failed to allocate memory for the AST node");
		return NULL;
	}

	node->kind = kind;
	node->location = token->location;
	return node;
}

static int
parser_ast_body_push(AstBody *body, AstNode *node)
{
	if (body->count >= body->capacity)
	{
		size_t new_capacity = body->capacity == 0 ? 8 : body->capacity * 2;
		AstNode **grow = realloc(body->items, new_capacity * sizeof(AstNode *));

		if (grow == NULL)
		{
			return EXIT_FAILURE;
		}

		body->items = grow;
		body->capacity = new_capacity;
	}

	body->items[body->count++] = node;
	return EXIT_SUCCESS;
}

static int
parser_ast_body_append(AstBody *destination, AstBody *source)
{
	for (size_t i = 0; i < source->count; i++)
	{
		if (parser_ast_body_push(destination, source->items[i]) != EXIT_SUCCESS)
		{
			for (size_t j = i; j < source->count; j++)
			{
				parser_free_node(source->items[j]);
			}

			free(source->items);
			source->items = NULL;
			source->count = 0;
			source->capacity = 0;
			return EXIT_FAILURE;
		}
	}

	free(source->items);
	source->items = NULL;
	source->count = 0;
	source->capacity = 0;
	return EXIT_SUCCESS;
}

static const Token *
parser_token_at(const Token *tokens, size_t count, size_t index)
{
	return index < count ? &tokens[index] : &tokens[count - 1];
}

static bool
parser_validate_type_at(const Token *tokens, size_t count, size_t *cursor)
{
	if (*cursor >= count)
	{
		return false;
	}

	const Token *token = &tokens[*cursor];

	switch (token->kind)
	{
		case TOKEN_PIPE:
		{
			(*cursor)++;

			size_t members = 0;

			while (*cursor < count && tokens[*cursor].kind != TOKEN_PIPE)
			{
				if (tokens[*cursor].kind == TOKEN_UNDERSCORE)
				{
					(*cursor)++;
				}
				else if (!parser_validate_type_at(tokens, count, cursor))
				{
					return false;
				}

				members++;
			}

			if (members == 0)
			{
				parser_error(token, "a union type cannot be empty");
				return false;
			}

			if (*cursor >= count)
			{
				parser_error(&tokens[count - 1], "expected `|` to close the union type");
				return false;
			}

			(*cursor)++;
			return true;
		}

		case TOKEN_LEFT_CURLY:
		{
			(*cursor)++;

			while (*cursor < count && tokens[*cursor].kind != TOKEN_RIGHT_CURLY)
			{
				if (!parser_validate_type_at(tokens, count, cursor))
				{
					return false;
				}

				if (*cursor >= count || tokens[*cursor].kind != TOKEN_IDENTIFIER)
				{
					parser_error(parser_token_at(tokens, count, *cursor), "expected a field name after the field type");
					return false;
				}

				(*cursor)++;
			}

			if (*cursor >= count)
			{
				parser_error(&tokens[count - 1], "expected `}` to close the struct type");
				return false;
			}

			(*cursor)++;
			return true;
		}

		case TOKEN_TYPE:
		case TOKEN_IDENTIFIER:
		{
			(*cursor)++;

			while (*cursor + 1 < count && tokens[*cursor].kind == TOKEN_DOT && tokens[*cursor + 1].kind == TOKEN_IDENTIFIER)
			{
				*cursor += 2;
			}

			if (*cursor >= count || tokens[*cursor].kind != TOKEN_LESS)
			{
				return true;
			}

			(*cursor)++;

			size_t arguments = 0;

			while (*cursor < count && tokens[*cursor].kind != TOKEN_GREATER)
			{
				if (tokens[*cursor].kind == TOKEN_UNDERSCORE || parser_is_size_literal(tokens[*cursor].kind))
				{
					(*cursor)++;
				}
				else if (!parser_validate_type_at(tokens, count, cursor))
				{
					return false;
				}

				arguments++;
			}

			if (arguments == 0)
			{
				parser_error(parser_token_at(tokens, count, *cursor), "a generic type needs at least one argument");
				return false;
			}

			if (*cursor >= count)
			{
				parser_error(&tokens[count - 1], "expected `>` to close the generic type");
				return false;
			}

			(*cursor)++;
			return true;
		}

		default:
			parser_error(token, "expected a type but found `%.*s`", (int) token->length, token->lexeme);
			return false;
	}
}

static bool
parser_validate_type(const Token *tokens, size_t count)
{
	size_t cursor = 0;

	if (!parser_validate_type_at(tokens, count, &cursor))
	{
		return false;
	}

	if (cursor != count)
	{
		parser_error(&tokens[cursor], "unexpected `%.*s` after the type", (int) tokens[cursor].length, tokens[cursor].lexeme);
		return false;
	}

	return true;
}

static bool
parser_scan_generic_arguments(Parser *parser)
{
	size_t depth = 1;

	parser_advance(parser);

	while (depth > 0)
	{
		TokenKind kind = parser_peek(parser)->kind;

		if (kind == TOKEN_EOF)
		{
			return false;
		}

		if (kind == TOKEN_LESS)
		{
			depth++;
		}
		else if (kind == TOKEN_GREATER)
		{
			depth--;
		}

		parser_advance(parser);
	}

	return true;
}

static bool
parser_scan_struct_type(Parser *parser)
{
	size_t depth = 1;

	parser_advance(parser);

	while (depth > 0)
	{
		TokenKind kind = parser_peek(parser)->kind;

		if (kind == TOKEN_EOF)
		{
			return false;
		}

		if (kind == TOKEN_LEFT_CURLY)
		{
			depth++;
		}
		else if (kind == TOKEN_RIGHT_CURLY)
		{
			depth--;
		}

		parser_advance(parser);
	}

	return true;
}

static bool
parser_scan_union_member(Parser *parser)
{
	TokenKind kind = parser_peek(parser)->kind;

	if (kind == TOKEN_UNDERSCORE)
	{
		parser_advance(parser);
		return true;
	}

	if (kind == TOKEN_LEFT_CURLY)
	{
		return parser_scan_struct_type(parser);
	}

	if (!parser_is_type_start(kind))
	{
		return false;
	}

	parser_advance(parser);

	while (parser_check(parser, TOKEN_DOT) && parser_peek_at(parser, 1)->kind == TOKEN_IDENTIFIER)
	{
		parser_advance(parser);
		parser_advance(parser);
	}

	if (parser_check(parser, TOKEN_LESS))
	{
		return parser_scan_generic_arguments(parser);
	}

	return true;
}

static ParserTypeStatus
parser_parse_type(Parser *parser, AstType *type)
{
	const Token *first = parser_peek(parser);
	size_t start = parser->current;

	if (first->kind == TOKEN_PIPE)
	{
		// Union literal type: |i32 string|. Unions cannot nest, so the
		// first pipe at member level always closes the union.
		parser_advance(parser);

		size_t members = 0;

		while (!parser_check(parser, TOKEN_PIPE))
		{
			if (parser_check(parser, TOKEN_EOF))
			{
				return PARSER_TYPE_ABSENT;
			}

			if (!parser_scan_union_member(parser))
			{
				if (parser_check(parser, TOKEN_EOF))
				{
					return PARSER_TYPE_ABSENT;
				}

				// Not the start of a member: keep scanning to the
				// closing pipe so the validator can report the
				// offending token.
				parser_advance(parser);
				continue;
			}

			members++;
		}

		const Token *close = parser_peek(parser);

		parser_advance(parser);

		if (members == 0 && parser_is_type_start(parser_peek(parser)->kind))
		{
			parser_error(close, "union types cannot nest");
			return PARSER_TYPE_INVALID;
		}

		if (!parser_validate_type(first, parser->current - start))
		{
			return PARSER_TYPE_INVALID;
		}

		type->first = first;
		type->count = parser->current - start;
		return PARSER_TYPE_VALID;
	}

	if (first->kind == TOKEN_LEFT_CURLY)
	{
		// Struct literal type: {i32 x i32 y}.
		if (!parser_scan_struct_type(parser))
		{
			return PARSER_TYPE_ABSENT;
		}

		if (!parser_validate_type(first, parser->current - start))
		{
			return PARSER_TYPE_INVALID;
		}

		type->first = first;
		type->count = parser->current - start;
		return PARSER_TYPE_VALID;
	}

	if (!parser_is_type_start(first->kind))
	{
		return PARSER_TYPE_ABSENT;
	}

	parser_advance(parser);

	while (parser_check(parser, TOKEN_DOT) && parser_peek_at(parser, 1)->kind == TOKEN_IDENTIFIER)
	{
		parser_advance(parser);
		parser_advance(parser);
	}

	if (parser_check(parser, TOKEN_LESS))
	{
		if (!parser_scan_generic_arguments(parser))
		{
			return PARSER_TYPE_ABSENT;
		}
	}

	if (!parser_validate_type(first, parser->current - start))
	{
		return PARSER_TYPE_INVALID;
	}

	type->first = first;
	type->count = parser->current - start;
	return PARSER_TYPE_VALID;
}

static bool
parser_parse_name(Parser *parser, AstName *name)
{
	const Token *first = parser_peek(parser);

	if (first->kind != TOKEN_IDENTIFIER && first->kind != TOKEN_ERROR)
	{
		return false;
	}

	size_t start = parser->current;

	parser_advance(parser);

	while (parser_check(parser, TOKEN_DOT)
		&& (parser_peek_at(parser, 1)->kind == TOKEN_IDENTIFIER || parser_peek_at(parser, 1)->kind == TOKEN_ERROR))
	{
		parser_advance(parser);
		parser_advance(parser);
	}

	name->first = first;
	name->count = parser->current - start;
	return true;
}

static bool
parser_is_function_ahead(const Parser *parser)
{
	size_t index = parser->current + 1;

	while (index < parser->count)
	{
		TokenKind kind = parser->tokens[index].kind;

		if (kind == TOKEN_PUBLIC || kind == TOKEN_PRIVATE || kind == TOKEN_UNSAFE)
		{
			index++;
			continue;
		}

		return kind == TOKEN_FUNCTION;
	}

	return false;
}

static int
parser_parse_simple(Parser *parser, AstBody *body)
{
	const Token *token = parser_peek(parser);
	bool is_literal = parser_is_literal(token->kind);

	AstNode *node = parser_new_node(token, is_literal ? AST_LITERAL : AST_WORD);

	if (node == NULL)
	{
		return EXIT_FAILURE;
	}

	if (is_literal)
	{
		node->as.literal.token = *token;
	}
	else
	{
		node->as.word.name = (AstName) { .first = token, .count = 1 };
	}

	parser_advance(parser);

	if (parser_ast_body_push(body, node) != EXIT_SUCCESS)
	{
		parser_free_node(node);
		return EXIT_FAILURE;
	}

	return EXIT_SUCCESS;
}

static int
parser_parse_require(Parser *parser, AstBody *body)
{
	const Token *token = parser_peek(parser);
	const Token *path = parser_expect(parser, TOKEN_STRING, "a module path");

	if (path == NULL)
	{
		return EXIT_FAILURE;
	}

	AstNode *node = parser_new_node(token, AST_DECL_REQUIRE);

	if (node == NULL)
	{
		return EXIT_FAILURE;
	}

	node->as.require.path = *path;

	if (parser_check(parser, TOKEN_IDENTIFIER) || parser_check(parser, TOKEN_ERROR))
	{
		node->as.require.has_scope = true;
		node->as.require.scope = (AstName) { .first = parser_peek(parser), .count = 1 };
		parser_advance(parser);
	}

	if (parser_expect(parser, TOKEN_REQUIRE, "`require`") == NULL)
	{
		parser_free_node(node);
		return EXIT_FAILURE;
	}

	if (parser_ast_body_push(body, node) != EXIT_SUCCESS)
	{
		parser_free_node(node);
		return EXIT_FAILURE;
	}

	return EXIT_SUCCESS;
}

static int
parser_parse_if(Parser *parser, AstBody *body)
{
	const Token *token = parser_peek(parser);

	parser_advance(parser);

	AstNode *node = parser_new_node(token, AST_IF);

	if (node == NULL)
	{
		return EXIT_FAILURE;
	}

	static const TokenKind then_stops[] = { TOKEN_ELSE, TOKEN_END };

	if (parser_parse_body(parser, &node->as.if_else.then_body, then_stops, 2) != EXIT_SUCCESS)
	{
		goto error;
	}

	if (parser_match(parser, TOKEN_ELSE))
	{
		node->as.if_else.has_else = true;

		static const TokenKind else_stops[] = { TOKEN_END };

		if (parser_parse_body(parser, &node->as.if_else.else_body, else_stops, 1) != EXIT_SUCCESS)
		{
			goto error;
		}
	}

	if (parser_expect(parser, TOKEN_END, "`end`") == NULL)
	{
		goto error;
	}

	if (parser_ast_body_push(body, node) != EXIT_SUCCESS)
	{
		goto error;
	}

	return EXIT_SUCCESS;

	error:
	parser_free_node(node);
	return EXIT_FAILURE;
}

static int
parser_parse_block(Parser *parser, AstBody *body, TokenKind kind)
{
	const Token *token = parser_peek(parser);
	AstKind node_kind;

	switch (kind)
	{
		case TOKEN_FOR:
			node_kind = AST_FOR;
			break;

		case TOKEN_DEFER:
			node_kind = AST_DEFER;
			break;

		default:
			node_kind = AST_UNSAFE;
			break;
	}

	parser_advance(parser);

	AstNode *node = parser_new_node(token, node_kind);

	if (node == NULL)
	{
		return EXIT_FAILURE;
	}

	static const TokenKind stops[] = { TOKEN_END };

	if (parser_parse_body(parser, &node->as.block.body, stops, 1) != EXIT_SUCCESS)
	{
		goto error;
	}

	if (parser_expect(parser, TOKEN_END, "`end`") == NULL)
	{
		goto error;
	}

	if (parser_ast_body_push(body, node) != EXIT_SUCCESS)
	{
		goto error;
	}

	return EXIT_SUCCESS;

	error:
	parser_free_node(node);
	return EXIT_FAILURE;
}

static int
parser_parse_match(Parser *parser, AstBody *body)
{
	const Token *token = parser_peek(parser);

	parser_advance(parser);

	AstNode *node = parser_new_node(token, AST_MATCH);

	if (node == NULL)
	{
		return EXIT_FAILURE;
	}

	while (!parser_check(parser, TOKEN_END) && !parser_check(parser, TOKEN_ELSE) && !parser_check(parser, TOKEN_EOF))
	{
		const Token *case_token = parser_peek(parser);
		AstNode *case_node = parser_new_node(case_token, AST_CASE);

		if (case_node == NULL)
		{
			goto error;
		}

		static const TokenKind condition_stops[] = { TOKEN_CASE };

		if (parser_parse_body(parser, &case_node->as.match_case.condition, condition_stops, 1) != EXIT_SUCCESS)
		{
			parser_free_node(case_node);
			goto error;
		}

		if (parser_expect(parser, TOKEN_CASE, "`case`") == NULL)
		{
			parser_free_node(case_node);
			goto error;
		}

		// A multi-case group shares one body: c1 case c2 case body end.
		// A segment closed by `case` is another condition, the one closed
		// by `end` is the body of the whole group. `else` stops the group
		// so a missing `end` is reported at the `else` instead of leaking
		// into the body parse.
		static const TokenKind group_stops[] = { TOKEN_CASE, TOKEN_END, TOKEN_ELSE };
		AstBody segment = { 0 };

		if (parser_parse_body(parser, &segment, group_stops, 3) != EXIT_SUCCESS)
		{
			parser_free_body(&segment);
			parser_free_node(case_node);
			goto error;
		}

		while (parser_check(parser, TOKEN_CASE))
		{
			if (parser_ast_body_append(&case_node->as.match_case.condition, &segment) != EXIT_SUCCESS)
			{
				parser_free_body(&segment);
				parser_free_node(case_node);
				goto error;
			}

			parser_advance(parser);

			if (parser_parse_body(parser, &segment, group_stops, 3) != EXIT_SUCCESS)
			{
				parser_free_body(&segment);
				parser_free_node(case_node);
				goto error;
			}
		}

		case_node->as.match_case.body = segment;

		// Every case group is closed by its own `end`, before the `else`
		// or the `end` of the enclosing match.
		if (parser_expect(parser, TOKEN_END, "`end`") == NULL)
		{
			parser_free_node(case_node);
			goto error;
		}

		if (parser_ast_body_push(&node->as.match.cases, case_node) != EXIT_SUCCESS)
		{
			parser_free_node(case_node);
			goto error;
		}
	}

	if (parser_match(parser, TOKEN_ELSE))
	{
		node->as.match.has_else = true;

		static const TokenKind else_stops[] = { TOKEN_END };

		if (parser_parse_body(parser, &node->as.match.else_body, else_stops, 1) != EXIT_SUCCESS)
		{
			goto error;
		}

		// The else block is closed by its own `end`, before the one closing the match.
		if (parser_expect(parser, TOKEN_END, "`end`") == NULL)
		{
			goto error;
		}
	}

	if (parser_expect(parser, TOKEN_END, "`end`") == NULL)
	{
		goto error;
	}

	if (parser_ast_body_push(body, node) != EXIT_SUCCESS)
	{
		goto error;
	}

	return EXIT_SUCCESS;

	error:
	parser_free_node(node);
	return EXIT_FAILURE;
}

static int
parser_parse_name_word(Parser *parser, AstBody *body)
{
	const Token *token = parser_peek(parser);

	if (token->kind == TOKEN_IDENTIFIER && parser_is_function_ahead(parser))
	{
		return parser_parse_declaration(parser, body);
	}

	// Type cast: <Type> as
	size_t saved = parser->current;
	AstType type;
	ParserTypeStatus status = parser_parse_type(parser, &type);

	if (status == PARSER_TYPE_INVALID)
	{
		return EXIT_FAILURE;
	}

	if (status == PARSER_TYPE_VALID && parser_check(parser, TOKEN_AS))
	{
		parser_advance(parser);

		AstNode *node = parser_new_node(token, AST_AS);

		if (node == NULL)
		{
			return EXIT_FAILURE;
		}

		node->as.as_cast.type = type;

		if (parser_ast_body_push(body, node) != EXIT_SUCCESS)
		{
			parser_free_node(node);
			return EXIT_FAILURE;
		}

		return EXIT_SUCCESS;
	}

	parser->current = saved;

	AstName name;

	if (!parser_parse_name(parser, &name))
	{
		return parser_parse_simple(parser, body);
	}

	TokenKind next = parser_peek(parser)->kind;

	// Variable declaration: name (mutable|const|static) Type
	if (name.count == 1 && (next == TOKEN_MUTABLE || next == TOKEN_CONST || next == TOKEN_STATIC))
	{
		const Token *mutability = parser_peek(parser);

		parser_advance(parser);

		const Token *type_token = parser_peek(parser);
		AstType variable_type;
		ParserTypeStatus status = parser_parse_type(parser, &variable_type);

		if (status != PARSER_TYPE_VALID)
		{
			if (status == PARSER_TYPE_ABSENT)
			{
				parser_error(type_token, "expected a type after `%.*s`", (int) mutability->length, mutability->lexeme);
			}

			return EXIT_FAILURE;
		}

		AstNode *node = parser_new_node(token, AST_VARIABLE);

		if (node == NULL)
		{
			return EXIT_FAILURE;
		}

		node->as.variable.name = name;
		node->as.variable.mutability = *mutability;
		node->as.variable.type = variable_type;

		if (parser_ast_body_push(body, node) != EXIT_SUCCESS)
		{
			parser_free_node(node);
			return EXIT_FAILURE;
		}

		return EXIT_SUCCESS;
	}

	// Assignment: name set
	if (next == TOKEN_SET)
	{
		parser_advance(parser);

		AstNode *node = parser_new_node(token, AST_SET);

		if (node == NULL)
		{
			return EXIT_FAILURE;
		}

		node->as.set.name = name;

		if (parser_ast_body_push(body, node) != EXIT_SUCCESS)
		{
			parser_free_node(node);
			return EXIT_FAILURE;
		}

		return EXIT_SUCCESS;
	}

	// Call with an optional handler: name call [handle ... end]
	if (next == TOKEN_CALL)
	{
		parser_advance(parser);

		AstNode *node;

		if (parser_check(parser, TOKEN_HANDLE))
		{
			parser_advance(parser);

			node = parser_new_node(token, AST_HANDLE);

			if (node == NULL)
			{
				return EXIT_FAILURE;
			}

			node->as.handle.name = name;

			static const TokenKind stops[] = { TOKEN_END };

			if (parser_parse_body(parser, &node->as.handle.body, stops, 1) != EXIT_SUCCESS
				|| parser_expect(parser, TOKEN_END, "`end`") == NULL)
			{
				parser_free_node(node);
				return EXIT_FAILURE;
			}
		}
		else
		{
			node = parser_new_node(token, AST_CALL);

			if (node == NULL)
			{
				return EXIT_FAILURE;
			}

			node->as.call.name = name;
		}

		if (parser_ast_body_push(body, node) != EXIT_SUCCESS)
		{
			parser_free_node(node);
			return EXIT_FAILURE;
		}

		return EXIT_SUCCESS;
	}

	// Plain word, possibly qualified like io.write_line
	AstNode *node = parser_new_node(token, AST_WORD);

	if (node == NULL)
	{
		return EXIT_FAILURE;
	}

	node->as.word.name = name;

	if (parser_ast_body_push(body, node) != EXIT_SUCCESS)
	{
		parser_free_node(node);
		return EXIT_FAILURE;
	}

	return EXIT_SUCCESS;
}

static int
parser_parse_word(Parser *parser, AstBody *body)
{
	const Token *token = parser_peek(parser);

	switch (token->kind)
	{
		case TOKEN_IF:
			return parser_parse_if(parser, body);

		case TOKEN_MATCH:
			return parser_parse_match(parser, body);

		case TOKEN_FOR:
		case TOKEN_DEFER:
		case TOKEN_UNSAFE:
			return parser_parse_block(parser, body, token->kind);

		case TOKEN_STRING:
		{
			// A string followed by [scope] require is a module import.
			const Token *next = parser_peek_at(parser, 1);
			const Token *after = parser_peek_at(parser, 2);
			bool is_scope = next->kind == TOKEN_IDENTIFIER || next->kind == TOKEN_ERROR;

			if (next->kind == TOKEN_REQUIRE || (is_scope && after->kind == TOKEN_REQUIRE))
			{
				return parser_parse_require(parser, body);
			}

			break;
		}

		case TOKEN_ELSE:
			parser_error(token, "unexpected `else` in a body; `else` is only allowed after an `if` branch");
			return EXIT_FAILURE;

		case TOKEN_CASE:
			parser_error(token, "unexpected `case` in a body; `case` is only allowed inside a `match`");
			return EXIT_FAILURE;

		case TOKEN_DO:
			parser_error(token, "unexpected `do` in a body; `do` is only allowed after a function signature");
			return EXIT_FAILURE;

		case TOKEN_WITH:
			parser_error(token, "unexpected `with` in a body; `with` is only allowed after a function `end` to declare its return type");
			return EXIT_FAILURE;

		case TOKEN_PIPE:
			parser_error(token, "unexpected `|` in a body; `|` is only used in union types and unions cannot nest");
			return EXIT_FAILURE;

		case TOKEN_IDENTIFIER:
		case TOKEN_TYPE:
		case TOKEN_ERROR:
			return parser_parse_name_word(parser, body);

		default:
			break;
	}

	return parser_parse_simple(parser, body);
}

static bool
parser_at_stop(const Parser *parser, const TokenKind *stops, size_t stop_count)
{
	TokenKind kind = parser_peek(parser)->kind;

	for (size_t i = 0; i < stop_count; i++)
	{
		if (kind == stops[i])
		{
			return true;
		}
	}

	return false;
}

static int
parser_parse_body(Parser *parser, AstBody *body, const TokenKind *stops, size_t stop_count)
{
	while (!parser_at_stop(parser, stops, stop_count))
	{
		if (parser_check(parser, TOKEN_EOF))
		{
			parser_error(parser_peek(parser), "unexpected end of file");
			return EXIT_FAILURE;
		}

		if (parser_parse_word(parser, body) != EXIT_SUCCESS)
		{
			return EXIT_FAILURE;
		}
	}

	return EXIT_SUCCESS;
}

static int
parser_parse_function(Parser *parser, AstBody *program, const Token *name_token, AstName name, bool is_public, bool is_unsafe)
{
	AstNode *node = parser_new_node(name_token, AST_DECL_FUNCTION);

	if (node == NULL)
	{
		return EXIT_FAILURE;
	}

	node->as.function.name = name;
	node->as.function.is_public = is_public;
	node->as.function.is_unsafe = is_unsafe;

	while (!parser_check(parser, TOKEN_DO) && !parser_check(parser, TOKEN_EOF))
	{
		const Token *param_token = parser_peek(parser);
		AstType type;
		ParserTypeStatus status = parser_parse_type(parser, &type);

		if (status != PARSER_TYPE_VALID)
		{
			if (status == PARSER_TYPE_ABSENT)
			{
				parser_error(param_token, "expected a parameter type but found `%.*s`", (int) param_token->length, param_token->lexeme);
			}

			goto error;
		}

		AstNode *param = parser_new_node(param_token, AST_PARAM);

		if (param == NULL)
		{
			goto error;
		}

		param->as.param.type = type;

		if (parser_ast_body_push(&node->as.function.params, param) != EXIT_SUCCESS)
		{
			parser_free_node(param);
			goto error;
		}
	}

	if (parser_expect(parser, TOKEN_DO, "`do`") == NULL)
	{
		goto error;
	}

	static const TokenKind stops[] = { TOKEN_END };

	if (parser_parse_body(parser, &node->as.function.body, stops, 1) != EXIT_SUCCESS)
	{
		goto error;
	}

	if (parser_expect(parser, TOKEN_END, "`end`") == NULL)
	{
		goto error;
	}

	if (parser_match(parser, TOKEN_WITH))
	{
		const Token *result_token = parser_peek(parser);
		ParserTypeStatus status = parser_parse_type(parser, &node->as.function.result);

		if (status != PARSER_TYPE_VALID)
		{
			if (status == PARSER_TYPE_ABSENT)
			{
				parser_error(result_token, "expected a return type after `with`");
			}

			goto error;
		}

		node->as.function.has_result = true;
	}

	if (parser_ast_body_push(program, node) != EXIT_SUCCESS)
	{
		goto error;
	}

	return EXIT_SUCCESS;

	error:
	parser_free_node(node);
	return EXIT_FAILURE;
}

static int
parser_parse_struct(Parser *parser, AstBody *program, const Token *name_token, AstName name, bool is_public)
{
	AstNode *node = parser_new_node(name_token, AST_DECL_STRUCT);

	if (node == NULL)
	{
		return EXIT_FAILURE;
	}

	node->as.struct_decl.name = name;
	node->as.struct_decl.is_public = is_public;

	while (!parser_check(parser, TOKEN_END) && !parser_check(parser, TOKEN_EOF))
	{
		const Token *field_token = parser_peek(parser);
		AstType type;
		ParserTypeStatus status = parser_parse_type(parser, &type);

		if (status != PARSER_TYPE_VALID)
		{
			if (status == PARSER_TYPE_ABSENT)
			{
				parser_error(field_token, "expected a field type but found `%.*s`", (int) field_token->length, field_token->lexeme);
			}

			goto error;
		}

		const Token *field_name = parser_expect(parser, TOKEN_IDENTIFIER, "a field name");

		if (field_name == NULL)
		{
			goto error;
		}

		bool field_public = false;

		if (parser_check(parser, TOKEN_PUBLIC) || parser_check(parser, TOKEN_PRIVATE))
		{
			field_public = parser_check(parser, TOKEN_PUBLIC);
			parser_advance(parser);
		}

		AstNode *field = parser_new_node(field_token, AST_FIELD);

		if (field == NULL)
		{
			goto error;
		}

		field->as.field.type = type;
		field->as.field.name = (AstName) { .first = field_name, .count = 1 };
		field->as.field.is_public = field_public;

		if (parser_ast_body_push(&node->as.struct_decl.fields, field) != EXIT_SUCCESS)
		{
			parser_free_node(field);
			goto error;
		}
	}

	if (parser_expect(parser, TOKEN_END, "`end`") == NULL)
	{
		goto error;
	}

	if (parser_ast_body_push(program, node) != EXIT_SUCCESS)
	{
		goto error;
	}

	return EXIT_SUCCESS;

	error:
	parser_free_node(node);
	return EXIT_FAILURE;
}

static int
parser_parse_members(Parser *parser, AstBody *members)
{
	while (!parser_check(parser, TOKEN_END) && !parser_check(parser, TOKEN_EOF))
	{
		const Token *member_token = parser_peek(parser);
		AstName name;

		if (!parser_parse_name(parser, &name))
		{
			parser_error(member_token, "expected a member name but found `%.*s`", (int) member_token->length, member_token->lexeme);
			return EXIT_FAILURE;
		}

		AstNode *node = parser_new_node(member_token, AST_ENUM_MEMBER);

		if (node == NULL)
		{
			return EXIT_FAILURE;
		}

		node->as.enum_member.name = name;

		if (parser_is_value_literal(parser_peek(parser)->kind))
		{
			node->as.enum_member.has_value = true;
			node->as.enum_member.value = *parser_peek(parser);
			parser_advance(parser);
		}

		if (parser_ast_body_push(members, node) != EXIT_SUCCESS)
		{
			parser_free_node(node);
			return EXIT_FAILURE;
		}
	}

	return parser_expect(parser, TOKEN_END, "`end`") == NULL ? EXIT_FAILURE : EXIT_SUCCESS;
}

static int
parser_parse_enum(Parser *parser, AstBody *program, const Token *name_token, AstName name, bool is_public, AstType carrier, bool has_carrier)
{
	AstNode *node = parser_new_node(name_token, AST_DECL_ENUM);

	if (node == NULL)
	{
		return EXIT_FAILURE;
	}

	node->as.enum_decl.name = name;
	node->as.enum_decl.is_public = is_public;
	node->as.enum_decl.carrier = carrier;
	node->as.enum_decl.has_carrier = has_carrier;

	if (parser_parse_members(parser, &node->as.enum_decl.members) != EXIT_SUCCESS)
	{
		goto error;
	}

	if (parser_ast_body_push(program, node) != EXIT_SUCCESS)
	{
		goto error;
	}

	return EXIT_SUCCESS;

	error:
	parser_free_node(node);
	return EXIT_FAILURE;
}

static int
parser_parse_union(Parser *parser, AstBody *program, const Token *name_token, AstName name, bool is_public)
{
	AstNode *node = parser_new_node(name_token, AST_DECL_UNION);

	if (node == NULL)
	{
		return EXIT_FAILURE;
	}

	node->as.union_decl.name = name;
	node->as.union_decl.is_public = is_public;

	while (!parser_check(parser, TOKEN_END) && !parser_check(parser, TOKEN_EOF))
	{
		const Token *member_token = parser_peek(parser);
		AstType type;
		ParserTypeStatus status = parser_parse_type(parser, &type);

		if (status != PARSER_TYPE_VALID)
		{
			if (status == PARSER_TYPE_ABSENT)
			{
				parser_error(member_token, "expected a member type but found `%.*s`", (int) member_token->length, member_token->lexeme);
			}

			goto error;
		}

		AstNode *member = parser_new_node(member_token, AST_UNION_MEMBER);

		if (member == NULL)
		{
			goto error;
		}

		member->as.union_member.type = type;

		if (parser_ast_body_push(&node->as.union_decl.members, member) != EXIT_SUCCESS)
		{
			parser_free_node(member);
			goto error;
		}
	}

	if (parser_expect(parser, TOKEN_END, "`end`") == NULL)
	{
		goto error;
	}

	if (parser_ast_body_push(program, node) != EXIT_SUCCESS)
	{
		goto error;
	}

	return EXIT_SUCCESS;

	error:
	parser_free_node(node);
	return EXIT_FAILURE;
}

static int
parser_parse_error_decl(Parser *parser, AstBody *program, const Token *name_token, AstName name, bool is_public)
{
	AstNode *node = parser_new_node(name_token, AST_DECL_ERROR);

	if (node == NULL)
	{
		return EXIT_FAILURE;
	}

	node->as.error_decl.name = name;
	node->as.error_decl.is_public = is_public;

	if (parser_parse_members(parser, &node->as.error_decl.members) != EXIT_SUCCESS)
	{
		goto error;
	}

	if (parser_ast_body_push(program, node) != EXIT_SUCCESS)
	{
		goto error;
	}

	return EXIT_SUCCESS;

	error:
	parser_free_node(node);
	return EXIT_FAILURE;
}

static int
parser_parse_implement(Parser *parser, AstBody *program, const Token *name_token, AstName name)
{
	AstNode *node = parser_new_node(name_token, AST_DECL_IMPLEMENT);

	if (node == NULL)
	{
		return EXIT_FAILURE;
	}

	node->as.implement.name = name;

	while (!parser_check(parser, TOKEN_END) && !parser_check(parser, TOKEN_EOF))
	{
		if (parser_parse_declaration(parser, &node->as.implement.methods) != EXIT_SUCCESS)
		{
			goto error;
		}
	}

	if (parser_expect(parser, TOKEN_END, "`end`") == NULL)
	{
		goto error;
	}

	if (parser_ast_body_push(program, node) != EXIT_SUCCESS)
	{
		goto error;
	}

	return EXIT_SUCCESS;

	error:
	parser_free_node(node);
	return EXIT_FAILURE;
}

static int
parser_parse_named_declaration(Parser *parser, AstBody *program)
{
	const Token *name_token = parser_peek(parser);
	AstName name;

	if (!parser_parse_name(parser, &name))
	{
		parser_error(name_token, "expected a declaration name but found `%.*s`", (int) name_token->length, name_token->lexeme);
		return EXIT_FAILURE;
	}

	bool is_public = false;

	if (parser_check(parser, TOKEN_PUBLIC) || parser_check(parser, TOKEN_PRIVATE))
	{
		is_public = parser_check(parser, TOKEN_PUBLIC);
		parser_advance(parser);
	}

	bool is_unsafe = parser_match(parser, TOKEN_UNSAFE);

	// An enum can declare its carrier type before the `enum` keyword.
	AstType carrier = { 0 };
	bool has_carrier = false;

	if (parser_is_type_start(parser_peek(parser)->kind))
	{
		size_t saved = parser->current;
		ParserTypeStatus status = parser_parse_type(parser, &carrier);

		if (status == PARSER_TYPE_VALID)
		{
			has_carrier = true;
		}
		else
		{
			parser->current = saved;

			if (status == PARSER_TYPE_INVALID)
			{
				return EXIT_FAILURE;
			}
		}
	}

	const Token *keyword = parser_peek(parser);

	switch (keyword->kind)
	{
		case TOKEN_FUNCTION:
			parser_advance(parser);
			return parser_parse_function(parser, program, name_token, name, is_public, is_unsafe);

		case TOKEN_STRUCT:
			parser_advance(parser);
			return parser_parse_struct(parser, program, name_token, name, is_public);

		case TOKEN_ENUM:
			parser_advance(parser);
			return parser_parse_enum(parser, program, name_token, name, is_public, carrier, has_carrier);

		case TOKEN_UNION:
			parser_advance(parser);
			return parser_parse_union(parser, program, name_token, name, is_public);

		case TOKEN_ERROR:
			parser_advance(parser);
			return parser_parse_error_decl(parser, program, name_token, name, is_public);

		case TOKEN_IMPLEMENT:
			parser_advance(parser);
			return parser_parse_implement(parser, program, name_token, name);

		default:
			parser_error(keyword, "expected a declaration keyword but found `%.*s`", (int) keyword->length, keyword->lexeme);
			return EXIT_FAILURE;
	}
}

static int
parser_parse_declaration(Parser *parser, AstBody *program)
{
	if (parser_check(parser, TOKEN_STRING))
	{
		return parser_parse_require(parser, program);
	}

	return parser_parse_named_declaration(parser, program);
}

static void
parser_free_body(AstBody *body)
{
	for (size_t i = 0; i < body->count; i++)
	{
		parser_free_node(body->items[i]);
	}

	free(body->items);
	body->items = NULL;
	body->count = 0;
	body->capacity = 0;
}

static void
parser_free_node(AstNode *node)
{
	switch (node->kind)
	{
		case AST_DECL_FUNCTION:
			parser_free_body(&node->as.function.params);
			parser_free_body(&node->as.function.body);
			break;

		case AST_DECL_STRUCT:
			parser_free_body(&node->as.struct_decl.fields);
			break;

		case AST_DECL_ENUM:
			parser_free_body(&node->as.enum_decl.members);
			break;

		case AST_DECL_UNION:
			parser_free_body(&node->as.union_decl.members);
			break;

		case AST_DECL_ERROR:
			parser_free_body(&node->as.error_decl.members);
			break;

		case AST_DECL_IMPLEMENT:
			parser_free_body(&node->as.implement.methods);
			break;

		case AST_IF:
			parser_free_body(&node->as.if_else.then_body);
			parser_free_body(&node->as.if_else.else_body);
			break;

		case AST_MATCH:
			parser_free_body(&node->as.match.cases);
			parser_free_body(&node->as.match.else_body);
			break;

		case AST_CASE:
			parser_free_body(&node->as.match_case.condition);
			parser_free_body(&node->as.match_case.body);
			break;

		case AST_FOR:
		case AST_DEFER:
		case AST_UNSAFE:
			parser_free_body(&node->as.block.body);
			break;

		case AST_HANDLE:
			parser_free_body(&node->as.handle.body);
			break;

		default:
			break;
	}

	free(node);
}

static void
parser_print_indent(int depth)
{
	for (int i = 0; i < depth; i++)
	{
		printf("  ");
	}
}

static void
parser_print_token(const Token *token)
{
	printf("%.*s", (int) token->length, token->lexeme);
}

static void
parser_print_span(const AstSpan *span)
{
	for (size_t i = 0; i < span->count; i++)
	{
		parser_print_token(&span->first[i]);
	}
}

static void
parser_print_body(const AstBody *body, int depth)
{
	for (size_t i = 0; i < body->count; i++)
	{
		parser_print_node(body->items[i], depth);
	}
}

static void
parser_print_node(const AstNode *node, int depth)
{
	parser_print_indent(depth);

	switch (node->kind)
	{
		case AST_DECL_REQUIRE:
			printf("require ");
			parser_print_token(&node->as.require.path);

			if (node->as.require.has_scope)
			{
				printf(" ");
				parser_print_span(&node->as.require.scope);
			}

			printf("\n");
			break;

		case AST_DECL_FUNCTION:
			printf("function ");
			parser_print_span(&node->as.function.name);
			printf("%s", node->as.function.is_public ? " public" : " private");

			if (node->as.function.is_unsafe)
			{
				printf(" unsafe");
			}

			printf("\n");
			parser_print_body(&node->as.function.params, depth + 1);
			parser_print_body(&node->as.function.body, depth + 1);

			if (node->as.function.has_result)
			{
				parser_print_indent(depth);
				printf("result ");
				parser_print_span(&node->as.function.result);
				printf("\n");
			}
			break;

		case AST_DECL_STRUCT:
			printf("struct ");
			parser_print_span(&node->as.struct_decl.name);
			printf("%s\n", node->as.struct_decl.is_public ? " public" : " private");
			parser_print_body(&node->as.struct_decl.fields, depth + 1);
			break;

		case AST_DECL_ENUM:
			printf("enum ");
			parser_print_span(&node->as.enum_decl.name);

			if (node->as.enum_decl.has_carrier)
			{
				printf(" carrier ");
				parser_print_span(&node->as.enum_decl.carrier);
			}

			printf("\n");
			parser_print_body(&node->as.enum_decl.members, depth + 1);
			break;

		case AST_DECL_UNION:
			printf("union ");
			parser_print_span(&node->as.union_decl.name);
			printf("\n");
			parser_print_body(&node->as.union_decl.members, depth + 1);
			break;

		case AST_DECL_ERROR:
			printf("error ");
			parser_print_span(&node->as.error_decl.name);
			printf("\n");
			parser_print_body(&node->as.error_decl.members, depth + 1);
			break;

		case AST_DECL_IMPLEMENT:
			printf("implement ");
			parser_print_span(&node->as.implement.name);
			printf("\n");
			parser_print_body(&node->as.implement.methods, depth + 1);
			break;

		case AST_PARAM:
			printf("param ");
			parser_print_span(&node->as.param.type);
			printf("\n");
			break;

		case AST_FIELD:
			printf("field ");
			parser_print_span(&node->as.field.type);
			printf(" ");
			parser_print_span(&node->as.field.name);
			printf("%s\n", node->as.field.is_public ? " public" : " private");
			break;

		case AST_ENUM_MEMBER:
			printf("member ");
			parser_print_span(&node->as.enum_member.name);

			if (node->as.enum_member.has_value)
			{
				printf(" = ");
				parser_print_token(&node->as.enum_member.value);
			}

			printf("\n");
			break;

		case AST_UNION_MEMBER:
			printf("member ");
			parser_print_span(&node->as.union_member.type);
			printf("\n");
			break;

		case AST_LITERAL:
			printf("literal ");
			parser_print_token(&node->as.literal.token);
			printf("\n");
			break;

		case AST_WORD:
			printf("word ");
			parser_print_span(&node->as.word.name);
			printf("\n");
			break;

		case AST_VARIABLE:
			printf("variable ");
			parser_print_span(&node->as.variable.name);
			printf(" ");
			parser_print_token(&node->as.variable.mutability);
			printf(" ");
			parser_print_span(&node->as.variable.type);
			printf("\n");
			break;

		case AST_SET:
			printf("set ");
			parser_print_span(&node->as.set.name);
			printf("\n");
			break;

		case AST_CALL:
			printf("call ");
			parser_print_span(&node->as.call.name);
			printf("\n");
			break;

		case AST_AS:
			printf("as ");
			parser_print_span(&node->as.as_cast.type);
			printf("\n");
			break;

		case AST_IF:
			printf("if\n");
			parser_print_body(&node->as.if_else.then_body, depth + 1);

			if (node->as.if_else.has_else)
			{
				parser_print_indent(depth);
				printf("else\n");
				parser_print_body(&node->as.if_else.else_body, depth + 1);
			}
			break;

		case AST_MATCH:
			printf("match\n");
			parser_print_body(&node->as.match.cases, depth + 1);

			if (node->as.match.has_else)
			{
				parser_print_indent(depth);
				printf("else\n");
				parser_print_body(&node->as.match.else_body, depth + 1);
			}
			break;

		case AST_CASE:
			printf("case\n");
			parser_print_body(&node->as.match_case.condition, depth + 1);
			parser_print_indent(depth + 1);
			printf("body\n");
			parser_print_body(&node->as.match_case.body, depth + 2);
			break;

		case AST_FOR:
			printf("for\n");
			parser_print_body(&node->as.block.body, depth + 1);
			break;

		case AST_DEFER:
			printf("defer\n");
			parser_print_body(&node->as.block.body, depth + 1);
			break;

		case AST_UNSAFE:
			printf("unsafe\n");
			parser_print_body(&node->as.block.body, depth + 1);
			break;

		case AST_HANDLE:
			printf("handle ");
			parser_print_span(&node->as.handle.name);
			printf("\n");
			parser_print_body(&node->as.handle.body, depth + 1);
			break;
	}
}

void
parser_free(AstBody *program)
{
	if (program != NULL)
	{
		parser_free_body(program);
	}
}

void
parser_print(const AstBody *program)
{
	parser_print_body(program, 0);
}


int
parser_parse(const Token *tokens, size_t count, AstBody *program)
{
	if (tokens == NULL || count == 0 || program == NULL)
	{
		return EXIT_FAILURE;
	}

	Parser parser = {
		.tokens = tokens,
		.count = count,
		.current = 0
	};

	program->items = NULL;
	program->count = 0;
	program->capacity = 0;

	while (!parser_check(&parser, TOKEN_EOF))
	{
		if (parser_parse_declaration(&parser, program) != EXIT_SUCCESS)
		{
			parser_free(program);
			return EXIT_FAILURE;
		}
	}

	return EXIT_SUCCESS;
}
