#ifndef CLI_H
#define CLI_H

#include <stddef.h>

#define CLI_VERSION "0.0.1"

typedef struct cli_flag CliFlag;
typedef struct cli_command CliCommand;
typedef struct cli_parsed_flag CliParsedFlag;
typedef struct cli_parse_result CliParseResult;

typedef int (*CliCommandHandler)(const CliParseResult *result);

struct cli_parsed_flag {
	const CliFlag *flag;
	const char *value;
	int present;
};

struct cli_parse_result {
	CliCommand *command;

	CliParsedFlag *flags;
	size_t flag_count;

	char **positionals;
	size_t positional_count;
};

typedef enum {
	CLI_FLAG_NONE,
	CLI_FLAG_REQUIRED_VALUE,
	CLI_FLAG_OPTIONAL_VALUE
} CliFlagTakesValue;

struct cli_flag {
	const char *name;
	const char *short_name;
	const char *description;

	CliFlagTakesValue takes_value;
	const char *value_name;
	const char *default_value;
};

struct cli_command {
	const char *name;
	const char *description;

	CliCommand *subcommands;
	size_t subcommand_count;

	CliFlag *flags;
	size_t flag_count;

	CliCommandHandler handler;
};

typedef struct {
	const char *name;
	const char *description;

	CliCommand *commands;
	size_t command_count;

	CliFlag *global_flags;
	size_t global_flag_count;
} Cli;

int cli_run_command(const CliParseResult *result);
int cli_build_command(const CliParseResult *result);
int cli_interpret_command(const CliParseResult *result);
int cli_lsp_command(const CliParseResult *result);
int cli_fmt_command(const CliParseResult *result);
int cli_debug_command(const CliParseResult *result);
int cli_debug_ast_command(const CliParseResult *result);
int cli_debug_ir_command(const CliParseResult *result);
int cli_debug_tokens_command(const CliParseResult *result);
int cli_has_flag(const CliParseResult *result, const char *name);
const char *cli_get_flag_value(const CliParseResult *result, const char *name);
static CliCommand *cli_find_command(CliCommand *commands, size_t count, const char *name);
static CliFlag *cli_find_flag(CliFlag *flags, size_t count, const char *name);
static CliFlag *cli_resolve_flag(Cli *cli, CliCommand **commands, size_t command_count, const char *name);
int cli_parse(Cli *cli, int argc, char **argv, CliParseResult *result);
int cli_parse_free(CliParseResult *result);
int cli_help(Cli *cli, CliCommand *command);
int cli_init(Cli *cli);
int cli_run(int argc, char **argv);

#endif
