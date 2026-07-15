<start> ::= <committee> <stmt> ;
<stmt> ::= <slot>+ ;

<committee> ::= "committee:\n" <validator_line>+ ;
<validator_line> ::= "  " <validator_name> " stake=" <stake> "\n" ;
<validator_name> ::= "v" <validator_number> ;
<validator_number> ::= "100" | <nonzero_digit> <digit> | <digit> ;
<stake> ::= <digit>+ ;
<digit> ::= "0" | <nonzero_digit> ;
<nonzero_digit> ::= "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;

<slot> ::= <block_catalog>
    "slot\n"
    "  round1:" <round_assignment>+ "\n"
    "  round2:" <round_assignment>+ "\n"
    "end-slot\n" ;

<block_catalog> ::= "blocks:\n"
    "  b1=" <block_id_1> "\n"
    "  b2=" <block_id_2> "\n"
    "  b3=" <block_id_3> "\n"
    "  b4=" <block_id_4> "\n" ;
<block_id_1> ::= <block_hash> ;
<block_id_2> ::= <block_hash> ;
<block_id_3> ::= <block_hash> ;
<block_id_4> ::= <block_hash> ;
<block_hash> ::= <hex_digit>+ ;
<hex_digit> ::= "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7"
    | "8" | "9" | "a" | "b" | "c" | "d" | "e" | "f" ;

<round_assignment> ::= " " <validator_name> "=" <vote> ;
<vote> ::= "skip-fallback" | "skip" | "finalize" | <notarize_vote>
    | <notarize_fallback_vote> | "absent" ;
<notarize_vote> ::= "notarize(" <block_reference> ")" ;
<notarize_fallback_vote> ::= "notarizefallback(" <block_reference> ")" ;
<block_reference> ::= "b1" | "b2" | "b3" | "b4" ;
