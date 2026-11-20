## CLI
- [ ] Implement a system to get the main input file. If the input flag specify a path then use this. If there is a project file and it specify an input file path then use this one. Else try to find a main.yar file and read it.
- [ ] After adding the error number system, add an explain command to fetch the specific error number documentation.

## Tokenizer
- [ ] Tokenize text into identifier, keywords or underscore.
- [ ] Tokenize number into integers or floats or plus and minus if alone.
- [ ] Tokenize strings and runes.
- [ ] Break on parasite characters like tabulations and what not.
- [ ] Handle new lines and comments.
- [ ] Fix the issue with the EOF token. Points at the last consumed character but but an empty file yeilds EOF at 0:0.

## Parser
- [ ] WIP

## Compiler
- [ ] WIP

## Interpreter
- [ ] WIP

## Formatter
- [ ] WIP

## LSP
- [ ] WIP

## Global
- [ ] Create a global error system that show the location, range and error message. Create a system of error number with a documentation for each error number.
