//! Implementation of a MIPS64 datapath.
//!
//! It is assumed that while moving through stages, only one
//! instruction will be active any any given point in time. Due to this,
//! we consider the datapath to be a "pseudo-single-cycle datapath."
//!
//! For the most part, this datapath is an implementation of MIPS64 Version 6.
//! (See below for exceptions.)
//!
//! # Differences Compared to MIPS64 Version 6
//!
//! It should be noted that this datapath chooses to diverge from the MIPS64
//! version 6 specification for the sake of simplicity in a few places:
//!
//! - There is no exception handling, including that for integer overflow. (See
//!   [`MipsDatapath::alu()`].)
//! - 32-bit instructions are treated exclusively with 32 bits, and the upper 32
//!   bits stored in a register are completely ignored in any of these cases. For
//!   example, before an `add` instruction, it should be checked whether it is a
//!   sign-extended 32-bit value stored in a 64-bit register. Instead, the upper
//!   32 bits are ignored when being used for 32-bit instructions.
//! - Instead of implementing the `cmp.condn.fmt` instructions, this datapath implements
//!   the `c.cond.fmt` instructions from MIPS64 version 5.
//! - This datapath implements the `addi` instruction as it exists in MIPS64 version 5.
//!   This instruction was deprecated in MIPS64 version 6 to allow for the `beqzalc`,
//!   `bnezalc`, `beqc`, and `bovc` instructions.

use super::super::datapath::Datapath;
use super::constants::*;
use super::control_signals::{floating_point::*, *};
use super::instruction::*;
use super::traditional_core::TradCore;
use super::{coprocessor::MipsFpCoprocessor, memory::Memory, registers::GpRegisters};

/// An implementation of a datapath for the MIPS64 ISA.
#[derive(Default)]
pub struct MipsDatapath {
    pub registers: GpRegisters,
    pub memory: Memory,
    pub coprocessor: MipsFpCoprocessor,

    pub instruction: Instruction,
    pub signals: ControlSignals,
    pub state: DatapathState,

    /// The currently-active stage in the datapath.
    current_stage: Stage,

    // traditional core object, SHOULD FIND A BETTER PLACE
    // TO PUT THIS!
    traditional_core: TradCore,
}

/// A collection of all the data lines and wires in the datapath.
#[derive(Default)]
pub struct DatapathState {
    /// *Data line.* The currently loaded instruction. Initialized after the
    /// Instruction Fetch stage.
    instruction: u32,
    rs: u32,
    rt: u32,
    rd: u32,
    shamt: u32,
    funct: u32,
    imm: u32,

    /// *Data line.* Data read from the register file based on the `rs`
    /// field of the instruction. Initialized after the Instruction
    /// Decode stage.
    read_data_1: u64,

    /// *Data line.* Data read from the register file based on the `rt`
    /// field of the instruction. Initialized after the Instruction
    /// Decode stage.
    read_data_2: u64,

    /// *Data line.* The instruction's immediate value sign-extended to
    /// 64 bits. Initialized after the Instruction Decode stage.
    sign_extend: u64,

    /// *Data line.* The final result as provided by the ALU.
    /// Initialized after the Execute stage.
    alu_result: u64,

    /// *Data line.* The data retrieved from memory. Initialized after
    /// the Memory stage.
    memory_data: u64,

    /// *Data line.* The data after the `MemToReg` multiplexer, but
    /// before the `DataWrite` multiplexer in the main processor.
    data_result: u64,

    /// *Data line.* The data after the `DataWrite` multiplexer in the main
    /// processor and the main processor register file.
    register_write_data: u64,
}

/// The possible stages the datapath could be in during execution.
#[derive(Default, Copy, Clone, Eq, PartialEq)]
enum Stage {
    #[default]
    InstructionFetch,
    InstructionDecode,
    Execute,
    Memory,
    WriteBack,
}

#[derive(Default, Copy, Clone, Eq, PartialEq)]
pub enum CoreSelect {
    #[default]
    DatapathCore,
    TradCore,
}

