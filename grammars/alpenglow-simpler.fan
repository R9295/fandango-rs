<start> ::= <committee> <stmt> ;
<stmt> ::= <block>+ ;
<block> ::= <block_catalog> <slot> ;

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

<committee> ::= "committee:\n" <validator_line>+ ;
<validator_line> ::= "  " <validator_name> " stake=1% role=" <role> "\n" ;
<validator_name> ::= "v" <validator_number> ;
<validator_number> ::= "100" | <nonzero_digit> <digit> | <digit> ;
<digit> ::= "0" | <nonzero_digit> ;
<nonzero_digit> ::= "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
<role> ::= "byzantine" | "absent" | "honest" ;

<slot> ::= "slot\n"
    "  round1:" <round1_assignment>+ "\n"
    "  round2: v1=" <round2_vote> "\n"
    "end-slot\n" ;
<round1_assignment> ::= " " <validator_name> "=" <vote_set> ;
<vote_set> ::= "absent" | <vote_target> ("," <vote_target>)* ;
<vote_target> ::= <fallback_vote> | <block_reference> | <skip_vote> ;
<block_reference> ::= "b1" | "b2" | "b3" | "b4" ;
<skip_vote> ::= "skip" ;
<round2_vote> ::= <round2_vote_target> ("," <round2_vote_target>)* ;
<round2_vote_target> ::= <notarize_vote> | "finalize" | "none" | <fallback_vote> ;
<notarize_vote> ::= "notarize(b1)" | "notarize(b2)" | "notarize(b3)" | "notarize(b4)" ;
<fallback_vote> ::= "notar-fallback(b1)" | "notar-fallback(b2)"
    | "notar-fallback(b3)" | "notar-fallback(b4)" | "skip-fallback" ;
