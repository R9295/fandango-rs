<start> ::= <block> ;

<block> ::= "{ " <statements> "}" ;
<statements> ::= <statement> " " <statements> | "" ;

<statement> ::= <variable_declaration>
    | <assignment>
    | <if_statement>
    | <for_loop>
    | <switch_statement>
    | <void_call>
    | <block> ;

<variable_declaration> ::= "let " <var_decl> ;
<assignment> ::= <var_use> " := " <value_expr> ;
<if_statement> ::= "if " <value_expr> " " <block> ;
<for_loop> ::= "for " <block> " " <value_expr> " " <block> " " <block> ;
<switch_statement> ::= "switch " <value_expr> " " <cases> <default_opt> | "switch " <value_expr> " " <default> ;
<cases> ::= <case> | <case> " " <cases> ;
<default_opt> ::= " " <default> | "" ;
<case> ::= "case " <literal> " " <block> ;
<default> ::= "default " <block> ;

<value_expr> ::= <var_use> | <literal> | <value_call> ;
<varg> ::= <var_use> | <literal> | <value_expr> ;
<value_call> ::= <value_call0> | <value_call1> | <value_call2> | <value_call3> | <value_call4> | <value_call6> | <value_call7> ;
<value_call0> ::= <builtin_value0> "()" ;
<builtin_value0> ::= "msize" | "gas" | "address" | "selfbalance" | "caller" | "callvalue" | "calldatasize" | "codesize" | "returndatasize" | "chainid" | "basefee" | "blobbasefee" | "origin" | "gasprice" | "coinbase" | "timestamp" | "number" | "difficulty" | "prevrandao" | "gaslimit" ;
<value_call1> ::= <builtin_value1> "(" <varg> ")" ;
<builtin_value1> ::= "not" | "iszero" | "clz" | "mload" | "sload" | "tload" | "balance" | "calldataload" | "extcodesize" | "extcodehash" | "blockhash" | "blobhash" ;
<value_call2> ::= <builtin_value2> "(" <varg> ", " <varg> ")" ;
<builtin_value2> ::= "add" | "sub" | "mul" | "div" | "sdiv" | "mod" | "smod" | "exp" | "lt" | "gt" | "slt" | "sgt" | "eq" | "and" | "or" | "xor" | "byte" | "shl" | "shr" | "sar" | "signextend" | "keccak256" ;
<value_call3> ::= <builtin_value3> "(" <varg> ", " <varg> ", " <varg> ")" ;
<builtin_value3> ::= "addmod" | "mulmod" | "create" ;
<value_call4> ::= <builtin_value4> "(" <varg> ", " <varg> ", " <varg> ", " <varg> ")" ;
<builtin_value4> ::= "create2" ;
<value_call6> ::= <builtin_value6> "(" <varg> ", " <varg> ", " <varg> ", " <varg> ", " <varg> ", " <varg> ")" ;
<builtin_value6> ::= "delegatecall" | "staticcall" ;
<value_call7> ::= <builtin_value7> "(" <varg> ", " <varg> ", " <varg> ", " <varg> ", " <varg> ", " <varg> ", " <varg> ")" ;
<builtin_value7> ::= "call" | "callcode" ;

<void_call> ::= <void_call0> | <void_call1> | <void_call2> | <void_call3> | <void_call4> | <void_call5> | <void_call6> ;
<void_call0> ::= <builtin_void0> "()" ;
<builtin_void0> ::= "stop" | "invalid" ;
<void_call1> ::= <builtin_void1> "(" <varg> ")" ;
<builtin_void1> ::= "pop" | "selfdestruct" ;
<void_call2> ::= <builtin_void2> "(" <varg> ", " <varg> ")" ;
<builtin_void2> ::= "mstore" | "mstore8" | "sstore" | "tstore" | "return" | "revert" | "log0" ;
<void_call3> ::= <builtin_void3> "(" <varg> ", " <varg> ", " <varg> ")" ;
<builtin_void3> ::= "calldatacopy" | "codecopy" | "returndatacopy" | "mcopy" | "log1" ;
<void_call4> ::= <builtin_void4> "(" <varg> ", " <varg> ", " <varg> ", " <varg> ")" ;
<builtin_void4> ::= "extcodecopy" | "log2" ;
<void_call5> ::= <builtin_void5> "(" <varg> ", " <varg> ", " <varg> ", " <varg> ", " <varg> ")" ;
<builtin_void5> ::= "log3" ;
<void_call6> ::= <builtin_void6> "(" <varg> ", " <varg> ", " <varg> ", " <varg> ", " <varg> ", " <varg> ")" ;
<builtin_void6> ::= "log4" ;

<var_decl> ::= <identifier> ;
<var_use> ::= <identifier> ;
<identifier> ::= "_" <ident_tail> ;
<ident_tail> ::= <ident_char> <ident_tail> | <ident_char> ;
<ident_char> ::= "a" | "b" | "c" | "d" | "e" | "f" | "g" | "h" | "i" | "j" | "k" | "l" | "m" | "n" | "o" | "p" | "q" | "r" | "s" | "t" | "u" | "v" | "w" | "x" | "y" | "z" | "A" | "B" | "C" | "D" | "E" | "F" | "G" | "H" | "I" | "J" | "K" | "L" | "M" | "N" | "O" | "P" | "Q" | "R" | "S" | "T" | "U" | "V" | "W" | "X" | "Y" | "Z" | "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "_" ;

<literal> ::= <number> | <string_literal> | <true_literal> | <false_literal> ;
<true_literal> ::= "true" ;
<false_literal> ::= "false" ;
<number> ::= <hex_number> | <decimal_number> ;
<hex_number> ::= "0x" <hex_digits> ;
<hex_digits> ::= <hex_digit> <hex_digits> | <hex_digit> ;
<hex_digit> ::= "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "a" | "b" | "c" | "d" | "e" | "f" | "A" | "B" | "C" | "D" | "E" | "F" ;
<decimal_number> ::= <dec_nonzero> <dec_digits> | <dec_digit> ;
<dec_digits> ::= <dec_digit> <dec_digits> | <dec_digit> ;
<dec_nonzero> ::= "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
<dec_digit> ::= "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;

<string_literal> ::= '"' <string_chars> '"' ;
<string_chars> ::= <string_char> <string_chars> | "" ;
<string_char> ::= <string_safe> | "\\" <string_escape> ;
<string_safe> ::= " " | "!" | "#" | "$" | "%" | "&" | "'" | "(" | ")" | "*" | "+" | "," | "-" | "." | "/" | "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | ":" | ";" | "<" | "=" | ">" | "?" | "@" | "A" | "B" | "C" | "D" | "E" | "F" | "G" | "H" | "I" | "J" | "K" | "L" | "M" | "N" | "O" | "P" | "Q" | "R" | "S" | "T" | "U" | "V" | "W" | "X" | "Y" | "Z" | "[" | "]" | "^" | "_" | "`" | "a" | "b" | "c" | "d" | "e" | "f" | "g" | "h" | "i" | "j" | "k" | "l" | "m" | "n" | "o" | "p" | "q" | "r" | "s" | "t" | "u" | "v" | "w" | "x" | "y" | "z" | "{" | "|" | "}" | "~" ;
<string_escape> ::= "\"" | "\\" | "n" | "r" | "t" ;
