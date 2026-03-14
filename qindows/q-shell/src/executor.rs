//! # Q-Shell Execution Engine (Microkernel IPC Broker)
//!
//! Executes parsed command pipelines by routing them through
//! the appropriate subsystem (Prism, Aether, Nexus, etc.)
//! via Q-Ring IPC system calls.

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Result of executing a command.
#[derive(Debug)]
pub enum CommandResult {
    /// Success with optional output text
    Success(Option<String>),
    /// Error with message
    Error(String),
    /// Output is a list of items (for piping to next command)
    List(Vec<String>),
    /// Output is structured data (for ~> flow operator)
    Data(Vec<(String, String)>),
    /// Command did not produce output (side effect only)
    Silent,
    /// Request to exit Q-Shell
    Exit,
}

/// A generalized Remote Procedure Call (RPC) broker for Q-Shell.
/// Pipes parsed commands to the Kernel's IPC / Syscall system.
pub struct SyscallBroker;

impl SyscallBroker {
    /// Transmit a command string to the kernel.
    pub fn dispatch_command(cmd: &str, args: &[&str]) -> CommandResult {
        let mut full_cmd = String::from(cmd);
        for arg in args {
            full_cmd.push(' ');
            full_cmd.push_str(arg);
        }
        
        // Emulate a blocking IPC call to the kernel via Syscall 23 (QRingSendBatch)
        let mut result: i64;
        let ptr = full_cmd.as_ptr() as u64;
        let len = full_cmd.len() as u64;
        
        unsafe { 
            core::arch::asm!(
                "syscall",
                in("rax") 23, // Syscall::QRingSendBatch
                in("rdi") 1,  // Channel 1 (Prism Daemon)
                in("rsi") ptr,
                in("rdx") len,
                out("rcx") _,
                out("r11") _,
                lateout("rax") result,
            ); 
        }

        if result < 0 {
            CommandResult::Error(alloc::format!("Kernel IPC Error: Syscall 23 failed ({})", result))
        } else {
            CommandResult::Success(Some(alloc::format!("🗲 Kernel Ack: Processed '{}' over IPC! Output size: {} bytes", full_cmd, result)))
        }
    }
}

/// Built-in command handlers.
pub fn execute_builtin(cmd: &str, args: &[&str]) -> CommandResult {
    match cmd {
        "help" | "?" => cmd_help(),
        "exit" | "quit" | "logout" => CommandResult::Exit,
        // All other commands (prism, aether, nexus, sentinel, nvme, tcp, etc.)
        // are dispatched securely across the microkernel Ring 3 boundary via IPC!
        _ => SyscallBroker::dispatch_command(cmd, args),
    }
}

fn cmd_help() -> CommandResult {
    CommandResult::Success(Some(String::from(
        "Q-Shell v1.0.0 — Semantic Command Palette (Ring 3)\n\
         \n\
         All execution logic has been decoupled to the Kernel Daemons.\n\
         Commands typed here are routed over Syscalls (IPC) for capability checking.\n\
         \n\
         Built-in:\n  \
           help, ?      Show this message\n  \
           exit, quit   Exit the shell\n\
         \n\
         Try testing IPC by typing `prism` or `nexus`."
    )))
}