impl Stage {
    /// Given a stage, return the next consecutive stage. If the last
    /// stage is given, return the first stage.
    fn get_next_stage(current_stage: Stage) -> Stage {
        match current_stage {
            Stage::InstructionFetch => Stage::InstructionDecode,
            Stage::InstructionDecode => Stage::Execute,
            Stage::Execute => Stage::Memory,
            Stage::Memory => Stage::WriteBack,
            Stage::WriteBack => Stage::InstructionFetch,
        }
    }
}

/// Handle an otherwise irrecoverable error within the datapath. At
/// present, this is the equivalent of `panic!()`.
pub fn error(message: &str) {
    panic!("{}", message);
}

impl Datapath for MipsDatapath {
    type RegisterData = u64;
    type RegisterEnum = super::registers::GpRegisterType;
    type MemoryType = Memory;

    fn execute_instruction_select(&mut self, core_preference: CoreSelect) {
        match core_preference {
            CoreSelect::TradCore => {
                // There should be some kind of signal somehow returned to the
                // frontend to signal which emulation core was used. I am putting
                // off the design of how the signal will work for now ~Kevin
                println!("Bob the builder is great");

                // self.finish_instruction(); should maybe be called here...

                self.instruction_fetch();
                self.instruction = Instruction::from(self.state.instruction);
                self.traditional_core.execute_instruction(
                    &self.instruction,
                    &mut self.memory,
                    &mut self.registers,
                    &mut self.coprocessor.fpr,
                );
            }
            CoreSelect::DatapathCore => {
                self.execute_instruction();
            }
        }
    }

    // Default instruction to execute an instruction.
    // By default an instruction will be run in the datapath core if possible
    fn execute_instruction(&mut self) {
        // If the last instruction has not finished, finish it instead.
        if self.current_stage != Stage::InstructionFetch {
            self.finish_instruction();
            return;
        }

        // IF
        self.stage_instruction_fetch();

        // ID
        self.stage_instruction_decode();

        // EX
        self.stage_execute();

        // MEM
        self.stage_memory();

        // WB
        self.stage_writeback();
    }

    fn execute_stage(&mut self) {
        match self.current_stage {
            Stage::InstructionFetch => self.stage_instruction_fetch(),
            Stage::InstructionDecode => self.stage_instruction_decode(),
            Stage::Execute => self.stage_execute(),
            Stage::Memory => self.stage_memory(),
            Stage::WriteBack => self.stage_writeback(),
        }

        self.current_stage = Stage::get_next_stage(self.current_stage);
    }

    fn get_register_by_enum(&self, register: Self::RegisterEnum) -> u64 {
        self.registers[register]
    }

    fn get_memory(&self) -> &Self::MemoryType {
        &self.memory
    }
}

impl MipsDatapath {
    /// Load a vector of 32-bit instructions into memory. If the process fails,
    /// from a lack of space or otherwise, an Err is returned.
    pub fn load_instructions(&mut self, instructions: Vec<u32>) -> Result<(), String> {
        for (i, data) in instructions.iter().enumerate() {
            self.memory.store_word((i as u64) + 4, *data)?
        }

        Ok(())
    }

    /// Finish the current instruction within the datapath. If the
    /// current stage is the first stage, do nothing as there is
    /// nothing to finish, only to start. (Use [`execute_instruction()`][MipsDatapath::execute_instruction()]
    /// in this case.)
    fn finish_instruction(&mut self) {
        while self.current_stage != Stage::InstructionFetch {
            self.execute_stage();
        }
    }

    /// Stage 1 of 5: Instruction Fetch (IF)
    ///
    /// Fetch the current instruction based on the given PC and load it
    /// into the datapath.
    fn stage_instruction_fetch(&mut self) {
        self.instruction_fetch();
        self.coprocessor.set_instruction(self.state.instruction);
    }

    /// Stage 2 of 5: Instruction Decode (ID)
    ///
    /// Parse the instruction, set control signals, and read registers.
    fn stage_instruction_decode(&mut self) {
        self.instruction_decode();
        self.sign_extend();
        self.set_control_signals();
        self.read_registers();
        self.set_alu_control();
        self.coprocessor.stage_instruction_decode();
        self.coprocessor
            .set_data_from_main_processor(self.state.read_data_2);
    }

