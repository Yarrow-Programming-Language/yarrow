#include <stdbool.h>
#include <stddef.h>

#ifndef CLI_H
#define CLI_H

#define CLI_VERSION "0.0.1"

typedef struct cli_flag CliFlag;
typedef struct cli_command CliCommand;
typedef struct cli_parsed_flag CliParsedFlag;
typedef struct cli_parse_result CliParseResult;

typedef int (*CliCommandHandler)(const CliParseResult *result);

struct cli_parsed_flag {
	const CliFlag *flag;
	const char *value;
	bool present;
};

struct cli_parse_result {
	CliCommand *command;

	CliCommand **command_path;
	size_t command_depth;

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

int
cli_run(int argc, char **argv);

#endif
