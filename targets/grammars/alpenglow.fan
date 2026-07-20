<start> ::= <scenario> ;

<scenario> ::= "alpenglow {\n"
    "  blocks: [b1, b2]\n"
    "  validators: 10\n"
    "  stake-per-validator: 10%\n"
    "  byzantine-stake: 20%\n"
    "  potentially-absent-stake: 20%\n"
    <byzantine_validators>
    <potentially_absent_validators>
    <honest_validators>
    "}\n" ;

<byzantine_validators> ::= <byzantine_v0> <byzantine_v1> ;
<byzantine_v0> ::= "  v0 { stake: 10%, role: byzantine, votes: [" <vote_list> "] }\n" ;
<byzantine_v1> ::= "  v1 { stake: 10%, role: byzantine, votes: [" <vote_list> "] }\n" ;

<potentially_absent_validators> ::= <potentially_absent_v2> <potentially_absent_v3> ;
<potentially_absent_v2> ::= "  v2 { stake: 10%, role: potentially-absent, votes: [" <optional_vote_list> "] }\n" ;
<potentially_absent_v3> ::= "  v3 { stake: 10%, role: potentially-absent, votes: [" <optional_vote_list> "] }\n" ;

<honest_validators> ::= <honest_v4> <honest_v5> <honest_v6> <honest_v7> <honest_v8> <honest_v9> ;
<honest_v4> ::= "  v4 { stake: 10%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v5> ::= "  v5 { stake: 10%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v6> ::= "  v6 { stake: 10%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v7> ::= "  v7 { stake: 10%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v8> ::= "  v8 { stake: 10%, role: honest, votes: [" <vote_list> "] }\n" ;
<honest_v9> ::= "  v9 { stake: 10%, role: honest, votes: [" <vote_list> "] }\n" ;

<optional_vote_list> ::= "" | <vote_list> ;
<vote_list> ::= <vote> | <vote> ", " <vote_list> ;

<vote> ::= <skip> | <notarize> | <notarize_fallback> | <skip_fallback> ;
<skip> ::= "skip" ;
<notarize> ::= "notarize(" <block> ")" ;
<notarize_fallback> ::= "notarize-fallback(" <block> ")" ;
<skip_fallback> ::= "skip-fallback" ;
<block> ::= "b1" | "b2" ;
