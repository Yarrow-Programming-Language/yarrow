#ifndef CLI_H
#define CLI_H

#include <stddef.h>

typedef struct cli_flag CliFlag;
typedef struct cli_command CliCommand;

struct cli_flag {
	const char *name;
	const char *short_name;
	const char *description;

	int takes_value;
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
};

typedef struct {
	const char *name;
	const char *description;

	CliCommand *commands;
	size_t command_count;

	CliFlag *global_flags;
	size_t global_flag_count;
} Cli;

int cli_global_help(Cli *cli, CliCommand *command);
int cli_init(Cli *cli);
int cli_run(int argc, char *argv[]);

#endif