    /// Stage 3 of 5: Execute (EX)
    ///
    /// Execute the current instruction with some arithmetic operation.
    fn stage_execute(&mut self) {
        self.alu();
        self.coprocessor.stage_execute();
    }

    /// Stage 4 of 5: Memory (MEM)
    ///
    /// Read or write to memory.
    fn stage_memory(&mut self) {
        if let MemRead::YesRead = self.signals.mem_read {
            self.memory_read();
        }

        if let MemWrite::YesWrite = self.signals.mem_write {
            self.memory_write();
        }

        // Determine what data will be sent to the registers: either
        // the result from the ALU, or data retrieved from memory.
        self.state.data_result = match self.signals.mem_to_reg {
            MemToReg::UseAlu => self.state.alu_result,
            MemToReg::UseMemory => self.state.memory_data,
        };

        self.coprocessor.stage_memory();
    }

    /// Stage 5 of 5: Writeback (WB)
    ///
    /// Write the result of the instruction's operation to a register,
    /// if desired. Additionally, set the PC for the next instruction.
    fn stage_writeback(&mut self) {
        self.coprocessor
            .set_fp_register_data_from_main_processor(self.state.data_result);
        self.register_write();
        self.set_pc();
        self.coprocessor.stage_writeback();
    }

    /// Load the raw binary instruction from memory and into the
    /// datapath. If there is an error with loading the word, assume
    /// the instruction to be bitwise zero and error.
    fn instruction_fetch(&mut self) {
        self.state.instruction = match self.memory.load_word(self.registers.pc) {
            Ok(data) => data,
            Err(e) => {
                error(e.as_str());
                0
            }
        }
    }

    /// Decode an instruction into its individual fields.
    fn instruction_decode(&mut self) {
        self.instruction = Instruction::from(self.state.instruction);

        // Set the data lines based on the contents of the instruction.
        // Some lines will hold uninitialized values as a result.
        match self.instruction {
            Instruction::RType(r) => {
                self.state.rs = r.rs as u32;
                self.state.rt = r.rt as u32;
                self.state.rd = r.rd as u32;

                self.state.shamt = r.shamt as u32;
                self.state.funct = r.funct as u32;
            }
            Instruction::IType(i) => {
                self.state.rs = i.rs as u32;
                self.state.rt = i.rt as u32;
                self.state.rd = 0; // Placeholder
                self.state.imm = i.immediate as u32;
            }
            // For instructions that exclusively use the FPU, these data lines
            // do not need to be used.
            Instruction::FpuRType(_) => (),
            _ => unimplemented!(),
        }
    }

    /// Extend the sign of a 16-bit value to the other 48 bits of a
    /// 64-bit value.
    fn sign_extend(&mut self) {
        self.state.sign_extend = self.state.imm as i16 as i64 as u64;
    }

    /// Set rtype control signals. This function may have a Match statement added
    /// in the future for dealing with different special case rtype instructions.
    fn set_rtype_control_signals(&mut self, r: RType) {
        self.signals.alu_op = AluOp::UseFunctField;
        self.signals.alu_src = AluSrc::ReadRegister2;
        self.signals.branch = Branch::NoBranch;
        self.signals.imm_shift = ImmShift::Shift0;
        self.signals.jump = Jump::NoJump;
        self.signals.mem_read = MemRead::NoRead;
        self.signals.mem_to_reg = MemToReg::UseAlu;
        self.signals.mem_write = MemWrite::NoWrite;
        self.signals.mem_write_src = MemWriteSrc::PrimaryUnit;
        self.signals.reg_dst = RegDst::Reg3;
        self.signals.reg_write = RegWrite::YesWrite;

        // The RegWidth signal might differ depending on the
        // specific R-type instruction.
        match reg_width_by_funct(r.funct) {
            Some(width) => self.signals.reg_width = width,
            None => unimplemented!(
                "funct code `{}` is unsupported for this opcode ({})",
                r.funct,
                r.op
            ),
        }
    }

