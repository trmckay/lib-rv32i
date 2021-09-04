use log::info;
use std::collections::HashMap;

use lib_rv32_common::constants::*;

use crate::{
    encode_b_imm, encode_func3, encode_func7, encode_i_imm, encode_j_imm, encode_opcode, encode_rd,
    encode_rs1, encode_rs2, encode_s_imm, encode_u_imm, error::AssemblerError, match_func3,
    match_func7, parse::*, tokenize,
};

enum InstructionFormat {
    Itype,
    Rtype,
    Jtype,
    Utype,
    Stype,
    Btype,
}

pub fn parse_labels(program: &str) -> HashMap<String, u32> {
    let mut pc: u32 = 0;
    let mut labels = HashMap::new();

    for line in program.split('\n') {
        let tokens: Vec<String> = tokenize!(line);

        if !tokens.is_empty() {
            if tokens[0].ends_with(':') {
                labels.insert(tokens[0].strip_suffix(':').unwrap().to_owned(), pc);

                if tokens.len() > 1 {
                    pc += 4;
                }
            } else {
                pc += 4;
            }
        }
    }
    labels
}

/// Assemble a single instruction.
///
/// Parameters:
///     `ir_string: &str`: The instruction
///     `labels: &mut std::collections::HashMap<String, u32>`: Map of labels
///     `pc: u32` Current location of the program
///
/// Returns:
///     `Result<Option<u32>>`: The assembled binary instruction, an error, or nothing.
pub fn assemble_ir(
    ir_string: &str,
    labels: &HashMap<String, u32>,
    pc: &mut u32,
) -> Result<Vec<u32>, AssemblerError> {
    let mut msg = String::new();
    let mut line_tokens: Vec<String> = tokenize!(ir_string);
    let mut binaries: Vec<u32> = Vec::new();

    if line_tokens.is_empty() {
        return Ok(vec![]);
    } else if line_tokens.len() > 5 {
        return Err(AssemblerError::TooManyTokensError);
    }

    // Strip leading label.
    if line_tokens[0].ends_with(':') {
        line_tokens.remove(0);
    }

    if line_tokens.is_empty() {
        return Ok(vec![]);
    }

    let base_instructions = transform_psuedo_ir(&line_tokens);
    if let Err(why) = base_instructions {
        return Err(why);
    }
    let base_instructions = base_instructions.unwrap();
    for ir_tokens in base_instructions {
        let op = &ir_tokens[0][..];
        let opcode = match_opcode(op);
        if let Err(why) = opcode {
            return Err(why);
        }
        let opcode = opcode.unwrap();

        let mut ir: u32 = 0;
        ir |= encode_opcode!(opcode);

        // Use the opcode to identify the instruction format.
        let format = match opcode {
            OPCODE_ARITHMETIC_IMM | OPCODE_JALR | OPCODE_LOAD => InstructionFormat::Itype,
            OPCODE_ARITHMETIC => InstructionFormat::Rtype,
            OPCODE_JAL => InstructionFormat::Jtype,
            OPCODE_LUI | OPCODE_AUIPC => InstructionFormat::Utype,
            OPCODE_BRANCH => InstructionFormat::Btype,
            OPCODE_STORE => InstructionFormat::Stype,
            _ => unreachable!(),
        };

        // Use the destination register field.
        if let InstructionFormat::Rtype | InstructionFormat::Itype | InstructionFormat::Utype =
            format
        {
            let rd = match_register(&ir_tokens[1]);
            if let Err(why) = rd {
                return Err(why);
            }
            ir |= encode_rd!(rd.unwrap());
        }

        // Use the first register operand and func3 fields.
        if let InstructionFormat::Itype
        | InstructionFormat::Rtype
        | InstructionFormat::Btype
        | InstructionFormat::Stype = format
        {
            let rs1 = match_register(
                &ir_tokens[match opcode {
                    OPCODE_LOAD => 3,
                    OPCODE_BRANCH => 1,
                    _ => 2,
                }],
            );
            if let Err(why) = rs1 {
                return Err(why);
            }
            ir |= encode_rs1!(rs1.unwrap());

            ir |= encode_func3!(match_func3!(op));
        }

        // Use the second register operand field.
        if let InstructionFormat::Rtype | InstructionFormat::Stype | InstructionFormat::Btype =
            format
        {
            let rs2 = match_register(
                &ir_tokens[match opcode {
                    OPCODE_STORE => 1,
                    OPCODE_BRANCH => 2,
                    _ => 3,
                }],
            );
            if let Err(why) = rs2 {
                return Err(why);
            }
            ir |= encode_rs2!(rs2.unwrap());
        }

        // Use the func7 field.
        if let InstructionFormat::Rtype = format {
            ir |= encode_func7!(match_func7!(op));
        }

        match format {
            InstructionFormat::Itype => {
                let imm = parse_imm(
                    &ir_tokens[match opcode {
                        OPCODE_LOAD => 2,
                        _ => 3,
                    }],
                    labels,
                    *pc,
                );
                if let Err(why) = imm {
                    return Err(why);
                }
                let imm = imm.unwrap();
                ir |= encode_i_imm!(imm);
            }
            InstructionFormat::Utype => {
                let imm = parse_imm(&ir_tokens[2], labels, *pc);
                if let Err(why) = imm {
                    return Err(why);
                }
                let imm = imm.unwrap();
                ir |= encode_u_imm!(imm);
            }
            InstructionFormat::Jtype => {
                let imm = parse_imm(&ir_tokens[2], labels, *pc);
                if let Err(why) = imm {
                    return Err(why);
                }
                let imm = imm.unwrap();
                ir |= encode_j_imm!(imm);
            }
            InstructionFormat::Btype => {
                let imm = parse_imm(&ir_tokens[3], labels, *pc);
                if let Err(why) = imm {
                    return Err(why);
                }
                let imm = imm.unwrap();
                ir |= encode_b_imm!(imm);
            }
            InstructionFormat::Stype => {
                let imm = parse_imm(&ir_tokens[2], labels, *pc);
                if let Err(why) = imm {
                    return Err(why);
                }
                let imm = imm.unwrap();
                ir |= encode_s_imm!(imm);
            }
            InstructionFormat::Rtype => (),
        }

        msg += &format!("{:08x}", ir);

        binaries.push(ir);
        *pc += 4;
    }

    Ok(binaries)
}

/// Assemble a full program of newline-separated instructions.
pub fn assemble_program(program: &str) -> Result<Vec<u32>, AssemblerError> {
    let mut prog = Vec::new();
    let mut pc: u32 = 0;

    let labels = parse_labels(program);

    for line in program.split('\n') {
        let instructions = assemble_ir(line, &labels, &mut pc);

        if let Err(why) = instructions {
            return Err(why);
        } else {
            for ir in instructions.unwrap() {
                prog.push(ir);
            }
        }
    }

    Ok(prog)
}
