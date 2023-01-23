// use crate::tests::emulation_core::registers;

use super::{constants::*, instruction::*};
use super::{memory::Memory, registers::GpRegisters};

// This struct may never need to use fields
#[derive(Default)]
pub struct TradCore {
    pub place_holder: i32,
}

impl TradCore {
    pub fn execute_instruction(
        &mut self,
        instruction: &Instruction,
        memory: &mut Memory,
        registers: &mut GpRegisters,
        _fpr: &mut [u64; 32],
    ) {
        println!("Print from execute_instruction");
        registers.gpr[3] = 45;

        match instruction {
            Instruction::RType(r) => {
                println!("instruction is an RType");
                match r.funct {
                    FUNCT_ADD => {
                        // rs + rt goes into rd

                        // Should really write code here to detect an
                        // overflow and send an exception

                        registers.gpr[r.rd as usize] =
                            registers.gpr[r.rs as usize].wrapping_add(registers.gpr[r.rt as usize])
                                as i32 as i64 as u64;
                    }
                    FUNCT_SUB => {
                        registers.gpr[r.rd as usize] =
                            registers.gpr[r.rs as usize].wrapping_sub(registers.gpr[r.rt as usize])
                                as i32 as i64 as u64;
                    }
                    FUNCT_SOP30 => {
                        // the mut instruction will be here, among others
                    }
                    _ => {
                        println!("THIS INSTRUCTION IS NOT IMPLIMENTED IN TRAD CORE");
                        println!("{:#?}", r);
                    }
                }
            }
            Instruction::IType(i) => {
                match i.op {
                    OPCODE_LW => {
                        let addr: u64 = (registers.gpr[i.rs as usize] as u64)
                            .wrapping_add(i.immediate as i16 as i64 as u64);
                        registers.gpr[i.rt as usize] = memory.load_word(addr).unwrap_or(0) as u64;
                    }
                    OPCODE_SW => {
                        let addr: u64 = (registers.gpr[i.rs as usize] as u64)
                            .wrapping_add(i.immediate as i16 as i64 as u64);
                        memory.store_word(addr, registers.gpr[i.rt as usize] as u32).expect("Failed store");
                    }
                    _ => {
                        println!("This instruction is unknown");
                        println!("{:#?}", i);
                    }
                }
                println!("Instruction is an IType")
            }
            Instruction::FpuRType(_) => (),
            _ => unimplemented!(),
        }

        // Clean zero register
        registers.gpr[0] = 0;
    }
}