    /// Set the control signals for the datapath, specifically in the
    /// case where the instruction is an I-type.
    fn set_itype_control_signals(&mut self, i: IType) {
        match i.op {
            OPCODE_ORI => {
                self.signals.alu_op = AluOp::Or;
                self.signals.alu_src = AluSrc::ZeroExtendedImmediate;
                self.signals.branch = Branch::NoBranch;
                self.signals.imm_shift = ImmShift::Shift0;
                self.signals.jump = Jump::NoJump;
                self.signals.mem_read = MemRead::NoRead;
                self.signals.mem_to_reg = MemToReg::UseAlu;
                self.signals.mem_write = MemWrite::NoWrite;
                self.signals.mem_write_src = MemWriteSrc::PrimaryUnit;
                self.signals.reg_dst = RegDst::Reg2;
                self.signals.reg_width = RegWidth::DoubleWord;
                self.signals.reg_write = RegWrite::YesWrite;
            }

            OPCODE_LW => {
                self.signals.alu_op = AluOp::Addition;
                self.signals.alu_src = AluSrc::SignExtendedImmediate; // may  be fishy
                self.signals.branch = Branch::NoBranch;
                self.signals.imm_shift = ImmShift::Shift0;
                self.signals.jump = Jump::NoJump;
                self.signals.mem_read = MemRead::YesRead;
                self.signals.mem_to_reg = MemToReg::UseMemory;
                self.signals.mem_write = MemWrite::NoWrite;
                self.signals.mem_write_src = MemWriteSrc::PrimaryUnit;
                self.signals.reg_dst = RegDst::Reg2;
                self.signals.reg_width = RegWidth::Word;
                self.signals.reg_write = RegWrite::YesWrite;
            }

            OPCODE_SW => {
                self.signals.alu_op = AluOp::Addition;
                self.signals.alu_src = AluSrc::SignExtendedImmediate; // may  be fishy
                self.signals.branch = Branch::NoBranch;
                self.signals.imm_shift = ImmShift::Shift0;
                self.signals.jump = Jump::NoJump;
                self.signals.mem_read = MemRead::NoRead;
                self.signals.mem_to_reg = MemToReg::UseMemory; // don't care
                self.signals.mem_write = MemWrite::YesWrite;
                self.signals.mem_write_src = MemWriteSrc::PrimaryUnit;
                self.signals.reg_dst = RegDst::Reg2;
                self.signals.reg_width = RegWidth::Word;
                self.signals.reg_write = RegWrite::NoWrite;
            }
            _ => unimplemented!("I-type instruction with opcode `{}`", i.op),
        }
    }

    /// Set the control signals for the datapath based on the
    /// instruction's opcode.
    fn set_control_signals(&mut self) {
        match self.instruction {
            Instruction::RType(r) => {
                self.set_rtype_control_signals(r);
            }
            Instruction::IType(i) => {
                self.set_itype_control_signals(i);
            }
            Instruction::JType(_) => todo!("JType instructions are not supported yet"),
            Instruction::FpuRType(_) => {
                self.signals = ControlSignals {
                    branch: Branch::NoBranch,
                    jump: Jump::NoJump,
                    mem_read: MemRead::NoRead,
                    mem_write: MemWrite::NoWrite,
                    reg_write: RegWrite::NoWrite,
                    ..Default::default()
                };
            } // _ => panic!("Unsupported instruction")
        }
    }

    /// Read the registers as specified from the instruction and pass
    /// the data into the datapath.
    fn read_registers(&mut self) {
        self.state.read_data_1 = self.registers.gpr[self.state.rs as usize];
        self.state.read_data_2 = self.registers.gpr[self.state.rt as usize];

        // Truncate the variable data if a 32-bit word is requested.
        if let RegWidth::Word = self.signals.reg_width {
            self.state.read_data_1 = self.state.read_data_1 as u32 as u64;
            self.state.read_data_2 = self.state.read_data_2 as u32 as u64;
        }
    }

