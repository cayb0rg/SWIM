// use crate::tests::emulation_core::registers;

use crate::emulation_core::mips::constants;

use super::super::datapath::Datapath;
use super::control_signals::{floating_point::*, *};
use super::instruction::*;
use super::{constants::*, instruction};
use super::{coprocessor::MipsFpCoprocessor, memory::Memory, registers::GpRegisters};

// This struct may never need to use fields
#[derive(Default)]
pub struct TradCore {
    pub place_holder: i32,
}

impl TradCore {
    pub fn execute_instruction(
        &mut self,
        instruction: &Instruction,
        _memory: &mut Memory,
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
                        println!("The funct code is ADD");
                        println!("{:#?}", r);
                    }
                    FUNCT_SUB => {
                        println!("The funct code is SUB");
                        println!("{:#?}", r);
                    }
                    _ => {
                        println!("bla bla bla");
                        println!("{:#?}", r);
                    }
                }
            }
            Instruction::IType(i) => {
                println!("Instruction is an IType")
            }
            Instruction::FpuRType(_) => (),
            _ => unimplemented!(),
        }
    }
}
