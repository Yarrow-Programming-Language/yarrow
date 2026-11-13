#include <stdbool.h>
#include <stddef.h>

#ifndef CLI_H
#define CLI_H

#define CLI_VERSION "0.0.1"

typedef struct cli_flag CliFlag;
typedef struct cli_command CliCommand;
typedef struct cli_parsed_flag CliParsedFlag;
typedef struct cli_parsed_result CliParsedResult;

typedef int (*CliCommandHandler)(const CliParsedResult *result);

typedef enum {
	CLI_FLAG_NONE,
	CLI_FLAG_REQUIRED_VALUE,
	CLI_FLAG_OPTIONAL_VALUE
} CliFlagTakesValue;

struct cli_parsed_flag {
	const CliFlag *flag;
	const char *value;
	bool present;
};

struct cli_parsed_result {
	CliCommand *command;
	CliCommand **command_path;
	CliParsedFlag *flags;
	char **positionals;
};

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
	CliFlag *flags;
	CliCommandHandler handler;
};

typedef struct {
	const char *name;
	const char *description;
	CliCommand *commands;
	CliFlag *global_flags;
} Cli;

int cli_run(int argc, char **argv);

#endif