    /// Set the ALU control signal based on the [`AluOp`] signal.
    fn set_alu_control(&mut self) {
        self.signals.alu_control = match self.signals.alu_op {
            AluOp::Addition => AluControl::Addition,
            AluOp::Subtraction => AluControl::Subtraction,
            AluOp::SetOnLessThanSigned => AluControl::SetOnLessThanSigned,
            AluOp::SetOnLessThanUnsigned => AluControl::SetOnLessThanUnsigned,
            AluOp::And => AluControl::And,
            AluOp::Or => AluControl::Or,
            AluOp::LeftShift16 => AluControl::LeftShift16,
            AluOp::UseFunctField => {
                match self.state.funct as u8 {
                    FUNCT_ADD | FUNCT_DADD => AluControl::Addition,
                    FUNCT_SUB | FUNCT_DSUB => AluControl::Subtraction,
                    FUNCT_AND => AluControl::And,
                    FUNCT_OR => AluControl::Or,
                    FUNCT_SLT => AluControl::SetOnLessThanSigned,
                    FUNCT_SLTU => AluControl::SetOnLessThanUnsigned,
                    FUNCT_SOP32 | FUNCT_SOP36 => match self.state.shamt as u8 {
                        ENC_DIV => AluControl::DivisionSigned,
                        _ => {
                            unimplemented!("MIPS Release 6 encoding `{}` unsupported for this function code ({})", self.state.shamt, self.state.funct);
                        }
                    },
                    FUNCT_SOP33 | FUNCT_SOP37 => match self.state.shamt as u8 {
                        ENC_DIVU => AluControl::DivisionUnsigned,
                        _ => {
                            unimplemented!("MIPS Release 6 encoding `{}` unsupported for this function code ({})", self.state.shamt, self.state.funct);
                        }
                    },
                    FUNCT_SOP30 | FUNCT_SOP34 => match self.state.shamt as u8 {
                        ENC_MUL => AluControl::MultiplicationSigned,
                        _ => {
                            unimplemented!("MIPS Release 6 encoding `{}` unsupported for this function code ({})", self.state.shamt, self.state.funct);
                        }
                    },
                    FUNCT_SOP31 | FUNCT_SOP35 => match self.state.shamt as u8 {
                        ENC_MULU => AluControl::MultiplicationUnsigned,
                        _ => {
                            unimplemented!("MIPS Release 6 encoding `{}` unsupported for this function code ({})", self.state.shamt, self.state.funct);
                        }
                    },
                    _ => {
                        unimplemented!("funct code `{}` is unsupported on ALU", self.state.funct);
                    }
                }
            }
        };
    }

    /// Perform an ALU operation.
    ///
    /// **Implementation Note:** Unlike the MIPS64 specification, this ALU
    /// does not handle integer overflow exceptions. Should this be implemented
    /// in the future, the ALU should be adjusted accordingly to address this.
    fn alu(&mut self) {
        // Left shift the immediate value based on the ImmShift control signal.
        let alu_immediate = match self.signals.imm_shift {
            ImmShift::Shift0 => self.state.sign_extend,
            ImmShift::Shift16 => self.state.sign_extend << 16,
            ImmShift::Shift32 => self.state.sign_extend << 32,
            ImmShift::Shift48 => self.state.sign_extend << 48,
        };

        // Specify the inputs for the operation. The first will always
        // be the first register, but the second may be either the
        // second register, the sign-extended immediate value, or the
        // zero-extended immediate value.
        let mut input1 = self.state.read_data_1;
        let mut input2 = match self.signals.alu_src {
            AluSrc::ReadRegister2 => self.state.read_data_2,
            AluSrc::SignExtendedImmediate => alu_immediate,
            AluSrc::ZeroExtendedImmediate => self.state.imm as u64,
        };

        // Truncate the inputs if 32-bit operations are expected.
        if let RegWidth::Word = self.signals.reg_width {
            input1 = input1 as i32 as u64;
            input2 = input2 as i32 as u64;
        }

        // Set the result.
        self.state.alu_result = match self.signals.alu_control {
            AluControl::Addition => input1.wrapping_add(input2),
            AluControl::Subtraction => (input1 as i64).wrapping_sub(input2 as i64) as u64,
            AluControl::SetOnLessThanSigned => ((input1 as i64) < (input2 as i64)) as u64,
            AluControl::SetOnLessThanUnsigned => (input1 < input2) as u64,
            AluControl::And => input1 & input2,
            AluControl::Or => input1 | input2,
            AluControl::LeftShift16 => input2 << 16,
            AluControl::Not => !input1,
            AluControl::MultiplicationSigned => ((input1 as i128) * (input2 as i128)) as u64,
            AluControl::MultiplicationUnsigned => ((input1 as u128) * (input2 as u128)) as u64,
            AluControl::DivisionSigned => {
                if input2 == 0 {
                    0
                } else {
                    ((input1 as i64) / (input2 as i64)) as u64
                }
            }
            AluControl::DivisionUnsigned => {
                if input2 == 0 {
                    0
                } else {
                    input1 / input2
                }
            }
        };

        // Truncate and sign-extend the output if 32-bit operations are expected.
        if let RegWidth::Word = self.signals.reg_width {
            self.state.alu_result = self.state.alu_result as i32 as i64 as u64;
        }

        // TODO: Set the zero bit.
    }

