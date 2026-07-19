<start> ::= <bytecode> ;
<bytecode> ::= <op> | <op> <bytecode> ;

<op> ::= <arithmetic_op>
    | <comparison_op>
    | <bitwise_op>
    | <keccak_op>
    | <environment_op>
    | <block_op>
    | <control_op>
    | <stack_op>
    | <memory_op>
    | <storage_op>
    | <push_op>
    | <dup_op>
    | <swap_op>
    | <log_op>
    | <system_op>
    | <precompile_call> ;

<arithmetic_op> ::= <stop> | <add> | <mul> | <sub> | <div> | <sdiv> | <mod> | <smod> | <addmod> | <mulmod> | <exp> | <signextend> ;
<stop> ::= b"\x00" ;
<add> ::= b"\x01" ;
<mul> ::= b"\x02" ;
<sub> ::= b"\x03" ;
<div> ::= b"\x04" ;
<sdiv> ::= b"\x05" ;
<mod> ::= b"\x06" ;
<smod> ::= b"\x07" ;
<addmod> ::= b"\x08" ;
<mulmod> ::= b"\x09" ;
<exp> ::= b"\x0a" ;
<signextend> ::= b"\x0b" ;

<comparison_op> ::= <lt> | <gt> | <slt> | <sgt> | <eq> | <iszero> ;
<lt> ::= b"\x10" ;
<gt> ::= b"\x11" ;
<slt> ::= b"\x12" ;
<sgt> ::= b"\x13" ;
<eq> ::= b"\x14" ;
<iszero> ::= b"\x15" ;

<bitwise_op> ::= <and> | <or> | <xor> | <not> | <byte> | <shl> | <shr> | <sar> ;
<and> ::= b"\x16" ;
<or> ::= b"\x17" ;
<xor> ::= b"\x18" ;
<not> ::= b"\x19" ;
<byte> ::= b"\x1a" ;
<shl> ::= b"\x1b" ;
<shr> ::= b"\x1c" ;
<sar> ::= b"\x1d" ;

<keccak_op> ::= <keccak256> ;
<keccak256> ::= b"\x20" ;

<environment_op> ::= <address> | <balance> | <origin> | <caller> | <callvalue> | <calldataload> | <calldatasize> | <calldatacopy> | <codesize> | <codecopy> | <gasprice> | <extcodesize> | <extcodecopy> | <returndatasize> | <returndatacopy> | <extcodehash> ;
<address> ::= b"\x30" ;
<balance> ::= b"\x31" ;
<origin> ::= b"\x32" ;
<caller> ::= b"\x33" ;
<callvalue> ::= b"\x34" ;
<calldataload> ::= b"\x35" ;
<calldatasize> ::= b"\x36" ;
<calldatacopy> ::= b"\x37" ;
<codesize> ::= b"\x38" ;
<codecopy> ::= b"\x39" ;
<gasprice> ::= b"\x3a" ;
<extcodesize> ::= b"\x3b" ;
<extcodecopy> ::= b"\x3c" ;
<returndatasize> ::= b"\x3d" ;
<returndatacopy> ::= b"\x3e" ;
<extcodehash> ::= b"\x3f" ;

<block_op> ::= <blockhash> | <coinbase> | <timestamp> | <number> | <prevrandao> | <gaslimit> | <chainid> | <selfbalance> | <basefee> | <blobhash> | <blobbasefee> ;
<blockhash> ::= b"\x40" ;
<coinbase> ::= b"\x41" ;
<timestamp> ::= b"\x42" ;
<number> ::= b"\x43" ;
<prevrandao> ::= b"\x44" ;
<gaslimit> ::= b"\x45" ;
<chainid> ::= b"\x46" ;
<selfbalance> ::= b"\x47" ;
<basefee> ::= b"\x48" ;
<blobhash> ::= b"\x49" ;
<blobbasefee> ::= b"\x4a" ;

