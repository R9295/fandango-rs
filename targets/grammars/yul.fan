<start> ::= <block> ;

<block> ::= "{ " <statement_list> "}" ;
<statement_list> ::= <statement> " " <statement_list> | "" ;

<statement> ::= <block>
    | <function_definition>
    | <variable_declaration>
    | <assignment>
    | <if_statement>
    | <expression>
    | <switch_statement>
    | <for_loop>
    | <break_continue>
    | <leave> ;

<function_definition> ::= "function " <identifier> "(" <typed_identifier_list_opt> ")" <return_types_opt> " " <block> ;
<typed_identifier_list_opt> ::= <typed_identifier_list> | "" ;
<return_types_opt> ::= " -> " <typed_identifier_list> | "" ;

<variable_declaration> ::= "let " <typed_identifier_list> <var_init_opt> ;
<var_init_opt> ::= " := " <expression> | "" ;

<assignment> ::= <identifier_list> " := " <expression> ;

<if_statement> ::= "if " <expression> " " <block> ;

<switch_statement> ::= "switch " <expression> " " <switch_body> ;
<switch_body> ::= <cases> <default_opt> | <default> ;
<cases> ::= <case> | <case> " " <cases> ;
<default_opt> ::= " " <default> | "" ;
<case> ::= "case " <literal> " " <block> ;
<default> ::= "default " <block> ;

<for_loop> ::= "for " <block> " " <expression> " " <block> " " <block> ;

<break_continue> ::= "break" | "continue" ;
<leave> ::= "leave" ;

<expression> ::= <function_call> | <builtin_call> | <identifier> | <literal> ;
<arg> ::= <identifier> | <literal> | <expression> ;
<function_call> ::= <identifier> "(" <call_arguments_opt> ")" ;
<call_arguments_opt> ::= <call_arguments> | "" ;
<call_arguments> ::= <arg> | <arg> ", " <call_arguments> ;

<builtin_call> ::= <builtin_0> | <builtin_1> | <builtin_2> | <builtin_3> | <builtin_4> | <builtin_5> | <builtin_6> | <builtin_7> ;
<builtin_0> ::= <builtin_0_name> "()" ;
<builtin_0_name> ::= "stop" | "msize" | "gas" | "address" | "selfbalance" | "caller" | "callvalue" | "calldatasize" | "codesize" | "returndatasize" | "invalid" | "chainid" | "basefee" | "blobbasefee" | "origin" | "gasprice" | "coinbase" | "timestamp" | "number" | "difficulty" | "prevrandao" | "gaslimit" ;
<builtin_1> ::= <builtin_1_name> "(" <arg> ")" ;
<builtin_1_name> ::= "not" | "iszero" | "clz" | "pop" | "mload" | "sload" | "tload" | "balance" | "calldataload" | "extcodesize" | "extcodehash" | "selfdestruct" | "blockhash" | "blobhash" ;
<builtin_2> ::= <builtin_2_name> "(" <arg> ", " <arg> ")" ;
<builtin_2_name> ::= "add" | "sub" | "mul" | "div" | "sdiv" | "mod" | "smod" | "exp" | "lt" | "gt" | "slt" | "sgt" | "eq" | "and" | "or" | "xor" | "byte" | "shl" | "shr" | "sar" | "signextend" | "keccak256" | "mstore" | "mstore8" | "sstore" | "tstore" | "return" | "revert" | "log0" ;
<builtin_3> ::= <builtin_3_name> "(" <arg> ", " <arg> ", " <arg> ")" ;
<builtin_3_name> ::= "addmod" | "mulmod" | "calldatacopy" | "codecopy" | "returndatacopy" | "mcopy" | "create" | "log1" ;
<builtin_4> ::= <builtin_4_name> "(" <arg> ", " <arg> ", " <arg> ", " <arg> ")" ;
<builtin_4_name> ::= "extcodecopy" | "create2" | "log2" ;
<builtin_5> ::= <builtin_5_name> "(" <arg> ", " <arg> ", " <arg> ", " <arg> ", " <arg> ")" ;
<builtin_5_name> ::= "log3" ;
<builtin_6> ::= <builtin_6_name> "(" <arg> ", " <arg> ", " <arg> ", " <arg> ", " <arg> ", " <arg> ")" ;
<builtin_6_name> ::= "delegatecall" | "staticcall" | "log4" ;
<builtin_7> ::= <builtin_7_name> "(" <arg> ", " <arg> ", " <arg> ", " <arg> ", " <arg> ", " <arg> ", " <arg> ")" ;
<builtin_7_name> ::= "call" | "callcode" ;

<identifier> ::= <ident_start> <ident_continue> | <ident_start> ;
<ident_continue> ::= <ident_char> <ident_continue> | <ident_char> ;
<ident_start> ::= "a" | "b" | "c" | "d" | "e" | "f" | "g" | "h" | "i" | "j" | "k" | "l" | "m" | "n" | "o" | "p" | "q" | "r" | "s" | "t" | "u" | "v" | "w" | "x" | "y" | "z" | "A" | "B" | "C" | "D" | "E" | "F" | "G" | "H" | "I" | "J" | "K" | "L" | "M" | "N" | "O" | "P" | "Q" | "R" | "S" | "T" | "U" | "V" | "W" | "X" | "Y" | "Z" | "_" | "$" ;
<ident_char> ::= <ident_start> | <dec_digit> | "." ;
<identifier_list> ::= <identifier> | <identifier> ", " <identifier_list> ;
<type_name> ::= <identifier> ;

<typed_identifier_list> ::= <typed_identifier> | <typed_identifier> ", " <typed_identifier_list> ;
<typed_identifier> ::= <identifier> <type_annotation_opt> ;
<type_annotation_opt> ::= ":" <type_name> | "" ;

<literal> ::= <literal_value> <type_annotation_opt> ;
<literal_value> ::= <number_literal> | <string_literal> | <true_literal> | <false_literal> ;
<number_literal> ::= <hex_number> | <decimal_number> ;
<true_literal> ::= "true" ;
<false_literal> ::= "false" ;

<hex_number> ::= "0x" <hex_digits> ;
<hex_digits> ::= <hex_digit> <hex_digits> | <hex_digit> ;
<hex_digit> ::= "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "a" | "b" | "c" | "d" | "e" | "f" | "A" | "B" | "C" | "D" | "E" | "F" ;

<decimal_number> ::= <dec_digits> ;
<dec_digits> ::= <dec_digit> <dec_digits> | <dec_digit> ;
<dec_digit> ::= "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;

<string_literal> ::= '"' <string_chars> '"' ;
<string_chars> ::= <string_char> <string_chars> | "" ;
<string_char> ::= <string_safe> | "\\" <string_escape> ;
<string_safe> ::= " " | "!" | "#" | "$" | "%" | "&" | "'" | "(" | ")" | "*" | "+" | "," | "-" | "." | "/" | "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | ":" | ";" | "<" | "=" | ">" | "?" | "@" | "A" | "B" | "C" | "D" | "E" | "F" | "G" | "H" | "I" | "J" | "K" | "L" | "M" | "N" | "O" | "P" | "Q" | "R" | "S" | "T" | "U" | "V" | "W" | "X" | "Y" | "Z" | "[" | "]" | "^" | "_" | "`" | "a" | "b" | "c" | "d" | "e" | "f" | "g" | "h" | "i" | "j" | "k" | "l" | "m" | "n" | "o" | "p" | "q" | "r" | "s" | "t" | "u" | "v" | "w" | "x" | "y" | "z" | "{" | "|" | "}" | "~" ;
<string_escape> ::= "\"" | "\\" | "n" | "r" | "t" ;