    /// Read from memory based on the address provided by the ALU in
    /// [`DatapathState::alu_result`]. Returns the result to [`DatapathState::memory_data`].
    /// Should the address be invalid or otherwise memory cannot be
    /// read at the given address, bitwise 0 will be used in lieu of
    /// any data.
    fn memory_read(&mut self) {
        let address = self.state.alu_result;

        // Load memory, first choosing the correct load function by the
        // RegWidth control signal, then reading the result from this
        // memory access.
        self.state.memory_data = match self.signals.reg_width {
            RegWidth::Word => self.memory.load_word(address).unwrap_or(0) as u64,
            RegWidth::DoubleWord => self.memory.load_double_word(address).unwrap_or(0),
        };
    }

    /// Write to memory based on the address provided by the ALU in
    /// [`DatapathState::alu_result`]. The source of the data being written to
    /// memory is determined by [`MemWriteSrc`].
    fn memory_write(&mut self) {
        let address = self.state.alu_result;

        let write_data = match self.signals.mem_write_src {
            MemWriteSrc::PrimaryUnit => self.state.read_data_2,
            // Awaiting implementation of the floating-point unit.
            MemWriteSrc::FloatingPointUnit => todo!(),
        };

        // Choose the correct store function based on the RegWidth
        // control signal.
        match self.signals.reg_width {
            RegWidth::Word => {
                self.memory.store_word(address, write_data as u32).ok();
            }
            RegWidth::DoubleWord => {
                self.memory.store_double_word(address, write_data).ok();
            }
        };
    }

    /// Write to a register. This will only write if the RegWrite
    /// control signal is set.
    fn register_write(&mut self) {
        // Determine what data will be sent to the register: either
        // the result from the ALU, or data retrieved from memory.
        self.state.data_result = match self.signals.mem_to_reg {
            MemToReg::UseAlu => self.state.alu_result,
            MemToReg::UseMemory => self.state.memory_data,
        };

        // Decide to retrieve data either from the main processor or the coprocessor.
        self.state.register_write_data = match self.coprocessor.signals.data_write {
            DataWrite::NoWrite => self.state.data_result,
            DataWrite::YesWrite => self.coprocessor.get_data_register(),
        };

        // Abort if the RegWrite signal is not set.
        if self.signals.reg_write == RegWrite::NoWrite {
            return;
        }

        // Determine the destination for the data to write. This is
        // determined by the RegDst control signal.
        let destination = match self.signals.reg_dst {
            RegDst::Reg1 => self.state.rs as usize,
            RegDst::Reg2 => self.state.rt as usize,
            RegDst::Reg3 => self.state.rd as usize,
        };

        // If we are attempting to write to register $zero, stop.
        if destination == 0 {
            return;
        }

        // If a 32-bit word is requested, ensure data is truncated and sign-extended.
        if let RegWidth::Word = self.signals.reg_width {
            self.state.data_result = self.state.data_result as i32 as u64;
        }

        // Write.
        self.registers.gpr[destination] = self.state.register_write_data;
    }

    /// Update the program counter register. At the moment, this only
    /// increments the PC by 4 and does not support branching or
    /// jumping.
    fn set_pc(&mut self) {
        self.registers.pc += 4;
    }
}