<control_op> ::= <jump> | <jumpi> | <pc> | <jumpdest> ;
<jump> ::= b"\x56" ;
<jumpi> ::= b"\x57" ;
<pc> ::= b"\x58" ;
<jumpdest> ::= b"\x5b" ;

<stack_op> ::= <pop> | <msize> | <gas> ;
<pop> ::= b"\x50" ;
<msize> ::= b"\x59" ;
<gas> ::= b"\x5a" ;

<memory_op> ::= <mload> | <mstore> | <mstore8> | <mcopy> ;
<mload> ::= b"\x51" ;
<mstore> ::= b"\x52" ;
<mstore8> ::= b"\x53" ;
<mcopy> ::= b"\x5e" ;

<storage_op> ::= <sload> | <sstore> | <tload> | <tstore> ;
<sload> ::= b"\x54" ;
<sstore> ::= b"\x55" ;
<tload> ::= b"\x5c" ;
<tstore> ::= b"\x5d" ;

<push_op> ::= <push0> | <push1> | <push2> | <push3> | <push4> | <push5> | <push6> | <push7> | <push8> | <push9> | <push10> | <push11> | <push12> | <push13> | <push14> | <push15> | <push16> | <push17> | <push18> | <push19> | <push20> | <push21> | <push22> | <push23> | <push24> | <push25> | <push26> | <push27> | <push28> | <push29> | <push30> | <push31> | <push32> ;
<push0> ::= b"\x5f" ;
<push1> ::= b"\x60" <data_byte> ;
<push2> ::= b"\x61" <data_byte> <data_byte> ;
<push3> ::= b"\x62" <data_byte> <data_byte> <data_byte> ;
<push4> ::= b"\x63" <data_byte> <data_byte> <data_byte> <data_byte> ;
<push5> ::= b"\x64" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push6> ::= b"\x65" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push7> ::= b"\x66" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push8> ::= b"\x67" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push9> ::= b"\x68" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push10> ::= b"\x69" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push11> ::= b"\x6a" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push12> ::= b"\x6b" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push13> ::= b"\x6c" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push14> ::= b"\x6d" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push15> ::= b"\x6e" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push16> ::= b"\x6f" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push17> ::= b"\x70" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push18> ::= b"\x71" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push19> ::= b"\x72" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push20> ::= b"\x73" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push21> ::= b"\x74" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push22> ::= b"\x75" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push23> ::= b"\x76" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push24> ::= b"\x77" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push25> ::= b"\x78" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push26> ::= b"\x79" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push27> ::= b"\x7a" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push28> ::= b"\x7b" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push29> ::= b"\x7c" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push30> ::= b"\x7d" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push31> ::= b"\x7e" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;
<push32> ::= b"\x7f" <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> <data_byte> ;

<dup_op> ::= <dup1> | <dup2> | <dup3> | <dup4> | <dup5> | <dup6> | <dup7> | <dup8> | <dup9> | <dup10> | <dup11> | <dup12> | <dup13> | <dup14> | <dup15> | <dup16> ;
<dup1> ::= b"\x80" ;
<dup2> ::= b"\x81" ;
<dup3> ::= b"\x82" ;
<dup4> ::= b"\x83" ;
<dup5> ::= b"\x84" ;
<dup6> ::= b"\x85" ;
<dup7> ::= b"\x86" ;
<dup8> ::= b"\x87" ;
<dup9> ::= b"\x88" ;
<dup10> ::= b"\x89" ;
<dup11> ::= b"\x8a" ;
<dup12> ::= b"\x8b" ;
<dup13> ::= b"\x8c" ;
<dup14> ::= b"\x8d" ;
<dup15> ::= b"\x8e" ;
<dup16> ::= b"\x8f" ;

