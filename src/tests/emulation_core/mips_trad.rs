#![allow(clippy::unusual_byte_groupings)]

use crate::emulation_core::datapath::Datapath;
use crate::emulation_core::mips::datapath::CoreSelect;
use crate::emulation_core::mips::datapath::MipsDatapath;
use crate::emulation_core::mips::registers::GpRegisterType;



#[test]
fn add_register_to_itself_trad() {
    let mut datapath = MipsDatapath::default();

    // $t1 = $t1 + $t1
    //                       R-type  t1    t1    t1  (shamt)  ADD
    let instruction: u32 = 0b000000_01001_01001_01001_00000_100000;
    datapath
        .memory
        .store_word(0, instruction)
        .expect("Failed to store instruction.");

    // Assume the register $t1 has the value 5.
    datapath.registers[GpRegisterType::T1] = 5;

    datapath.execute_instruction_select(CoreSelect::DatapathCore);

    // After the operation is finished, the register should be 10.
    assert_eq!(datapath.registers[GpRegisterType::T1], 10);
}