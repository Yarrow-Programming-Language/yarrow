# Syntax

EBNF for Yarrow, derived from [`GRAMMAR.md`](GRAMMAR.md).

Notation:

- `=` definition
- `|` alternation
- `[ X ]` optional
- `{ X }` zero or more
- `( X )` grouping
- `"lit"` terminal
- `(* ... *)` comment.

```ebnf
(* ===== Program ===== *)

program =
	{ top_level } , main_function , { top_level } ;

top_level =
		require_stmt
	| function_decl
	| struct_decl
	| implement_block
	| enum_decl
	| union_decl
	| error_decl
	;

(* Main is the only entity public by default; return type is optional. *)
main_function =
	"main" , "function" , "do" , { statement } , "end" , [ "with" , type ] ;


(* ===== Modules ===== *)

(* Alias imports into a named scope; omit the alias to import into the current scope. *)
require_stmt =
	string_literal , [ identifier ] , "require" ;


(* ===== Declarations ===== *)

visibility =
	"public" | "private" ;

function_decl =
	identifier , [ visibility ] , [ "unsafe" ] , "function" ,
	{ parameter } ,
	"do" , { statement } , "end" , [ "with" , type ] ;

(* Parameters are moved onto the local stack in declaration order.
	 `copy` deep-copies; `mutable` on a reference requires a mutable pointee. *)
parameter =
	type , [ "copy" | "mutable" ] ;

struct_decl =
	identifier , [ visibility ] , "struct" ,
	{ field_decl } ,
	"end" ;

field_decl =
	type , identifier , [ visibility ] ;

implement_block =
	identifier , "implement" ,
	{ function_decl } ,
	"end" ;

(* Default underlying type is i32; an explicit type yields e.g. a string enum. *)
enum_decl =
	identifier , [ type ] , "enum" ,
	{ enum_member } ,
	"end" ;

enum_member =
	identifier , [ integer_literal ] ;

union_decl =
	identifier , "union" ,
	type , { type } ,
	"end" ;

(* Optional qualified name injects members from another error type. *)
error_decl =
	identifier , [ qualified_name ] , "error" ,
	{ identifier } ,
	"end" ;


(* ===== Types ===== *)

type =
		primitive_type
	| generic_type
	| union_type_literal
	| qualified_name
	;

primitive_type =
		"void" | "bool" | "string" | "rune"
	| integer_type | float_type
	;

integer_type =
		"i8" | "i16" | "i32" | "i64"
	| "u8" | "u16" | "u32" | "u64"
	;

float_type =
	"f16" | "f32" | "f64" ;

(* Type arguments inside <> are whitespace-separated. *)
generic_type =
		"array" , "<" , type , [ integer_literal ] , ">"
	| "list" , "<" , type , ">"
	| "hashmap" , "<" , type , type , ">"
	| "pointer" , "<" , type , ">"
	| "reference" , "<" , type , ">"
	;

(* Anonymous union used e.g. as a function return type. *)
union_type_literal =
	"|" , type , { type } , "|" ;


(* ===== Statements & control flow ===== *)

statement =
		require_stmt
	| function_decl
	| var_decl
	| assignment
	| move_stmt
	| if_stmt
	| match_stmt
	| for_stmt
	| defer_stmt
	| unsafe_block
	| handle_stmt
	| fallback_stmt
	| return_stmt
	| word
	;

var_decl =
	(* value already on the stack *)
	lvalue , ( "mutable" | "const" | "static" ) , type ;

assignment =
	(* new value already on the stack *)
	lvalue , "set" ;

(* `source target move`: the source words are already on the stack;
	 the target name is the word right before `move`. *)
move_stmt =
	{ word } , identifier , "move" ;

lvalue =
	qualified_name ;

if_stmt =
	(* condition bool already on the stack *)
	"if" , { statement } , [ "else" , { statement } ] , "end" ;

(* Subject is on the stack before `match` (value or union). Inside `handle`,
	 `match` alone dispatches on the error. Subject is borrowed for the duration;
	 the original stack is restored afterward.
	 Value/error cases: words before `case` leave a bool.
	 Union cases: the word before `case` is a member type. *)
match_stmt =
	"match" , { match_case } , [ match_else ] , "end" ;

match_case =
	word , { word } , "case" , { statement } , "end" ;

match_else =
	"else" , { statement } , "end" ;

(* Condition form acts like while; iterable form iterates containers. *)
for_stmt =
	(* condition bool or iterable already on the stack *)
	"for" , { statement } , "end" ;

defer_stmt =
	"defer" , { statement } , "end" ;

unsafe_block =
	"unsafe" , { statement } , "end" ;

(* After a call that may error. The body is an optional handler; a `fallback`
   statement inside it supplies the recovery value.
   e.g. `call handle 0 fallback end`
		`call handle match ... end 0 fallback end` *)
handle_stmt =
	"handle" , { statement } , "end" ;

(* Only meaningful inside `handle ... end`: the words before `fallback`
   are the value pushed when the handled call failed. *)
fallback_stmt =
	{ word } , "fallback" ;

return_stmt =
	"return" ;


(* ===== Words (stack terms) ===== *)

(* A word is one stack effect: push a value, name lookup, operator, or keyword op. *)
word =
		literal
	| container_literal
	| qualified_name
	| type                 (* type values, e.g. for typeof / comparisons *)
	| operator
	| stack_op
	| memory_op
	| call_op
	| builtin
	| "typeof"
	;

literal =
		integer_literal
	| float_literal
	| string_literal
	| rune_literal
	| bool_literal
	;

bool_literal =
	"true" | "false" ;

container_literal =
		list_literal
	| array_literal
	| map_or_struct_literal
	;

list_literal =
	"(" , { word } , ")" ;

array_literal =
	"[" , { word } , "]" ;

(* Literal keys → hashmap; identifier keys → struct. Empty {} needs a typed context. *)
map_or_struct_literal =
	"{" , { word } , "}" ;

operator =
		arithmetic_op
	| concatenation_op
	| logical_op
	| comparison_op
	| bitwise_op
	;

arithmetic_op =
	"+" | "-" | "*" | "/" | "//" | "%" | "^" ;

(* String join only. `+` is never overloaded for concatenation. *)
concatenation_op =
	"~" ;

logical_op =
	"and" | "or" | "not" ;

comparison_op =
	"==" | "!=" | ">" | "<" | ">=" | "<=" ;

bitwise_op =
	"and" | "or" | "xor" | "lshift" | "rshift" | "not" ;

stack_op =
	"drop" | "dup" | "swap" | "rot" | "unrot" | "pop" ;

memory_op =
	"borrow" | "load" | "store" ;

call_op =
	"call" | "unwrap" ;

(* Raw host word: `@alloc`, `@print_str`, ... The memory builtins
   (`@alloc`, `@free`, `@load`, `@store`) require an unsafe context. *)
builtin =
	"@" , identifier ;


(* ===== Lexical ===== *)

qualified_name =
	identifier , { "." , identifier } ;

(* Keywords (if, end, function, match, ...) are reserved and not identifiers. *)
identifier =
	letter , { letter | digit | "_" } ;

integer_literal =
		[ "-" ] , decimal_digits
	| "0b" , binary_digits
	| "0x" , hex_digits
	;

float_literal =
	[ "-" ] , decimal_digits , "." , decimal_digits ;

decimal_digits =
	digit , { digit | "_" } ;

binary_digits =
	binary_digit , { binary_digit | "_" } ;

hex_digits =
	hex_digit , { hex_digit | "_" } ;

string_literal =
	'"' , { string_char } , '"' ;

rune_literal =
	"'" , rune_char , "'" ;

string_char =
		? any character except " or \ ?
	| escape
	;

rune_char =
		? any character except ' or \ ?
	| escape
	;

escape =
	"\" , ( "n" | "t" | "r" | "\" | '"' | "'" | ? other escape ? ) ;

letter =
	"A" | ... | "Z" | "a" | ... | "z" ;

digit =
	"0" | ... | "9" ;

binary_digit =
	"0" | "1" ;

hex_digit =
	digit | "A" | ... | "F" | "a" | ... | "f" ;

comment =
	"#" , { ? any character except newline ? } , ? newline ? ;

(* Comments and whitespace separate tokens; indentation is insignificant. *)
```