<swap_op> ::= <swap1> | <swap2> | <swap3> | <swap4> | <swap5> | <swap6> | <swap7> | <swap8> | <swap9> | <swap10> | <swap11> | <swap12> | <swap13> | <swap14> | <swap15> | <swap16> ;
<swap1> ::= b"\x90" ;
<swap2> ::= b"\x91" ;
<swap3> ::= b"\x92" ;
<swap4> ::= b"\x93" ;
<swap5> ::= b"\x94" ;
<swap6> ::= b"\x95" ;
<swap7> ::= b"\x96" ;
<swap8> ::= b"\x97" ;
<swap9> ::= b"\x98" ;
<swap10> ::= b"\x99" ;
<swap11> ::= b"\x9a" ;
<swap12> ::= b"\x9b" ;
<swap13> ::= b"\x9c" ;
<swap14> ::= b"\x9d" ;
<swap15> ::= b"\x9e" ;
<swap16> ::= b"\x9f" ;

<log_op> ::= <log0> | <log1> | <log2> | <log3> | <log4> ;
<log0> ::= b"\xa0" ;
<log1> ::= b"\xa1" ;
<log2> ::= b"\xa2" ;
<log3> ::= b"\xa3" ;
<log4> ::= b"\xa4" ;

<system_op> ::= <create> | <call> | <callcode> | <return> | <delegatecall> | <create2> | <staticcall> | <revert> | <invalid> | <selfdestruct> ;
<create> ::= b"\xf0" ;
<call> ::= b"\xf1" ;
<callcode> ::= b"\xf2" ;
<return> ::= b"\xf3" ;
<delegatecall> ::= b"\xf4" ;
<create2> ::= b"\xf5" ;
<staticcall> ::= b"\xfa" ;
<revert> ::= b"\xfd" ;
<invalid> ::= b"\xfe" ;
<selfdestruct> ::= b"\xff" ;

<precompile_call> ::= <precompile_value_call> | <precompile_static_call> ;
<precompile_value_call> ::= <push1_any> <push1_any> <push1_any> <push1_any> <push1_any> <precompile_addr> <gas_push> <value_call_op> ;
<precompile_static_call> ::= <push1_any> <push1_any> <push1_any> <push1_any> <precompile_addr> <gas_push> <static_call_op> ;
<value_call_op> ::= b"\xf1" | b"\xf2" ;
<static_call_op> ::= b"\xfa" | b"\xf4" ;
<push1_any> ::= b"\x60" <data_byte> ;
<precompile_addr> ::= b"\x60" <precompile_id> ;
<precompile_id> ::= b"\x01" | b"\x02" | b"\x03" | b"\x04" | b"\x05" | b"\x06" | b"\x07" | b"\x08" | b"\x09" | b"\x0a" ;
<gas_push> ::= b"\x5a" | b"\x61" <data_byte> <data_byte> ;

<data_byte> ::= 
b"\x00" | b"\x01" | b"\x02" | b"\x03" | b"\x04" | b"\x05" | b"\x06" | b"\x07"
    | b"\x08" | b"\x09" | b"\x0a" | b"\x0b" | b"\x0c" | b"\x0d" | b"\x0e" | b"\x0f"
    | b"\x10" | b"\x11" | b"\x12" | b"\x13" | b"\x14" | b"\x15" | b"\x16" | b"\x17"
    | b"\x18" | b"\x19" | b"\x1a" | b"\x1b" | b"\x1c" | b"\x1d" | b"\x1e" | b"\x1f"
    | b"\x20" | b"\x21" | b"\x22" | b"\x23" | b"\x24" | b"\x25" | b"\x26" | b"\x27"
    | b"\x28" | b"\x29" | b"\x2a" | b"\x2b" | b"\x2c" | b"\x2d" | b"\x2e" | b"\x2f"
    | b"\x30" | b"\x31" | b"\x32" | b"\x33" | b"\x34" | b"\x35" | b"\x36" | b"\x37"
    | b"\x38" | b"\x39" | b"\x3a" | b"\x3b" | b"\x3c" | b"\x3d" | b"\x3e" | b"\x3f"
    | b"\x40" | b"\x41" | b"\x42" | b"\x43" | b"\x44" | b"\x45" | b"\x46" | b"\x47"
    | b"\x48" | b"\x49" | b"\x4a" | b"\x4b" | b"\x4c" | b"\x4d" | b"\x4e" | b"\x4f"
    | b"\x50" | b"\x51" | b"\x52" | b"\x53" | b"\x54" | b"\x55" | b"\x56" | b"\x57"
    | b"\x58" | b"\x59" | b"\x5a" | b"\x5b" | b"\x5c" | b"\x5d" | b"\x5e" | b"\x5f"
    | b"\x60" | b"\x61" | b"\x62" | b"\x63" | b"\x64" | b"\x65" | b"\x66" | b"\x67"
    | b"\x68" | b"\x69" | b"\x6a" | b"\x6b" | b"\x6c" | b"\x6d" | b"\x6e" | b"\x6f"
    | b"\x70" | b"\x71" | b"\x72" | b"\x73" | b"\x74" | b"\x75" | b"\x76" | b"\x77"
    | b"\x78" | b"\x79" | b"\x7a" | b"\x7b" | b"\x7c" | b"\x7d" | b"\x7e" | b"\x7f"
    | b"\x80" | b"\x81" | b"\x82" | b"\x83" | b"\x84" | b"\x85" | b"\x86" | b"\x87"
    | b"\x88" | b"\x89" | b"\x8a" | b"\x8b" | b"\x8c" | b"\x8d" | b"\x8e" | b"\x8f"
    | b"\x90" | b"\x91" | b"\x92" | b"\x93" | b"\x94" | b"\x95" | b"\x96" | b"\x97"
    | b"\x98" | b"\x99" | b"\x9a" | b"\x9b" | b"\x9c" | b"\x9d" | b"\x9e" | b"\x9f"
    | b"\xa0" | b"\xa1" | b"\xa2" | b"\xa3" | b"\xa4" | b"\xa5" | b"\xa6" | b"\xa7"
    | b"\xa8" | b"\xa9" | b"\xaa" | b"\xab" | b"\xac" | b"\xad" | b"\xae" | b"\xaf"
    | b"\xb0" | b"\xb1" | b"\xb2" | b"\xb3" | b"\xb4" | b"\xb5" | b"\xb6" | b"\xb7"
    | b"\xb8" | b"\xb9" | b"\xba" | b"\xbb" | b"\xbc" | b"\xbd" | b"\xbe" | b"\xbf"
    | b"\xc0" | b"\xc1" | b"\xc2" | b"\xc3" | b"\xc4" | b"\xc5" | b"\xc6" | b"\xc7"
    | b"\xc8" | b"\xc9" | b"\xca" | b"\xcb" | b"\xcc" | b"\xcd" | b"\xce" | b"\xcf"
    | b"\xd0" | b"\xd1" | b"\xd2" | b"\xd3" | b"\xd4" | b"\xd5" | b"\xd6" | b"\xd7"
    | b"\xd8" | b"\xd9" | b"\xda" | b"\xdb" | b"\xdc" | b"\xdd" | b"\xde" | b"\xdf"
    | b"\xe0" | b"\xe1" | b"\xe2" | b"\xe3" | b"\xe4" | b"\xe5" | b"\xe6" | b"\xe7"
    | b"\xe8" | b"\xe9" | b"\xea" | b"\xeb" | b"\xec" | b"\xed" | b"\xee" | b"\xef"
    | b"\xf0" | b"\xf1" | b"\xf2" | b"\xf3" | b"\xf4" | b"\xf5" | b"\xf6" | b"\xf7"
    | b"\xf8" | b"\xf9" | b"\xfa" | b"\xfb" | b"\xfc" | b"\xfd" | b"\xfe" | b"\xff" ;
